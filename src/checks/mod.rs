//! Verification model.
//!
//! A check reads an [`ExecutionContext`] and a [`LineageBundle`] and returns a
//! [`CheckResult`]. Results are deliberately not booleans:
//!
//! - `PASS` — the stated relationship holds on observed evidence.
//! - `FAIL` — the relationship is contradicted by observed evidence.
//! - `CANDIDATE` — consistent with the claim, but the evidence cannot
//!   distinguish it from coincidence. A candidate never becomes a pass.
//! - `UNKNOWN` — the evidence needed exists in principle but was not observed.
//! - `NOT_APPLICABLE` — the check does not apply to this artifact at all.
//!
//! The distinction between `UNKNOWN` and `NOT_APPLICABLE` matters: "we did not
//! resolve the lookup tables" and "this response has no fee to account for"
//! are different statements, and collapsing them would let a missing input
//! read as a clean bill of health.

pub mod dflow;
pub mod generic;
pub mod settlement;
pub mod solana;

use serde::{Deserialize, Serialize};

use crate::adapters::ProviderId;
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::LineageBundle;

pub const VERIFICATION_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Fail,
    Candidate,
    Unknown,
    NotApplicable,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Candidate => "CANDIDATE",
            CheckStatus::Unknown => "UNKNOWN",
            CheckStatus::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_id: String,
    pub status: CheckStatus,
    pub stages: Vec<Stage>,
    pub provider: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Concrete evidence: field paths, byte offsets, instruction indexes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub explanation: String,
    /// The strongest claim this result can support, whatever its status.
    pub evidence_ceiling: String,
    /// Artifact / lineage identifiers backing the result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<String>,
}

impl CheckResult {
    pub fn new(
        check_id: &str,
        status: CheckStatus,
        stages: Vec<Stage>,
        provider: ProviderId,
        explanation: impl Into<String>,
        evidence_ceiling: impl Into<String>,
    ) -> Self {
        Self {
            check_id: check_id.to_string(),
            status,
            stages,
            provider,
            observed: None,
            expected: None,
            evidence: Vec::new(),
            explanation: explanation.into(),
            evidence_ceiling: evidence_ceiling.into(),
            provenance: Vec::new(),
        }
    }

    pub fn with_observed(mut self, v: impl Into<String>) -> Self {
        self.observed = Some(v.into());
        self
    }

    pub fn with_expected(mut self, v: impl Into<String>) -> Self {
        self.expected = Some(v.into());
        self
    }

    pub fn with_evidence(mut self, e: impl IntoIterator<Item = String>) -> Self {
        self.evidence.extend(e);
        self
    }

    pub fn with_provenance(mut self, p: impl IntoIterator<Item = String>) -> Self {
        self.provenance.extend(p);
        self
    }
}

pub trait ExecutionCheck {
    fn id(&self) -> &'static str;

    fn run(&self, ctx: &ExecutionContext, lineage: &LineageBundle) -> CheckResult;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationSummary {
    pub pass: usize,
    pub fail: usize,
    pub candidate: usize,
    pub unknown: usize,
    pub not_applicable: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub schema_version: String,
    pub provider: ProviderId,
    pub artifact_id: String,
    pub stages_present: Vec<Stage>,
    pub summary: VerificationSummary,
    pub results: Vec<CheckResult>,
}

impl VerificationReport {
    pub fn to_canonical_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// True when any check contradicts the evidence. Candidates and unknowns
    /// are not failures — and are not successes either.
    pub fn has_failures(&self) -> bool {
        self.summary.fail > 0
    }
}

/// Every check, in a fixed order: generic, provider, Solana, settlement.
///
/// Provider-specific checks are selected by the context's provider, so a
/// Jupiter artifact never runs DFlow's arithmetic.
pub fn all_checks(provider: ProviderId) -> Vec<Box<dyn ExecutionCheck>> {
    let mut checks: Vec<Box<dyn ExecutionCheck>> = generic::checks();
    if provider == ProviderId::Dflow {
        checks.extend(dflow::checks());
    }
    checks.extend(solana::checks());
    checks.extend(settlement::checks());
    checks
}

pub fn verify(ctx: &ExecutionContext, lineage: &LineageBundle) -> VerificationReport {
    let results: Vec<CheckResult> = all_checks(ctx.provider)
        .iter()
        .map(|c| c.run(ctx, lineage))
        .collect();

    let mut summary = VerificationSummary::default();
    for r in &results {
        match r.status {
            CheckStatus::Pass => summary.pass += 1,
            CheckStatus::Fail => summary.fail += 1,
            CheckStatus::Candidate => summary.candidate += 1,
            CheckStatus::Unknown => summary.unknown += 1,
            CheckStatus::NotApplicable => summary.not_applicable += 1,
        }
    }

    VerificationReport {
        schema_version: VERIFICATION_SCHEMA_VERSION.into(),
        provider: ctx.provider,
        artifact_id: ctx.provenance.artifact_id.clone(),
        stages_present: ctx.stages_present(),
        summary,
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dflow_checks_only_run_for_dflow() {
        let dflow_ids: Vec<&'static str> = all_checks(ProviderId::Dflow)
            .iter()
            .map(|c| c.id())
            .collect();
        let jupiter_ids: Vec<&'static str> = all_checks(ProviderId::Jupiter)
            .iter()
            .map(|c| c.id())
            .collect();

        assert!(dflow_ids.iter().any(|id| id.starts_with("dflow.")));
        assert!(!jupiter_ids.iter().any(|id| id.starts_with("dflow.")));
    }

    #[test]
    fn check_ids_are_unique() {
        let ids: Vec<&'static str> = all_checks(ProviderId::Dflow)
            .iter()
            .map(|c| c.id())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate check id in registry");
    }
}
