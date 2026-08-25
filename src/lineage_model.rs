//! Normalized lineage model shared across providers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::evidence::{AttributionClaim, EvidenceLevel, UnresolvedField, LINEAGE_SCHEMA_VERSION};
use crate::execution_context::Stage;
use crate::transaction::DecodedTransaction;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureMetadata {
    pub artifact_id: String,
    pub provider: String,
    pub surface: String,
    pub captured_at_utc: String,
    pub pair: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuoteObservation {
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub min_out_amount: Option<String>,
    pub request_or_quote_id: Option<String>,
    pub expiry: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeeObservation {
    pub platform_fee_visible: Option<String>,
    pub fee_bps: Option<u32>,
    pub fee_account: Option<String>,
    pub fee_mint: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteLegObservation {
    pub venue_or_label: String,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    pub in_amount: Option<String>,
    pub out_amount: Option<String>,
    pub market_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteObservation {
    pub provider_route_label: Option<String>,
    pub legs: Vec<RouteLegObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransactionConstruction {
    pub present: bool,
    pub encoding: Option<String>,
    pub transaction_type: Option<String>,
    pub fee_payer: Option<String>,
    pub num_instructions: Option<usize>,
    pub num_lookup_tables: Option<usize>,
    pub program_ids: Vec<String>,
    pub program_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionObservation {
    pub invoked_programs: Vec<String>,
    pub loaded_account_count: Option<usize>,
    pub compute_budget_present: bool,
    pub unknown_program_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeliveryObservation {
    pub for_jito_bundle_flag: Option<bool>,
    pub jito_tip_instruction_indexes: Vec<usize>,
    pub priority_fee_observed: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SettlementObservation {
    pub applicable: bool,
    pub signature: Option<String>,
    pub status: Option<String>,
    pub slot: Option<u64>,
    pub block_time: Option<i64>,
    pub compute_units_consumed: Option<u64>,
    pub runtime_program_set: Vec<String>,
    pub notes: Vec<String>,
}

/// A relationship observed *between* two stages.
///
/// Links are the cross-layer part of the lineage: "the mint the caller asked
/// for is the mint the quote priced", "this quoted value appears as bytes in
/// this instruction". Every link carries its evidence level and an explicit
/// claim ceiling, because a numeric coincidence is not a semantic fact — a
/// byte match says the value is present, never that the program reads it as
/// that quantity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageLink {
    pub id: String,
    pub from_stage: Stage,
    pub to_stage: Stage,
    /// `same_value`, `value_mismatch`, `candidate_byte_match`, `derived_from`,
    /// `not_recoverable`.
    pub relationship: String,
    pub subject: String,
    pub object: String,
    pub evidence_level: EvidenceLevel,
    /// The strongest thing this link is allowed to support.
    pub claim_ceiling: String,
    pub explanation: String,
    /// Provenance references: field paths, instruction indexes, artifact ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

impl LineageLink {
    // A link is nine facts, and naming each one at the call site is what keeps
    // the claim ceiling from being forgotten. A builder would hide that.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        from_stage: Stage,
        to_stage: Stage,
        relationship: impl Into<String>,
        subject: impl Into<String>,
        object: impl Into<String>,
        evidence_level: EvidenceLevel,
        claim_ceiling: impl Into<String>,
        explanation: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            from_stage,
            to_stage,
            relationship: relationship.into(),
            subject: subject.into(),
            object: object.into(),
            evidence_level,
            claim_ceiling: claim_ceiling.into(),
            explanation: explanation.into(),
            evidence: Vec::new(),
        }
    }

    pub fn with_evidence(mut self, evidence: impl IntoIterator<Item = String>) -> Self {
        self.evidence.extend(evidence);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageBundle {
    pub schema_version: String,
    pub capture: CaptureMetadata,
    pub quote: QuoteObservation,
    pub fee: FeeObservation,
    pub route: RouteObservation,
    pub transaction_construction: TransactionConstruction,
    pub execution: ExecutionObservation,
    pub delivery: DeliveryObservation,
    pub settlement: SettlementObservation,
    pub claims: Vec<AttributionClaim>,
    /// Cross-stage relationships. Defaulted so bundles written before links
    /// existed still deserialize.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LineageLink>,
    pub unresolved: Vec<UnresolvedField>,
    /// Provider-specific leftovers, namespaced, never silently dropped.
    pub raw_extensions: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_transaction: Option<DecodedTransaction>,
}

impl LineageBundle {
    pub fn new(capture: CaptureMetadata) -> Self {
        Self {
            schema_version: LINEAGE_SCHEMA_VERSION.to_string(),
            capture,
            quote: QuoteObservation::default(),
            fee: FeeObservation::default(),
            route: RouteObservation::default(),
            transaction_construction: TransactionConstruction::default(),
            execution: ExecutionObservation::default(),
            delivery: DeliveryObservation::default(),
            settlement: SettlementObservation {
                applicable: false,
                ..Default::default()
            },
            claims: Vec::new(),
            links: Vec::new(),
            unresolved: Vec::new(),
            raw_extensions: BTreeMap::new(),
            decoded_transaction: None,
        }
    }

    pub fn push_claim(&mut self, claim: AttributionClaim) {
        self.claims.push(claim);
    }

    pub fn push_link(&mut self, link: LineageLink) {
        self.links.push(link);
    }

    /// All links touching a stage, in insertion order.
    pub fn links_for_stage(&self, stage: Stage) -> Vec<&LineageLink> {
        self.links
            .iter()
            .filter(|l| l.from_stage == stage || l.to_stage == stage)
            .collect()
    }

    pub fn push_unresolved(&mut self, field: impl Into<String>, reason: impl Into<String>) {
        self.unresolved.push(UnresolvedField {
            field: field.into(),
            reason: reason.into(),
            source_artifact_id: Some(self.capture.artifact_id.clone()),
        });
    }

    pub fn validate_schema(&self) -> anyhow::Result<()> {
        if self.schema_version != LINEAGE_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported lineage schema_version '{}'; supported '{}'",
                self.schema_version,
                LINEAGE_SCHEMA_VERSION
            );
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> anyhow::Result<String> {
        self.validate_schema()?;
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Unsigned artifacts must never emit settlement claims.
    pub fn assert_unsigned_has_no_settlement_claims(&self) -> anyhow::Result<()> {
        if !self.settlement.applicable {
            for c in &self.claims {
                if c.predicate.contains("settled") || c.subject == "settlement" {
                    anyhow::bail!(
                        "unsigned artifact emitted settlement claim: {} {} {}",
                        c.subject,
                        c.predicate,
                        c.object
                    );
                }
            }
        }
        Ok(())
    }
}

pub fn classify_bucket(level: EvidenceLevel) -> &'static str {
    match level {
        EvidenceLevel::DirectObservation
        | EvidenceLevel::DecodedFromTransaction
        | EvidenceLevel::ResolvedFromRpc
        | EvidenceLevel::ExternalProgramLabel => "recovered",
        EvidenceLevel::CrossArtifactInference | EvidenceLevel::Candidate => "candidate",
        EvidenceLevel::Unresolved => "unresolved",
    }
}
