//! Provider-independent checks.
//!
//! Nothing here may read a provider-native field name or branch on
//! [`ProviderId`] — these run identically for DFlow, Jupiter and generic
//! artifacts.

use super::{CheckResult, CheckStatus, ExecutionCheck};
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::LineageBundle;

pub fn checks() -> Vec<Box<dyn ExecutionCheck>> {
    vec![
        Box::new(InputMintConsistency),
        Box::new(OutputMintConsistency),
        Box::new(InputAmountConsistency),
        Box::new(RoutePresence),
        Box::new(TransactionPresence),
        Box::new(TransactionDecode),
    ]
}

/// Compare a value the caller asked for against the value the route describes.
fn mint_consistency(
    ctx: &ExecutionContext,
    check_id: &'static str,
    label: &str,
    quoted: Option<&String>,
    leg_value: Option<&String>,
    leg_path: &str,
) -> CheckResult {
    let stages = vec![Stage::ProviderResponse, Stage::Route];
    let ceiling = "field-level agreement inside one response body; not on-chain confirmation";

    match (quoted, leg_value) {
        (None, _) => CheckResult::new(
            check_id,
            CheckStatus::Unknown,
            stages,
            ctx.provider,
            format!("response carries no {label}"),
            ceiling,
        ),
        (Some(q), None) => CheckResult::new(
            check_id,
            CheckStatus::Unknown,
            stages,
            ctx.provider,
            format!("no route leg to compare the quoted {label} against"),
            ceiling,
        )
        .with_observed(q.clone()),
        (Some(q), Some(l)) if q == l => CheckResult::new(
            check_id,
            CheckStatus::Pass,
            stages,
            ctx.provider,
            format!("quoted {label} matches {leg_path}"),
            ceiling,
        )
        .with_observed(l.clone())
        .with_expected(q.clone())
        .with_evidence([format!("response.{label}"), leg_path.to_string()]),
        (Some(q), Some(l)) => CheckResult::new(
            check_id,
            CheckStatus::Fail,
            stages,
            ctx.provider,
            format!("quoted {label} does not match {leg_path}"),
            ceiling,
        )
        .with_observed(l.clone())
        .with_expected(q.clone())
        .with_evidence([format!("response.{label}"), leg_path.to_string()]),
    }
}

pub struct InputMintConsistency;

impl ExecutionCheck for InputMintConsistency {
    fn id(&self) -> &'static str {
        "generic.input_mint_consistency"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        mint_consistency(
            ctx,
            self.id(),
            "input_mint",
            ctx.provider_response
                .as_ref()
                .and_then(|r| r.input_mint.as_ref()),
            ctx.route
                .as_ref()
                .and_then(|r| r.legs.first())
                .and_then(|l| l.input_mint.as_ref()),
            "route.legs[0].input_mint",
        )
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct OutputMintConsistency;

impl ExecutionCheck for OutputMintConsistency {
    fn id(&self) -> &'static str {
        "generic.output_mint_consistency"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        mint_consistency(
            ctx,
            self.id(),
            "output_mint",
            ctx.provider_response
                .as_ref()
                .and_then(|r| r.output_mint.as_ref()),
            ctx.route
                .as_ref()
                .and_then(|r| r.legs.last())
                .and_then(|l| l.output_mint.as_ref()),
            "route.legs[last].output_mint",
        )
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct InputAmountConsistency;

impl ExecutionCheck for InputAmountConsistency {
    fn id(&self) -> &'static str {
        "generic.input_amount_consistency"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::Intent, Stage::ProviderResponse, Stage::Route];
        let ceiling = "field-level agreement inside one response body; base-unit strings are \
                       compared as integers, never as floats";

        let requested = ctx.intent.as_ref().and_then(|i| i.in_amount.as_ref());
        let quoted = ctx
            .provider_response
            .as_ref()
            .and_then(|r| r.in_amount.as_ref());
        let first_leg = ctx
            .route
            .as_ref()
            .and_then(|r| r.legs.first())
            .and_then(|l| l.in_amount.as_ref());

        let Some(quoted) = quoted else {
            return CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "response carries no input amount",
                ceiling,
            );
        };

        let mut evidence = vec!["response.in_amount".to_string()];
        let mut mismatch: Option<String> = None;
        if let Some(r) = requested {
            evidence.push("intent.in_amount".into());
            if r != quoted {
                mismatch = Some(format!("intent.in_amount={r}"));
            }
        }
        if let Some(l) = first_leg {
            evidence.push("route.legs[0].in_amount".into());
            if l != quoted && mismatch.is_none() {
                mismatch = Some(format!("route.legs[0].in_amount={l}"));
            }
        }

        match mismatch {
            Some(m) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "input amount disagrees across stages",
                ceiling,
            )
            .with_observed(m)
            .with_expected(quoted.clone())
            .with_evidence(evidence),
            None if requested.is_none() && first_leg.is_none() => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "only one stage reports an input amount; nothing to compare it with",
                ceiling,
            )
            .with_observed(quoted.clone()),
            None => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "input amount agrees across every stage that reports it",
                ceiling,
            )
            .with_observed(quoted.clone())
            .with_evidence(evidence),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct RoutePresence;

impl ExecutionCheck for RoutePresence {
    fn id(&self) -> &'static str {
        "generic.route_presence"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let ceiling = "the provider's own account of the route; not on-chain proof of execution";
        match &ctx.route {
            Some(route) if !route.legs.is_empty() => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                vec![Stage::Route],
                ctx.provider,
                "provider described a route",
                ceiling,
            )
            .with_observed(format!("{} leg(s)", route.legs.len()))
            .with_evidence(
                route
                    .legs
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("route.legs[{i}].venue={}", l.venue_or_label)),
            ),
            Some(_) => CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                vec![Stage::Route],
                ctx.provider,
                "route object present but empty",
                ceiling,
            )
            .with_observed("0 leg(s)"),
            None => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                vec![Stage::Route],
                ctx.provider,
                "artifact carries no route observation",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct TransactionPresence;

impl ExecutionCheck for TransactionPresence {
    fn id(&self) -> &'static str {
        "generic.transaction_presence"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let ceiling = "presence of an unsigned payload; nothing about signing or submission";
        let stages = vec![Stage::TransactionConstruction];

        match ctx.transaction_ref() {
            Some(r) if r.present && r.payload.is_some() => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "provider returned an inline unsigned transaction",
                ceiling,
            )
            .with_observed(r.encoding.clone().unwrap_or_else(|| "unknown".into())),
            Some(r) if r.present => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "transaction is referenced but its bytes were not supplied",
                ceiling,
            )
            .with_observed(r.external_ref.clone().unwrap_or_else(|| "reference".into())),
            Some(_) => CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "this surface returned no transaction; absence is a property of the surface, \
                 not a failure",
                ceiling,
            ),
            None if ctx.transaction.is_some() => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "transaction bytes were supplied directly",
                ceiling,
            ),
            None => CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "no provider extraction and no transaction bytes",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct TransactionDecode;

impl ExecutionCheck for TransactionDecode {
    fn id(&self) -> &'static str {
        "generic.transaction_decode"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::TransactionConstruction];
        let ceiling = "the bytes parse as a Solana transaction; parsing is not validity";

        match (&ctx.transaction, ctx.transaction_ref()) {
            (Some(tx), _) => CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "transaction decoded",
                ceiling,
            )
            .with_observed(format!(
                "{} instruction(s), {} static key(s)",
                tx.topology.num_instructions, tx.topology.num_static_keys
            ))
            .with_evidence([format!("transaction_sha256={}", tx.transaction_sha256)]),
            (None, Some(r)) if r.present => CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "a transaction exists but was not decoded in this run",
                ceiling,
            ),
            _ => CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "no transaction to decode",
                ceiling,
            ),
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}
