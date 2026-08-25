//! Settlement checks.
//!
//! Every check here returns NOT_APPLICABLE without settlement evidence. No
//! settlement input means no settlement claim — a signature on its own is a
//! pointer, not an observation, and does not unlock any of these.

use super::{CheckResult, CheckStatus, ExecutionCheck};
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::{LineageBundle, SettlementObservation};

pub fn checks() -> Vec<Box<dyn ExecutionCheck>> {
    vec![
        Box::new(LandedStatus),
        Box::new(RuntimeProgramInvocation),
        Box::new(RealizedOutputVersusThreshold),
        Box::new(FeesAndComputeUnits),
    ]
}

fn settlement(ctx: &ExecutionContext) -> Option<&SettlementObservation> {
    if !ctx.has_settlement_evidence() {
        return None;
    }
    ctx.settlement.as_ref()
}

fn not_applicable(id: &'static str, ctx: &ExecutionContext) -> CheckResult {
    let reason = match &ctx.settlement {
        Some(s) if s.signature.is_some() && !s.applicable => {
            "a signature was supplied but no settlement observation was fetched"
        }
        Some(_) => "settlement stage present but carries no signature",
        None => "artifact is unsigned or unsubmitted; no settlement evidence exists",
    };
    CheckResult::new(
        id,
        CheckStatus::NotApplicable,
        vec![Stage::Settlement],
        ctx.provider,
        reason,
        "no settlement input means no settlement claim",
    )
    .with_provenance([ctx.provenance.artifact_id.clone()])
}

pub struct LandedStatus;

impl ExecutionCheck for LandedStatus {
    fn id(&self) -> &'static str {
        "settlement.landed_status"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(s) = settlement(ctx) else {
            return not_applicable(self.id(), ctx);
        };
        let ceiling = "the runtime's own status for this signature";

        match s.status.as_deref() {
            Some("success") => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                vec![Stage::Settlement],
                ctx.provider,
                "transaction landed and the runtime reported no error",
                ceiling,
            )
            .with_observed("success")
            .with_evidence(
                s.slot
                    .map(|sl| format!("slot={sl}"))
                    .into_iter()
                    .collect::<Vec<_>>(),
            ),
            Some(other) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                vec![Stage::Settlement],
                ctx.provider,
                "transaction landed with a runtime error",
                ceiling,
            )
            .with_observed(other.to_string())
            .with_expected("success"),
            None => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                vec![Stage::Settlement],
                ctx.provider,
                "settlement observation carries no status",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct RuntimeProgramInvocation;

impl ExecutionCheck for RuntimeProgramInvocation {
    fn id(&self) -> &'static str {
        "settlement.runtime_program_invocation"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(s) = settlement(ctx) else {
            return not_applicable(self.id(), ctx);
        };
        let stages = vec![Stage::TransactionConstruction, Stage::Settlement];
        let ceiling = "programs the runtime logged; a constructed program set matching a \
                       runtime set does not prove these bytes are what landed";

        if s.runtime_program_set.is_empty() {
            return CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "no runtime program set was recovered from the settlement logs",
                ceiling,
            )
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }

        let Some(t) = &ctx.transaction else {
            return CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "runtime programs observed but there is no constructed transaction to compare",
                ceiling,
            )
            .with_observed(format!(
                "{} runtime program(s)",
                s.runtime_program_set.len()
            ))
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        };

        let missing: Vec<String> = t
            .topology
            .program_ids
            .iter()
            .filter(|p| !s.runtime_program_set.contains(p))
            .cloned()
            .collect();

        if missing.is_empty() {
            CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "every program in the constructed message appears in the runtime log",
                ceiling,
            )
            .with_observed(format!("{} program(s)", t.topology.program_ids.len()))
        } else {
            CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "a program in the constructed message was never invoked at runtime",
                ceiling,
            )
            .with_observed(format!("{} not invoked", missing.len()))
            .with_evidence(missing)
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

/// Realized output against the provider's declared minimum.
///
/// Requires a token balance delta for the output mint. The settlement
/// observation in this repository does not carry balance deltas yet, so this
/// resolves to UNKNOWN rather than being quietly skipped — the gap is visible
/// in the report.
pub struct RealizedOutputVersusThreshold;

impl ExecutionCheck for RealizedOutputVersusThreshold {
    fn id(&self) -> &'static str {
        "settlement.realized_output_vs_threshold"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(_s) = settlement(ctx) else {
            return not_applicable(self.id(), ctx);
        };
        let stages = vec![Stage::ProviderResponse, Stage::Settlement];
        let ceiling = "a realized amount versus a declared minimum; it does not establish that \
                       the minimum was enforced on chain";

        let threshold = ctx.provider_response.as_ref().and_then(|r| {
            r.other_amount_threshold
                .as_ref()
                .or(r.min_out_amount.as_ref())
        });

        CheckResult::new(
            self.id(),
            CheckStatus::Unknown,
            stages,
            ctx.provider,
            "settlement enrichment does not yet recover token balance deltas, so the realized \
             output amount is unavailable",
            ceiling,
        )
        .with_expected(threshold.cloned().unwrap_or_else(|| "unknown".into()))
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct FeesAndComputeUnits;

impl ExecutionCheck for FeesAndComputeUnits {
    fn id(&self) -> &'static str {
        "settlement.fees_and_compute_units"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(s) = settlement(ctx) else {
            return not_applicable(self.id(), ctx);
        };
        let stages = vec![Stage::Settlement];
        let ceiling = "resources the runtime reported consuming; not a fee attribution to any \
                       delivery service";

        match s.compute_units_consumed {
            Some(cu) => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "compute units consumed recovered from settlement metadata",
                ceiling,
            )
            .with_observed(cu.to_string()),
            None => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "settlement metadata carries no compute-unit figure",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}
