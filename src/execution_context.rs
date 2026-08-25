//! Normalized execution state.
//!
//! [`ExecutionContext`] is what the verifier core operates on. Every stage is
//! optional, so the same model covers a response with nothing else, a response
//! plus an unsigned transaction, a transaction on its own, and a full
//! intent → settlement lineage. An absent stage means "not observed", never
//! "observed empty".
//!
//! A manifest is one convenient way to fill in provenance. It is not required,
//! and it is not the domain model.

use serde::{Deserialize, Serialize};

use crate::adapters::{
    ExecutionIntent, ProviderExtraction, ProviderId, ProviderResponse, UnsignedTransactionRef,
};
use crate::artifact::ArtifactManifest;
use crate::lineage_model::{RouteObservation, SettlementObservation};
use crate::solana::TransactionObservation;

pub const EXECUTION_CONTEXT_SCHEMA_VERSION: &str = "1.0.0";

/// Which stage an observation, link or check result belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Intent,
    ProviderResponse,
    Route,
    TransactionConstruction,
    Settlement,
}

impl Stage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Stage::Intent => "intent",
            Stage::ProviderResponse => "provider_response",
            Stage::Route => "route",
            Stage::TransactionConstruction => "transaction_construction",
            Stage::Settlement => "settlement",
        }
    }
}

/// Where a stage's evidence came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageProvenance {
    pub stage: Stage,
    /// `provider_response`, `unsigned_transaction`, `rpc_get_transaction`, …
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Provenance {
    /// Stable, content-derived unless a manifest supplied one.
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pair: Option<String>,
    pub tool: String,
    pub stages: Vec<StageProvenance>,
}

/// Normalized execution state assembled so far.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub schema_version: String,
    pub provider: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<ExecutionIntent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response: Option<ProviderResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settlement: Option<SettlementObservation>,
    pub provenance: Provenance,
    /// Carried through from the adapter so nothing is silently dropped. Core
    /// code must not branch on these.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_extraction: Option<ProviderExtraction>,
}

impl ExecutionContext {
    pub fn new(provider: ProviderId, artifact_id: impl Into<String>) -> Self {
        Self {
            schema_version: EXECUTION_CONTEXT_SCHEMA_VERSION.into(),
            provider,
            intent: None,
            provider_response: None,
            route: None,
            transaction: None,
            settlement: None,
            provenance: Provenance {
                artifact_id: artifact_id.into(),
                surface: None,
                captured_at_utc: None,
                pair: None,
                tool: format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
                stages: Vec::new(),
            },
            provider_extraction: None,
        }
    }

    /// Fill the response-side stages from an adapter extraction.
    pub fn with_extraction(mut self, extraction: ProviderExtraction) -> Self {
        self.provider = extraction.provider;
        // A manifest's declared surface outranks the adapter's inference.
        if self.provenance.surface.is_none() {
            self.provenance.surface = extraction.surface.clone();
        }
        self.intent = extraction.intent.clone();
        self.provider_response = Some(extraction.response.clone());
        self.route = extraction.route.clone();
        self.provider_extraction = Some(extraction);
        self
    }

    pub fn with_transaction(mut self, tx: TransactionObservation) -> Self {
        self.transaction = Some(tx);
        self
    }

    pub fn with_settlement(mut self, settlement: SettlementObservation) -> Self {
        self.settlement = Some(settlement);
        self
    }

    /// Adopt identity/provenance from a manifest when the caller has one.
    pub fn with_manifest(mut self, manifest: &ArtifactManifest) -> Self {
        self.provenance.artifact_id = manifest.artifact_id.clone();
        self.provenance.surface = Some(manifest.surface.clone());
        self.provenance.captured_at_utc = Some(manifest.captured_at_utc.clone());
        self.provenance.pair = Some(manifest.pair.clone());
        self
    }

    pub fn push_stage_provenance(&mut self, p: StageProvenance) {
        self.provenance.stages.push(p);
    }

    /// The unsigned transaction reference the adapter reported, if any.
    pub fn transaction_ref(&self) -> Option<&UnsignedTransactionRef> {
        self.provider_extraction
            .as_ref()
            .and_then(|e| e.transaction.as_ref())
    }

    /// Stages actually populated, in pipeline order.
    pub fn stages_present(&self) -> Vec<Stage> {
        let mut out = Vec::new();
        if self.intent.is_some() {
            out.push(Stage::Intent);
        }
        if self.provider_response.is_some() {
            out.push(Stage::ProviderResponse);
        }
        if self.route.is_some() {
            out.push(Stage::Route);
        }
        if self.transaction.is_some() {
            out.push(Stage::TransactionConstruction);
        }
        if self.settlement.is_some() {
            out.push(Stage::Settlement);
        }
        out
    }

    /// Settlement claims require settlement evidence — a signature alone is a
    /// pointer, not an observation.
    pub fn has_settlement_evidence(&self) -> bool {
        self.settlement
            .as_ref()
            .map(|s| s.applicable && s.signature.is_some())
            .unwrap_or(false)
    }

    pub fn to_canonical_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{ProviderAdapter, RawProviderArtifact};

    #[test]
    fn response_only_context_reports_one_stage_set() {
        let raw = RawProviderArtifact::from_value(serde_json::json!({
            "input_mint": "A", "output_mint": "B", "out_amount": "5"
        }));
        let e = crate::adapters::generic::GenericAdapter
            .extract(&raw)
            .unwrap();
        let ctx = ExecutionContext::new(ProviderId::Generic, "id").with_extraction(e);

        assert_eq!(
            ctx.stages_present(),
            vec![Stage::Intent, Stage::ProviderResponse]
        );
        assert!(ctx.transaction.is_none());
        assert!(!ctx.has_settlement_evidence());
    }

    #[test]
    fn signature_without_settlement_observation_is_not_evidence() {
        let mut ctx = ExecutionContext::new(ProviderId::Dflow, "id");
        ctx.settlement = Some(SettlementObservation {
            applicable: false,
            signature: Some("sig".into()),
            ..Default::default()
        });
        assert!(!ctx.has_settlement_evidence());
    }
}
