//! Trace: *explain* a lineage. It no longer builds one of its own.
//!
//! A manifest is one more ingestion source, not a second construction path.
//! Everything here funnels into the single canonical pipeline:
//!
//! ```text
//! manifest / provider JSON / tx / signature
//!   → RawProviderArtifact
//!   → provider extraction (adapters/)
//!   → ExecutionContext
//!   → Solana extraction (solana/)
//!   → LineageBuilder
//!   → LineageBundle
//! ```
//!
//! [`build_trace`] keeps its old signature so existing callers are unaffected;
//! it now delegates to [`crate::extract`] and returns the bundle that pipeline
//! produced. Provider normalization, transaction decoding and cross-stage
//! linking are all owned elsewhere — this module contributes ingestion glue
//! and nothing more.

use anyhow::Result;
use std::path::Path;

use crate::artifact::ArtifactManifest;
use crate::extract::{self, Extraction};
use crate::lineage_model::LineageBundle;
use crate::solana::RpcContext;

// Re-exported so callers that reach for a decoded transaction through `trace`
// still find it. The implementation lives with the lineage builder.
pub use crate::lineage_builder::apply_decoded_transaction;

pub struct TraceInputs<'a> {
    pub manifest: Option<&'a ArtifactManifest>,
    pub provider_json_path: Option<&'a Path>,
    pub transaction_b64_path: Option<&'a Path>,
    pub signature: Option<&'a str>,
    pub rpc_url: &'a str,
    pub resolve_alts: bool,
    pub enrich_settlement: bool,
}

impl<'a> TraceInputs<'a> {
    /// Ingestion mapping: trace flags → canonical extraction inputs.
    fn into_extract_inputs(self) -> extract::ExtractInputs<'a> {
        extract::ExtractInputs {
            // Provider identity comes from the artifact or the manifest, never
            // from a trace-local guess.
            provider: None,
            response_path: self.provider_json_path,
            transaction_b64_path: self.transaction_b64_path,
            manifest: self.manifest,
            signature: self.signature,
            rpc: Some(RpcContext {
                rpc_url: self.rpc_url.to_string(),
                resolve_alts: self.resolve_alts,
                // Account facts are what owner-derived integrator markers need,
                // and resolving tables is a precondition for having any.
                fetch_account_facts: self.resolve_alts,
            }),
            enrich_settlement: self.enrich_settlement,
        }
    }
}

/// Build the lineage for a trace. Thin wrapper over the canonical pipeline.
pub async fn build_trace(inputs: TraceInputs<'_>) -> Result<LineageBundle> {
    Ok(build_trace_full(inputs).await?.lineage)
}

/// Same pipeline, keeping the [`crate::execution_context::ExecutionContext`]
/// so the CLI can write it out and `verify` can read it back.
pub async fn build_trace_full(inputs: TraceInputs<'_>) -> Result<Extraction> {
    extract::extract(inputs.into_extract_inputs()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_context::Stage;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("trace_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[tokio::test]
    async fn trace_output_carries_links_and_provenance() {
        let path = write_temp(
            "order.json",
            r#"{"inputMint":"A","inAmount":"1","outputMint":"B","outAmount":"2",
                "otherAmountThreshold":"1","minOutAmount":"1","slippageBps":50,
                "routePlan":[{"venue":"V","inputMint":"A","outputMint":"B"}],
                "requestId":"r1"}"#,
        );

        let out = build_trace_full(TraceInputs {
            manifest: None,
            provider_json_path: Some(&path),
            transaction_b64_path: None,
            signature: None,
            rpc_url: "http://127.0.0.1:9",
            resolve_alts: false,
            enrich_settlement: false,
        })
        .await
        .unwrap();

        // The old trace path produced no links at all.
        assert!(!out.lineage.links.is_empty());
        assert!(out
            .context
            .provenance
            .stages
            .iter()
            .any(|p| p.stage == Stage::ProviderResponse));
        assert_eq!(out.lineage.capture.provider, "dflow");
        assert_eq!(
            out.lineage.raw_extensions.get("_adapter"),
            Some(&serde_json::json!("dflow"))
        );
    }

    /// The manifest supplies identity and provenance, not normalization.
    #[tokio::test]
    async fn manifest_supplies_identity_only() {
        let path = write_temp("q.json", r#"{"routePlan":[],"requestId":"r"}"#);
        let raw_for_hash = write_temp("hashed.json", "{}");
        let manifest = ArtifactManifest {
            schema_version: crate::evidence::ARTIFACT_SCHEMA_VERSION.into(),
            artifact_id: "art_from_manifest".into(),
            capture_run_id: "run".into(),
            matched_set_id: None,
            provider: "dflow".into(),
            surface: "dev_quote".into(),
            endpoint_type: crate::artifact::EndpointType::Developer,
            endpoint_hostname: "example.invalid".into(),
            authentication_mode: "none".into(),
            captured_at_utc: "2026-07-29T12:34:22Z".into(),
            pair: "USDC/SOL".into(),
            input_mint: "A".into(),
            output_mint: "B".into(),
            raw_input_amount: "1".into(),
            slippage_configuration: "50_bps".into(),
            raw_artifact_path: raw_for_hash.display().to_string(),
            raw_artifact_sha256: "a".repeat(64),
            sanitized_artifact_path: String::new(),
            sanitization_status: crate::artifact::SanitizationStatus::NotRequired,
            transaction_presence: crate::artifact::TransactionPresence::Absent,
            signature: None,
            source_notes: "unit test".into(),
        };

        let out = build_trace_full(TraceInputs {
            manifest: Some(&manifest),
            provider_json_path: Some(&path),
            transaction_b64_path: None,
            signature: None,
            rpc_url: "http://127.0.0.1:9",
            resolve_alts: false,
            enrich_settlement: false,
        })
        .await
        .unwrap();

        assert_eq!(out.lineage.capture.artifact_id, "art_from_manifest");
        // Manifest surface wins over the adapter's inference.
        assert_eq!(out.lineage.capture.surface, "dev_quote");
        assert_eq!(out.lineage.capture.pair, "USDC/SOL");
        assert_eq!(out.lineage.capture.captured_at_utc, "2026-07-29T12:34:22Z");
    }
}
