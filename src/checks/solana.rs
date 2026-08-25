//! Solana transaction-mechanics checks. Provider-independent.

use super::{CheckResult, CheckStatus, ExecutionCheck};
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::LineageBundle;
use crate::solana::TransactionObservation;

pub fn checks() -> Vec<Box<dyn ExecutionCheck>> {
    vec![
        Box::new(TransactionVersionCheck),
        Box::new(AltResolutionCheck),
        Box::new(AccountIndexValidityCheck),
        Box::new(ProgramPresenceCheck),
        Box::new(TransactionTopologyCheck),
        Box::new(CandidateByteSearchCheck),
    ]
}

fn no_transaction(id: &'static str, ctx: &ExecutionContext) -> CheckResult {
    CheckResult::new(
        id,
        CheckStatus::NotApplicable,
        vec![Stage::TransactionConstruction],
        ctx.provider,
        "no decoded transaction in this context",
        "none",
    )
    .with_provenance([ctx.provenance.artifact_id.clone()])
}

fn tx(ctx: &ExecutionContext) -> Option<&TransactionObservation> {
    ctx.transaction.as_ref()
}

pub struct TransactionVersionCheck;

impl ExecutionCheck for TransactionVersionCheck {
    fn id(&self) -> &'static str {
        "solana.transaction_version"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(t) = tx(ctx) else {
            return no_transaction(self.id(), ctx);
        };
        CheckResult::new(
            self.id(),
            CheckStatus::Pass,
            vec![Stage::TransactionConstruction],
            ctx.provider,
            "message version read from the encoded message",
            "the encoded version; not a statement about runtime support",
        )
        .with_observed(t.version.as_str())
        .with_evidence([format!("decoder_alt_label={}", t.decoded.transaction_type)])
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct AltResolutionCheck;

impl ExecutionCheck for AltResolutionCheck {
    fn id(&self) -> &'static str {
        "solana.alt_resolution"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(t) = tx(ctx) else {
            return no_transaction(self.id(), ctx);
        };
        let alt = &t.alt_resolution;
        let stages = vec![Stage::TransactionConstruction];
        let ceiling = "table membership is not transaction relevance: only indexed entries are \
                       loaded";

        if alt.tables_referenced.is_empty() {
            return CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "transaction references no address lookup table",
                ceiling,
            )
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }
        if !alt.attempted {
            return CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "lookup tables referenced but resolution was not attempted (offline extraction)",
                ceiling,
            )
            .with_observed(format!(
                "{} table(s) referenced",
                alt.tables_referenced.len()
            ))
            .with_evidence(alt.tables_referenced.clone())
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }

        if alt.complete {
            CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "every referenced lookup table resolved",
                ceiling,
            )
            .with_observed(format!(
                "{}/{}",
                alt.tables_resolved.len(),
                alt.tables_referenced.len()
            ))
            .with_evidence(alt.tables_resolved.clone())
        } else {
            CheckResult::new(
                self.id(),
                CheckStatus::Unknown,
                stages,
                ctx.provider,
                "at least one lookup table could not be resolved; the account vector is \
                 incomplete and absence claims about accounts are not available",
                ceiling,
            )
            .with_observed(format!(
                "{}/{}",
                alt.tables_resolved.len(),
                alt.tables_referenced.len()
            ))
            .with_evidence(
                alt.tables_unresolved
                    .iter()
                    .map(|(t, e)| format!("{t}: {e}")),
            )
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct AccountIndexValidityCheck;

impl ExecutionCheck for AccountIndexValidityCheck {
    fn id(&self) -> &'static str {
        "solana.account_index_validity"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(t) = tx(ctx) else {
            return no_transaction(self.id(), ctx);
        };
        let v = &t.account_index_validity;
        let stages = vec![Stage::TransactionConstruction];
        let ceiling = "structural validity of the compiled message; not a claim that the \
                       accounts are the right ones";

        if v.all_indexes_in_range {
            CheckResult::new(
                self.id(),
                CheckStatus::Pass,
                stages,
                ctx.provider,
                "every account index resolves inside the loaded account vector",
                ceiling,
            )
            .with_observed(format!(
                "max index {} < vector length {}",
                v.max_index_referenced
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "-".into()),
                v.account_vector_len
            ))
        } else {
            CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "an instruction names an account index outside the loaded account vector",
                ceiling,
            )
            .with_observed(format!(
                "{} out-of-range index/indexes",
                v.out_of_range.len()
            ))
            .with_expected(format!("< {}", v.account_vector_len))
            .with_evidence(v.out_of_range.iter().map(|o| {
                format!(
                    "instruction[{}].{}={}",
                    o.instruction_index, o.position, o.index
                )
            }))
        }
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct ProgramPresenceCheck;

impl ExecutionCheck for ProgramPresenceCheck {
    fn id(&self) -> &'static str {
        "solana.program_presence"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(t) = tx(ctx) else {
            return no_transaction(self.id(), ctx);
        };
        let stages = vec![Stage::TransactionConstruction];
        let ceiling = "a program ID appearing in the message; invocation is only observable \
                       after settlement";

        let known: Vec<&String> = t
            .topology
            .program_ids
            .iter()
            .filter(|p| !t.topology.unknown_program_ids.contains(p))
            .collect();

        if t.topology.program_ids.is_empty() {
            return CheckResult::new(
                self.id(),
                CheckStatus::Fail,
                stages,
                ctx.provider,
                "transaction contains no instructions and therefore no programs",
                ceiling,
            )
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }

        let status = if t.topology.unknown_program_ids.is_empty() {
            CheckStatus::Pass
        } else {
            // An unlabelled program is not a defect; it is a gap in the local
            // registry, and the result says which one.
            CheckStatus::Candidate
        };

        CheckResult::new(
            self.id(),
            status,
            stages,
            ctx.provider,
            if status == CheckStatus::Pass {
                "every program in the message is in the verified registry"
            } else {
                "some programs are not in the verified registry; they are named, not labelled"
            },
            ceiling,
        )
        .with_observed(format!(
            "{} program(s), {} unlabelled",
            t.topology.program_ids.len(),
            t.topology.unknown_program_ids.len()
        ))
        .with_evidence(
            known.into_iter().cloned().chain(
                t.topology
                    .unknown_program_ids
                    .iter()
                    .map(|p| format!("unlabelled={p}")),
            ),
        )
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

pub struct TransactionTopologyCheck;

impl ExecutionCheck for TransactionTopologyCheck {
    fn id(&self) -> &'static str {
        "solana.transaction_topology"
    }

    fn run(&self, ctx: &ExecutionContext, _lineage: &LineageBundle) -> CheckResult {
        let Some(t) = tx(ctx) else {
            return no_transaction(self.id(), ctx);
        };
        let stages = vec![Stage::TransactionConstruction];
        let ceiling = "shape of the constructed message only";

        let status = if t.topology.num_instructions == 0 {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        };

        CheckResult::new(
            self.id(),
            status,
            stages,
            ctx.provider,
            "instruction / account / lookup-table counts recovered from the message",
            ceiling,
        )
        .with_observed(format!(
            "{} instruction(s), {} static key(s), {} table(s), {} ALT-loaded account(s), \
             vector length {}",
            t.topology.num_instructions,
            t.topology.num_static_keys,
            t.topology.num_lookup_tables,
            t.topology.num_alt_loaded_accounts,
            t.topology.account_vector_len,
        ))
        .with_evidence(
            t.topology
                .instruction_data_lens
                .iter()
                .enumerate()
                .map(|(i, l)| format!("instruction[{i}].data_len={l}")),
        )
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}

/// Reports the byte-level relationships the lineage builder recorded.
///
/// This check exists to make the ceiling explicit and machine-readable: a hit
/// is CANDIDATE and there is no code path that promotes it to PASS.
pub struct CandidateByteSearchCheck;

impl ExecutionCheck for CandidateByteSearchCheck {
    fn id(&self) -> &'static str {
        "solana.candidate_byte_search"
    }

    fn run(&self, ctx: &ExecutionContext, lineage: &LineageBundle) -> CheckResult {
        let stages = vec![Stage::ProviderResponse, Stage::TransactionConstruction];
        let ceiling = "byte presence only. A match shows the integer's encoding occurs in a \
                       payload; it does not show the program reads it as that quantity. \
                       A non-match is not evidence of absence.";

        if tx(ctx).is_none() {
            return no_transaction(self.id(), ctx);
        }

        let byte_links: Vec<_> = lineage
            .links
            .iter()
            .filter(|l| l.id.starts_with("response_to_transaction_bytes:"))
            .collect();

        if byte_links.is_empty() {
            return CheckResult::new(
                self.id(),
                CheckStatus::NotApplicable,
                stages,
                ctx.provider,
                "no response values were available to search for",
                ceiling,
            )
            .with_provenance([ctx.provenance.artifact_id.clone()]);
        }

        let hits: Vec<_> = byte_links
            .iter()
            .filter(|l| l.relationship == "candidate_byte_match")
            .collect();

        let status = if hits.is_empty() {
            CheckStatus::Unknown
        } else {
            CheckStatus::Candidate
        };

        CheckResult::new(
            self.id(),
            status,
            stages,
            ctx.provider,
            if hits.is_empty() {
                "no searched response value appears verbatim in any instruction payload"
            } else {
                "at least one response value appears verbatim in instruction bytes"
            },
            ceiling,
        )
        .with_observed(format!(
            "{}/{} value(s) matched",
            hits.len(),
            byte_links.len()
        ))
        .with_evidence(byte_links.iter().map(|l| {
            format!(
                "{}={} [{}]",
                l.id.trim_start_matches("response_to_transaction_bytes:"),
                l.relationship,
                l.evidence.join(",")
            )
        }))
        .with_provenance([ctx.provenance.artifact_id.clone()])
    }
}
