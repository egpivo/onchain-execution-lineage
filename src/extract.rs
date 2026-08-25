//! Raw provider evidence → [`ExecutionContext`] → [`LineageBundle`].
//!
//! This is the path that removes the frontend and the Python generators from
//! the critical path: hand it a recorded provider response and Rust does the
//! rest — adapter selection, provider extraction, transaction discovery,
//! generic Solana extraction, lineage construction.
//!
//! Nothing here makes a network request unless an [`RpcContext`] is supplied,
//! and no stage is invented: a missing input produces a missing stage.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::adapters::{self, ProviderId, RawProviderArtifact};
use crate::artifact::{sha256_bytes, ArtifactManifest};
use crate::execution_context::{ExecutionContext, Stage, StageProvenance};
use crate::lineage_builder::{build_lineage, derive_artifact_id};
use crate::lineage_model::LineageBundle;
use crate::solana::{RpcContext, SolanaExtractor};

#[derive(Default)]
pub struct ExtractInputs<'a> {
    /// Explicit provider, or None to detect from the artifact's shape.
    pub provider: Option<ProviderId>,
    /// Raw provider response JSON.
    pub response_path: Option<&'a Path>,
    /// Base64 transaction file. Overrides any payload inline in the response.
    pub transaction_b64_path: Option<&'a Path>,
    /// Optional manifest, used only for identity and provenance.
    pub manifest: Option<&'a ArtifactManifest>,
    /// Signature of a landed transaction, if one is known.
    pub signature: Option<&'a str>,
    pub rpc: Option<RpcContext>,
    /// Fetch settlement metadata for `signature`. Requires `rpc`.
    pub enrich_settlement: bool,
}

pub struct Extraction {
    pub context: ExecutionContext,
    pub lineage: LineageBundle,
}

impl Extraction {
    /// Deterministic per-run output directory, derived from the artifact id.
    pub fn default_out_dir(&self) -> PathBuf {
        PathBuf::from("artifacts/lineage").join(&self.context.provenance.artifact_id)
    }

    pub fn write_to_dir(&self, dir: &Path) -> Result<WrittenPaths> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create output directory {}", dir.display()))?;
        let context_path = dir.join("context.json");
        let lineage_path = dir.join("lineage.json");
        std::fs::write(&context_path, self.context.to_canonical_json()?)?;
        std::fs::write(&lineage_path, self.lineage.to_canonical_json()?)?;
        Ok(WrittenPaths {
            context: context_path,
            lineage: lineage_path,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct WrittenPaths {
    pub context: PathBuf,
    pub lineage: PathBuf,
}

pub async fn extract(inputs: ExtractInputs<'_>) -> Result<Extraction> {
    let mut raw: Option<RawProviderArtifact> = None;

    if let Some(path) = inputs.response_path {
        let bytes = std::fs::read(path)
            .with_context(|| format!("read provider response {}", path.display()))?;
        raw = Some(
            RawProviderArtifact::from_bytes(&bytes)
                .with_context(|| format!("parse provider response {}", path.display()))?
                .with_source_path(path.display().to_string()),
        );
    }

    // Provider identity: explicit flag, else adapter detection, else the
    // manifest's provider string, else generic.
    let extraction = match &raw {
        Some(raw) => Some(adapters::extract(inputs.provider, raw)?),
        None => None,
    };
    let provider = extraction
        .as_ref()
        .map(|e| e.provider)
        .or(inputs.provider)
        .or_else(|| inputs.manifest.and_then(|m| ProviderId::parse(&m.provider)))
        .unwrap_or(ProviderId::Generic);

    let artifact_id = match inputs.manifest {
        Some(m) => m.artifact_id.clone(),
        None => derive_artifact_id(
            provider,
            raw.as_ref()
                .and_then(|r| r.sha256.as_deref())
                .or(inputs.signature),
        ),
    };

    let mut ctx = ExecutionContext::new(provider, artifact_id);
    if let Some(m) = inputs.manifest {
        ctx = ctx.with_manifest(m);
    }
    if let Some(e) = extraction {
        ctx = ctx.with_extraction(e);
    }
    if let Some(raw) = &raw {
        ctx.push_stage_provenance(StageProvenance {
            stage: Stage::ProviderResponse,
            source: "provider_response".into(),
            source_path: raw.source_path.clone(),
            sha256: raw.sha256.clone(),
        });
    }

    // Transaction bytes: an explicit file wins over the inline payload, so a
    // sanitized capture that points at a sibling file still works.
    let mut tx_b64: Option<(String, StageProvenance)> = None;
    if let Some(path) = inputs.transaction_b64_path {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read transaction file {}", path.display()))?;
        let provenance = StageProvenance {
            stage: Stage::TransactionConstruction,
            source: "unsigned_transaction_file".into(),
            source_path: Some(path.display().to_string()),
            sha256: Some(sha256_bytes(text.trim().as_bytes())),
        };
        tx_b64 = Some((text, provenance));
    } else if let Some(payload) = ctx.transaction_ref().and_then(|r| r.payload.clone()) {
        let provenance = StageProvenance {
            stage: Stage::TransactionConstruction,
            source: "provider_response_inline".into(),
            source_path: raw.as_ref().and_then(|r| r.source_path.clone()),
            sha256: Some(sha256_bytes(payload.trim().as_bytes())),
        };
        tx_b64 = Some((payload, provenance));
    } else if let (Some(sig), Some(rpc)) = (inputs.signature, inputs.rpc.as_ref()) {
        let b64 = crate::rpc::fetch_transaction_base64(&rpc.rpc_url, sig).await?;
        let provenance = StageProvenance {
            stage: Stage::TransactionConstruction,
            source: "rpc_get_transaction".into(),
            source_path: None,
            sha256: Some(sha256_bytes(b64.trim().as_bytes())),
        };
        tx_b64 = Some((b64, provenance));
    }

    if let Some((b64, provenance)) = tx_b64 {
        let extractor = match inputs.rpc.clone() {
            Some(rpc) => SolanaExtractor::with_rpc(rpc),
            None => SolanaExtractor::offline(),
        };
        let construction = extractor.extract_base64(&b64).await?;
        ctx = ctx.with_transaction(construction);
        ctx.push_stage_provenance(provenance);
    }

    // Settlement only when a signature exists *and* metadata was fetched.
    if let (Some(sig), Some(rpc), true) = (
        inputs.signature,
        inputs.rpc.as_ref(),
        inputs.enrich_settlement,
    ) {
        let mut scratch = LineageBundle::new(Default::default());
        crate::settlement::enrich_settlement(&mut scratch, &rpc.rpc_url, sig).await?;
        ctx = ctx.with_settlement(scratch.settlement);
        ctx.push_stage_provenance(StageProvenance {
            stage: Stage::Settlement,
            source: "rpc_get_transaction_meta".into(),
            source_path: None,
            sha256: None,
        });
    } else if let Some(sig) = inputs.signature {
        // Record the pointer without letting it masquerade as an observation.
        ctx = ctx.with_settlement(crate::lineage_model::SettlementObservation {
            applicable: false,
            signature: Some(sig.to_string()),
            notes: vec![
                "signature supplied but settlement metadata was not fetched; no settlement \
                 claim is available"
                    .into(),
            ],
            ..Default::default()
        });
    }

    let lineage = build_lineage(&ctx)?;
    Ok(Extraction {
        context: ctx,
        lineage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("extract_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[tokio::test]
    async fn extracts_response_only_lineage_without_network() {
        let path = write_temp(
            "response.json",
            r#"{"inputMint":"A","inAmount":"1","outputMint":"B","outAmount":"2",
                "otherAmountThreshold":"1","minOutAmount":"1","slippageBps":50,
                "routePlan":[{"venue":"V","inputMint":"A","outputMint":"B"}],
                "requestId":"r1"}"#,
        );
        let out = extract(ExtractInputs {
            provider: Some(ProviderId::Dflow),
            response_path: Some(&path),
            ..Default::default()
        })
        .await
        .unwrap();

        assert_eq!(out.context.provider, ProviderId::Dflow);
        assert!(out.context.transaction.is_none());
        assert!(out.context.settlement.is_none());
        assert!(!out.lineage.settlement.applicable);
        // Identity is content-derived, so re-running yields the same id.
        assert!(out.context.provenance.artifact_id.starts_with("dflow_"));
    }

    #[tokio::test]
    async fn artifact_id_is_deterministic_for_identical_bytes() {
        let path = write_temp("det.json", r#"{"routePlan":[],"requestId":"x"}"#);
        let a = extract(ExtractInputs {
            response_path: Some(&path),
            ..Default::default()
        })
        .await
        .unwrap();
        let b = extract(ExtractInputs {
            response_path: Some(&path),
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(
            a.context.provenance.artifact_id,
            b.context.provenance.artifact_id
        );
        assert_eq!(
            a.lineage.to_canonical_json().unwrap(),
            b.lineage.to_canonical_json().unwrap()
        );
    }

    #[tokio::test]
    async fn signature_without_enrichment_makes_no_settlement_claim() {
        let path = write_temp("sig.json", r#"{"routePlan":[],"requestId":"x"}"#);
        let out = extract(ExtractInputs {
            response_path: Some(&path),
            signature: Some("5xY"),
            ..Default::default()
        })
        .await
        .unwrap();

        assert!(!out.context.has_settlement_evidence());
        assert!(!out.lineage.settlement.applicable);
        out.lineage
            .assert_unsigned_has_no_settlement_claims()
            .unwrap();
    }
}
