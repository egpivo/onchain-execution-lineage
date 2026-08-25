//! DFlow-specific checks.
//!
//! Only DFlow API semantics this repository has actually verified against
//! recorded artifacts are encoded here. The threshold identity below was
//! confirmed on every request of the recorded route-stable bracket run
//! (`artifacts/experiments/dflow_order_slippage_route_stable_live/`) and is
//! reproduced independently by the publication extract in
//! [`crate::evidence_extract`]; the two do not share an implementation on
//! purpose, so a change in either one shows up as a disagreement.

use super::{CheckResult, CheckStatus, ExecutionCheck};
use crate::adapters::ProviderResponse;
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::LineageBundle;

pub fn checks() -> Vec<Box<dyn ExecutionCheck>> {
    vec![
        Box::new(SlippageThresholdArithmetic),
        Box::new(MinOutMatchesThreshold),
        Box::new(PlatformFeeAccounting),
    ]
}

/// `M = ceil(Q * (10000 - S) / 10000)` in exact integer arithmetic on base
/// units. Kept in `u128` because base-unit amounts routinely exceed the range
/// where f64 is exact.
///
/// Public so [`crate::reference_case`] can re-derive published thresholds with
/// the verifier's implementation rather than the publication extract's own.
pub fn ceil_threshold(out_amount: u128, slippage_bps: u32) -> Option<u128> {
    let factor = 10_000u128.checked_sub(u128::from(slippage_bps))?;
    Some(out_amount.checked_mul(factor)?.div_ceil(10_000))
}

pub fn floor_threshold(out_amount: u128, slippage_bps: u32) -> Option<u128> {
    let factor = 10_000u128.checked_sub(u128::from(slippage_bps))?;
    Some(out_amount.checked_mul(factor)? / 10_000)
}

fn response(ctx: &ExecutionContext) -> Option<&ProviderResponse> {
    ctx.provider_response.as_ref()
}

pub struct SlippageThresholdArithmetic;

impl ExecutionCheck for SlippageThresholdArithmetic {
    fn id(&self) -> &'static str {
        "dflow.slippage_threshold_arithmetic"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::ProviderResponse];
        let ceiling = "response-level arithmetic only: the response is internally consistent. \
                       It says nothing about what the transaction enforces on chain.";

        let na = |explanation: &str| {
            CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages.clone(),
                ctx.provider,
                explanation.to_string(),
                ceiling,
            )
        };

        let Some(r) = response(ctx) else {
            return na("no provider response in this context");
        };
        let (Some(out), Some(threshold), Some(bps)) = (
            &r.out_amount,
            r.other_amount_threshold
                .as_ref()
                .or(r.min_out_amount.as_ref()),
            r.slippage_bps,
        ) else {
            return na("response lacks out amount, threshold or slippage; identity not testable");
        };
        let (Ok(q), Ok(m)) = (out.parse::<u128>(), threshold.parse::<u128>()) else {
            return na("out amount or threshold is not an integer in base units");
        };
        if bps > 10_000 {
            return na("slippage exceeds 10000 bps; the identity is undefined there");
        }

        let predicted = ceil_threshold(q, bps);
        let floor = floor_threshold(q, bps);
        let evidence = vec![
            format!("out_amount={q}"),
            format!("slippage_bps={bps}"),
            format!("other_amount_threshold={m}"),
            format!(
                "predicted_ceil={}",
                predicted.map(|v| v.to_string()).unwrap_or_default()
            ),
        ];

        match predicted {
            Some(p) if p == m => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "threshold equals ceil(out_amount * (10000 - slippage_bps) / 10000)",
                ceiling,
            )
            .with_observed(m.to_string())
            .with_expected(p.to_string())
            .with_evidence(evidence),
            Some(p) if floor == Some(m) => CheckResult::new(
                self.id(),
                CheckStatus::Candidate,
                stages,
                ctx.provider,
                "threshold matches the floor variant, not the ceiling variant; one sample \
                 cannot distinguish a rounding convention from a coincidence",
                ceiling,
            )
            .with_observed(m.to_string())
            .with_expected(p.to_string())
            .with_evidence(evidence),
            Some(p) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "threshold does not follow the documented slippage identity under either \
                 rounding convention",
                ceiling,
            )
            .with_observed(m.to_string())
            .with_expected(p.to_string())
            .with_evidence(evidence),
            None => na("threshold arithmetic overflowed u128"),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct MinOutMatchesThreshold;

impl ExecutionCheck for MinOutMatchesThreshold {
    fn id(&self) -> &'static str {
        "dflow.min_out_matches_threshold"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::ProviderResponse];
        let ceiling = "two fields of one response agree; neither is verified against the \
                       transaction";

        let Some(r) = response(ctx) else {
            return CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "no provider response in this context",
                ceiling,
            );
        };

        match (&r.min_out_amount, &r.other_amount_threshold) {
            (Some(a), Some(b)) if a == b => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "minOutAmount and otherAmountThreshold carry the same value",
                ceiling,
            )
            .with_observed(a.clone())
            .with_expected(b.clone()),
            (Some(a), Some(b)) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "minOutAmount and otherAmountThreshold disagree; the response describes two \
                 different minimums",
                ceiling,
            )
            .with_observed(a.clone())
            .with_expected(b.clone()),
            _ => CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "this surface does not return both fields",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct PlatformFeeAccounting;

impl ExecutionCheck for PlatformFeeAccounting {
    fn id(&self) -> &'static str {
        "dflow.platform_fee_accounting"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::ProviderResponse];
        let ceiling = "the fee the response declares; realized fees need settlement evidence";

        let Some(fee) = response(ctx).and_then(|r| r.platform_fee.as_ref()) else {
            return CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "response carries no platformFee field",
                ceiling,
            )
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        };

        if !fee.present {
            return CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "platformFee is null: the provider declared no platform fee for this request",
                ceiling,
            )
            .with_observed(fee.visible.clone().unwrap_or_else(|| "null".into()))
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }

        let bps = fee.fee_bps;
        let amount = fee.amount.as_ref().and_then(|a| a.parse::<u128>().ok());
        let out = response(ctx)
            .and_then(|r| r.out_amount.as_ref())
            .and_then(|a| a.parse::<u128>().ok());

        let result = match (bps, amount, out) {
            // A zero-bps fee must be a zero amount; anything else contradicts
            // the response's own declaration.
            (Some(0), Some(0), _) => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages.clone(),
                ctx.provider,
                "fee is declared as 0 bps and 0 base units",
                ceiling,
            )
            .with_observed("0")
            .with_expected("0"),
            (Some(0), Some(a), _) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages.clone(),
                ctx.provider,
                "fee declares 0 bps but a non-zero amount",
                ceiling,
            )
            .with_observed(a.to_string())
            .with_expected("0"),
            (Some(b), Some(a), Some(q)) => {
                // Fee mode decides the base the bps applies to, and only the
                // outputMint mode is confirmed against a recorded artifact.
                if fee.mode.as_deref() == Some("outputMint") {
                    let expected = q * u128::from(b) / 10_000;
                    if expected == a {
                        CheckResult::new(
                            self.id(),
                            CheckStatus::Pass,
                            stages.clone(),
                            ctx.provider,
                            "fee amount equals out_amount * feeBps / 10000",
                            ceiling,
                        )
                        .with_observed(a.to_string())
                        .with_expected(expected.to_string())
                    } else {
                        CheckResult::new(
                            self.id(),
                            CheckStatus::Candidate,
                            stages.clone(),
                            ctx.provider,
                            "fee amount is off the simple bps-of-output computation; the base \
                             or rounding convention is not pinned down by one artifact",
                            ceiling,
                        )
                        .with_observed(a.to_string())
                        .with_expected(expected.to_string())
                    }
                } else {
                    CheckResult::new(
                        self.id(),
                        CheckStatus::Unknown,
                        stages.clone(),
                        ctx.provider,
                        "fee mode is not one this repository has verified against an artifact",
                        ceiling,
                    )
                    .with_observed(fee.mode.clone().unwrap_or_else(|| "absent".into()))
                }
            }
            _ => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages.clone(),
                ctx.provider,
                "platformFee present but lacks the fields needed to check it",
                ceiling,
            )
            .with_observed(fee.visible.clone().unwrap_or_default()),
        };

        result
            .with_evidence([format!(
                "platform_fee.mode={}",
                fee.mode.clone().unwrap_or_else(|| "absent".into())
            )])
            .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceil_identity_matches_the_recorded_reference_values() {
        // From artifacts/experiments/dflow_order_slippage_route_stable_live/raw/b00_A1_50.json
        assert_eq!(ceil_threshold(1_373_827_780, 50), Some(1_366_958_642));
    }

    #[test]
    fn ceil_and_floor_differ_where_rounding_matters() {
        assert_eq!(ceil_threshold(3, 5000), Some(2));
        assert_eq!(floor_threshold(3, 5000), Some(1));
    }

    #[test]
    fn zero_slippage_leaves_the_quote_untouched() {
        assert_eq!(ceil_threshold(1_000, 0), Some(1_000));
    }
}
