//! Builds a [`LineageBundle`] from an [`ExecutionContext`], and links
//! observations across stages.
//!
//! The builder's job is the *joins*: which mint the caller asked for versus
//! which mint the quote priced versus which token accounts the transaction
//! touches. Every join it records carries an evidence level and a claim
//! ceiling. A numeric match between a quoted value and bytes in an instruction
//! payload is recorded as a candidate, and there is no path in this module that
//! turns a candidate into a semantic fact.

use anyhow::Result;

use crate::adapters::ProviderId;
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::execution_context::{ExecutionContext, Stage};
use crate::lineage_model::{CaptureMetadata, LineageBundle, LineageLink};
use crate::program_registry::known_programs;
use crate::transaction::DecodedTransaction;
use crate::tx_compare::{amount_needles, find_all_subslices};

/// Quantities worth looking for in instruction bytes, in a fixed order.
const SEARCHED_RESPONSE_VALUES: [&str; 3] = ["out_amount", "other_amount_threshold", "in_amount"];

pub struct LineageBuilder<'a> {
    ctx: &'a ExecutionContext,
}

impl<'a> LineageBuilder<'a> {
    pub fn new(ctx: &'a ExecutionContext) -> Self {
        Self { ctx }
    }

    pub fn build(&self) -> Result<LineageBundle> {
        let ctx = self.ctx;
        let mut bundle = LineageBundle::new(CaptureMetadata {
            artifact_id: ctx.provenance.artifact_id.clone(),
            provider: ctx.provider.as_str().to_string(),
            surface: ctx.provenance.surface.clone().unwrap_or_default(),
            captured_at_utc: ctx.provenance.captured_at_utc.clone().unwrap_or_default(),
            pair: ctx.provenance.pair.clone().unwrap_or_default(),
        });

        self.apply_response(&mut bundle);
        self.apply_route(&mut bundle);
        self.apply_transaction(&mut bundle);
        self.apply_settlement(&mut bundle);

        self.link_intent_to_response(&mut bundle);
        self.link_response_to_route(&mut bundle);
        self.link_route_to_transaction(&mut bundle);
        self.link_response_values_to_transaction_bytes(&mut bundle)?;
        self.link_transaction_to_settlement(&mut bundle);

        if let Some(extraction) = &ctx.provider_extraction {
            bundle
                .raw_extensions
                .insert("_adapter".into(), serde_json::json!(ctx.provider.as_str()));
            for u in &extraction.unsupported {
                bundle.push_unresolved(u.field.clone(), u.reason.clone());
            }
            for (k, v) in &extraction.extensions {
                bundle
                    .raw_extensions
                    .insert(format!("{}:{k}", ctx.provider), v.clone());
            }
        }

        if !bundle.settlement.applicable {
            bundle.push_unresolved(
                "settlement",
                "not applicable — no settlement observation was supplied",
            );
            bundle.assert_unsigned_has_no_settlement_claims()?;
        }

        // Interface-level items that stay unresolved unless separately evidenced.
        bundle.push_unresolved(
            "private_route_selection_policy",
            "router internal policy is not observable from a response body or transaction bytes",
        );
        bundle.push_unresolved(
            "delivery_channel",
            "Jito tip match or forJitoBundle flag is infrastructure evidence, not confirmed delivery path",
        );

        Ok(bundle)
    }

    fn apply_response(&self, bundle: &mut LineageBundle) {
        let Some(response) = &self.ctx.provider_response else {
            return;
        };
        bundle.quote.input_mint = response.input_mint.clone();
        bundle.quote.output_mint = response.output_mint.clone();
        bundle.quote.in_amount = response.in_amount.clone();
        bundle.quote.out_amount = response.out_amount.clone();
        // The legacy bundle has one slot; the context keeps both apart and the
        // DFlow check reads them from the context, not from here.
        bundle.quote.min_out_amount = response
            .min_out_amount
            .clone()
            .or_else(|| response.other_amount_threshold.clone());
        bundle.quote.request_or_quote_id = response.request_or_quote_id.clone();
        bundle.quote.error = response.error.clone();

        if let Some(fee) = &response.platform_fee {
            bundle.fee.platform_fee_visible = fee.visible.clone();
            bundle.fee.fee_bps = fee.fee_bps;
            bundle.fee.fee_account = fee.fee_account.clone();
            bundle.fee.fee_mint = fee.fee_mint.clone();
            bundle.fee.mode = fee.mode.clone();
        }

        if let Some(id) = &response.request_or_quote_id {
            bundle.push_claim(
                AttributionClaim::new(
                    "provider_response",
                    "has_request_id",
                    id.clone(),
                    EvidenceLevel::DirectObservation,
                    &bundle.capture.artifact_id,
                    "request/quote id present in the provider response",
                )
                .with_field("request_or_quote_id"),
            );
        }
    }

    fn apply_route(&self, bundle: &mut LineageBundle) {
        if let Some(route) = &self.ctx.route {
            bundle.route = route.clone();
        }
    }

    fn apply_transaction(&self, bundle: &mut LineageBundle) {
        let Some(tx) = &self.ctx.transaction else {
            if let Some(r) = self.ctx.transaction_ref() {
                bundle.transaction_construction.present = r.present;
                bundle.transaction_construction.encoding = r.encoding.clone();
            }
            return;
        };

        // Reuse the existing claim/observation mapping so the `trace` output
        // and the new path describe a transaction the same way.
        apply_decoded_transaction(bundle, &tx.decoded);

        bundle.execution.loaded_account_count = tx
            .account_map
            .as_ref()
            .map(|m| m.total_account_vector_len)
            .or(Some(tx.topology.account_vector_len));
        bundle.raw_extensions.insert(
            "solana_extraction".into(),
            serde_json::json!({
                "version": tx.version.as_str(),
                "transaction_sha256": tx.transaction_sha256,
                "account_vector_len": tx.topology.account_vector_len,
                "alt_tables_referenced": tx.alt_resolution.tables_referenced.len(),
                "alt_resolution_attempted": tx.alt_resolution.attempted,
                "alt_resolution_complete": tx.alt_resolution.complete,
                "account_indexes_in_range": tx.account_index_validity.all_indexes_in_range,
            }),
        );

        if let Some(map) = &tx.account_map {
            bundle.raw_extensions.insert(
                "loaded_address_summary".into(),
                serde_json::json!({
                    "total_static_keys": map.total_static_keys,
                    "total_loaded_from_alts": map.total_loaded_from_alts,
                    "total_addresses_in_referenced_tables": map.total_addresses_in_referenced_tables,
                }),
            );

            // Owner-derived integrator markers, only meaningful once account
            // facts were fetched. The owner of an account can carry attribution
            // the address itself does not — but the origin stays unconfirmed,
            // so these are candidates.
            for (addr, owner) in
                crate::instruction_map::owner_derived_markers(map, "candidate_integrator_program")
            {
                bundle.push_claim(AttributionClaim::new(
                    addr,
                    "owned_by_candidate_integrator",
                    owner,
                    EvidenceLevel::Candidate,
                    &bundle.capture.artifact_id,
                    "account owner matches a candidate integrator program label; origin unconfirmed",
                ));
            }
        }

        for (table, err) in &tx.alt_resolution.tables_unresolved {
            bundle.push_unresolved(format!("alt:{table}"), err.clone());
        }
    }

    fn apply_settlement(&self, bundle: &mut LineageBundle) {
        if let Some(settlement) = &self.ctx.settlement {
            bundle.settlement = settlement.clone();
        }
    }

    fn link_intent_to_response(&self, bundle: &mut LineageBundle) {
        let (Some(intent), Some(response)) = (&self.ctx.intent, &self.ctx.provider_response) else {
            return;
        };

        let pairs: [(&str, &Option<String>, &Option<String>); 3] = [
            ("input_mint", &intent.input_mint, &response.input_mint),
            ("output_mint", &intent.output_mint, &response.output_mint),
            ("in_amount", &intent.in_amount, &response.in_amount),
        ];

        // When the intent was recovered from the response itself, agreement is
        // tautological and is recorded as such rather than as corroboration.
        let echoed = intent.recovered_from == "provider_response_echo";

        for (field, requested, quoted) in pairs {
            let (Some(requested), Some(quoted)) = (requested, quoted) else {
                continue;
            };
            let same = requested == quoted;
            bundle.push_link(
                LineageLink::new(
                    format!("intent_to_response:{field}"),
                    Stage::Intent,
                    Stage::ProviderResponse,
                    if same { "same_value" } else { "value_mismatch" },
                    format!("intent.{field}={requested}"),
                    format!("response.{field}={quoted}"),
                    if echoed {
                        EvidenceLevel::DirectObservation
                    } else {
                        EvidenceLevel::CrossArtifactInference
                    },
                    if echoed {
                        "provider's own echo of the request; not independent corroboration"
                    } else {
                        "two artifacts agree on this field"
                    },
                    format!(
                        "requested {field} {} the {field} the provider priced",
                        if same { "equals" } else { "differs from" }
                    ),
                )
                .with_evidence([format!("intent.recovered_from={}", intent.recovered_from)]),
            );
        }
    }

    fn link_response_to_route(&self, bundle: &mut LineageBundle) {
        let (Some(response), Some(route)) = (&self.ctx.provider_response, &self.ctx.route) else {
            return;
        };
        let (Some(first), Some(last)) = (route.legs.first(), route.legs.last()) else {
            return;
        };

        if let (Some(quoted), Some(leg)) = (&response.input_mint, &first.input_mint) {
            bundle.push_link(LineageLink::new(
                "response_to_route:input_mint",
                Stage::ProviderResponse,
                Stage::Route,
                if quoted == leg {
                    "same_value"
                } else {
                    "value_mismatch"
                },
                format!("response.input_mint={quoted}"),
                format!("route.legs[0].input_mint={leg}"),
                EvidenceLevel::DirectObservation,
                "both values come from the same response body",
                "quoted input mint against the first route leg's input mint",
            ));
        }
        if let (Some(quoted), Some(leg)) = (&response.output_mint, &last.output_mint) {
            bundle.push_link(LineageLink::new(
                "response_to_route:output_mint",
                Stage::ProviderResponse,
                Stage::Route,
                if quoted == leg {
                    "same_value"
                } else {
                    "value_mismatch"
                },
                format!("response.output_mint={quoted}"),
                format!("route.legs[last].output_mint={leg}"),
                EvidenceLevel::DirectObservation,
                "both values come from the same response body",
                "quoted output mint against the last route leg's output mint",
            ));
        }
    }

    fn link_route_to_transaction(&self, bundle: &mut LineageBundle) {
        let (Some(route), Some(tx)) = (&self.ctx.route, &self.ctx.transaction) else {
            return;
        };

        // A market key naming an account the transaction actually loads is the
        // strongest route↔transaction join available without settlement.
        let loaded: Vec<String> = match &tx.account_map {
            Some(map) => map
                .loaded_addresses
                .iter()
                .map(|a| a.address.clone())
                .collect(),
            None => tx.decoded.static_account_keys.clone(),
        };
        let complete_vector = tx.account_map.is_some();

        for (i, leg) in route.legs.iter().enumerate() {
            let Some(market_key) = &leg.market_key else {
                continue;
            };
            let found = loaded.iter().any(|a| a == market_key);
            let (relationship, level, ceiling, explanation) = if found {
                (
                    "same_value",
                    EvidenceLevel::DecodedFromTransaction,
                    "the account is loaded by this transaction; it does not prove the leg executed",
                    "route leg market key appears in the transaction's loaded accounts",
                )
            } else if complete_vector {
                (
                    "value_mismatch",
                    EvidenceLevel::DecodedFromTransaction,
                    "absence from a fully resolved account vector",
                    "route leg market key is absent from the fully resolved account vector",
                )
            } else {
                (
                    "not_recoverable",
                    EvidenceLevel::Unresolved,
                    "account vector incomplete; absence proves nothing",
                    "route leg market key not found, but lookup tables were not resolved",
                )
            };
            bundle.push_link(
                LineageLink::new(
                    format!("route_to_transaction:leg{i}_market_key"),
                    Stage::Route,
                    Stage::TransactionConstruction,
                    relationship,
                    format!("route.legs[{i}].market_key={market_key}"),
                    format!("transaction.loaded_accounts (n={})", loaded.len()),
                    level,
                    ceiling,
                    explanation,
                )
                .with_evidence([format!("route.legs[{i}].venue={}", leg.venue_or_label)]),
            );
        }
    }

    /// Quoted values against instruction bytes.
    ///
    /// A hit means the little/big-endian encoding of that integer occurs at
    /// some offset in some instruction payload. It does not mean the program
    /// reads those bytes as that quantity, and the claim ceiling says so.
    fn link_response_values_to_transaction_bytes(&self, bundle: &mut LineageBundle) -> Result<()> {
        let (Some(response), Some(tx)) = (&self.ctx.provider_response, &self.ctx.transaction)
        else {
            return Ok(());
        };
        let payloads = tx.instruction_payloads()?;

        let values: Vec<(&str, Option<&String>)> = vec![
            ("out_amount", response.out_amount.as_ref()),
            (
                "other_amount_threshold",
                response
                    .other_amount_threshold
                    .as_ref()
                    .or(response.min_out_amount.as_ref()),
            ),
            ("in_amount", response.in_amount.as_ref()),
        ];
        debug_assert_eq!(values.len(), SEARCHED_RESPONSE_VALUES.len());

        for (name, value) in values {
            let Some(value) = value else { continue };
            let Ok(parsed) = value.parse::<u128>() else {
                continue;
            };

            let mut sites: Vec<String> = Vec::new();
            let mut encodings: Vec<String> = Vec::new();
            for (_width, needle, label) in amount_needles(parsed) {
                for (index, data) in payloads.iter().enumerate() {
                    for offset in find_all_subslices(data, &needle) {
                        sites.push(format!("instruction[{index}]+{offset}"));
                        encodings.push(label.to_string());
                    }
                }
            }
            sites.sort();
            sites.dedup();
            encodings.sort();
            encodings.dedup();

            let link = if sites.is_empty() {
                LineageLink::new(
                    format!("response_to_transaction_bytes:{name}"),
                    Stage::ProviderResponse,
                    Stage::TransactionConstruction,
                    "not_recoverable",
                    format!("response.{name}={value}"),
                    "no byte match in any instruction payload".to_string(),
                    EvidenceLevel::Unresolved,
                    "non-recovery is not evidence of absence: the value may be encoded, \
                     derived at runtime, or carried in an account rather than a payload",
                    format!("searched every instruction payload for {name} under the tested encoding family; no hit"),
                )
            } else {
                LineageLink::new(
                    format!("response_to_transaction_bytes:{name}"),
                    Stage::ProviderResponse,
                    Stage::TransactionConstruction,
                    "candidate_byte_match",
                    format!("response.{name}={value}"),
                    format!("{} byte site(s)", sites.len()),
                    EvidenceLevel::Candidate,
                    "byte presence only; the program's interpretation of these bytes is unverified",
                    format!("{name} occurs verbatim in instruction bytes"),
                )
                .with_evidence(
                    sites
                        .iter()
                        .cloned()
                        .chain(encodings.iter().map(|e| format!("encoding={e}"))),
                )
            };
            bundle.push_link(link);
        }
        Ok(())
    }

    fn link_transaction_to_settlement(&self, bundle: &mut LineageBundle) {
        let (Some(tx), Some(settlement)) = (&self.ctx.transaction, &self.ctx.settlement) else {
            return;
        };
        if !settlement.applicable || settlement.signature.is_none() {
            return;
        }

        let constructed: std::collections::BTreeSet<&String> =
            tx.topology.program_ids.iter().collect();
        let runtime: std::collections::BTreeSet<&String> =
            settlement.runtime_program_set.iter().collect();
        let missing: Vec<String> = constructed
            .difference(&runtime)
            .map(|s| (*s).clone())
            .collect();

        bundle.push_link(
            LineageLink::new(
                "transaction_to_settlement:program_invocation",
                Stage::TransactionConstruction,
                Stage::Settlement,
                if missing.is_empty() {
                    "same_value"
                } else {
                    "value_mismatch"
                },
                format!("constructed programs (n={})", constructed.len()),
                format!("runtime programs (n={})", runtime.len()),
                EvidenceLevel::ResolvedFromRpc,
                "the settled transaction may not be the one that was constructed \
                 unless the signature was derived from these bytes",
                "constructed program set against the runtime program set from settlement logs",
            )
            .with_evidence(missing.into_iter().map(|p| format!("not_invoked={p}"))),
        );
    }
}

/// Convenience: context → bundle.
pub fn build_lineage(ctx: &ExecutionContext) -> Result<LineageBundle> {
    LineageBuilder::new(ctx).build()
}

/// Map a decoded transaction onto a bundle's construction/execution stages.
///
/// Moved here from `trace` when trace stopped constructing bundles. Still
/// public — and re-exported from [`crate::trace`] — because it is the one
/// place that turns decoded instructions into program-attribution claims.
pub fn apply_decoded_transaction(bundle: &mut LineageBundle, dec: &DecodedTransaction) {
    let known = known_programs();
    bundle.transaction_construction.present = true;
    bundle.transaction_construction.encoding = Some("base64".into());
    bundle.transaction_construction.transaction_type = Some(dec.transaction_type.clone());
    bundle.transaction_construction.fee_payer = dec.fee_payer.clone();
    bundle.transaction_construction.num_instructions = Some(dec.instructions.len());
    bundle.transaction_construction.num_lookup_tables =
        Some(dec.address_lookup_table_references.len());

    let mut programs = Vec::new();
    let mut labels = Vec::new();
    for ix in &dec.instructions {
        programs.push(ix.program_id.clone());
        labels.push(ix.program_label.clone());
        if ix.program_label != "unknown" && ix.program_label != "unclassified" {
            let level = if ix.program_label.starts_with("candidate_") {
                EvidenceLevel::Candidate
            } else if known.contains_key(ix.program_id.as_str()) {
                EvidenceLevel::ExternalProgramLabel
            } else {
                EvidenceLevel::DecodedFromTransaction
            };
            bundle.push_claim(
                AttributionClaim::new(
                    "instruction",
                    "invokes_program",
                    format!("{} ({})", ix.program_label, ix.program_id),
                    level,
                    &bundle.capture.artifact_id,
                    format!(
                        "{} appears as program ID in instruction {}",
                        ix.program_label, ix.index
                    ),
                )
                .with_instruction(ix.index),
            );
        }
    }
    programs.sort();
    programs.dedup();
    labels.sort();
    labels.dedup();
    bundle.transaction_construction.program_ids = programs.clone();
    bundle.transaction_construction.program_labels = labels;
    bundle.execution.invoked_programs = programs;
    bundle.execution.unknown_program_ids = dec.unknown_program_ids.clone();
    bundle.execution.compute_budget_present = dec
        .instructions
        .iter()
        .any(|i| i.program_label == "compute_budget");
    bundle.delivery.jito_tip_instruction_indexes = dec.candidate_jito_tip_transfers.clone();
    if !dec.candidate_jito_tip_transfers.is_empty() {
        bundle.delivery.notes.push(
            "system transfer touches a known Jito tip account — delivery candidate, not confirmation"
                .into(),
        );
    }
    if bundle.execution.compute_budget_present {
        bundle.delivery.priority_fee_observed = Some(true);
        bundle.delivery.notes.push(
            "compute-budget instruction present; priority fee alone does not identify a delivery service"
                .into(),
        );
    }

    bundle.decoded_transaction = Some(dec.clone());
}

/// Stable, content-derived artifact id for an extraction with no manifest.
pub fn derive_artifact_id(provider: ProviderId, sha256: Option<&str>) -> String {
    match sha256 {
        Some(h) => format!("{}_{}", provider.as_str(), &h[..h.len().min(12)]),
        None => format!("{}_unhashed", provider.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{ProviderAdapter, RawProviderArtifact};
    use crate::execution_context::ExecutionContext;

    fn response_only_ctx() -> ExecutionContext {
        let raw = RawProviderArtifact::from_value(serde_json::json!({
            "input_mint": "MintA",
            "output_mint": "MintB",
            "in_amount": "100",
            "out_amount": "200",
            "route": [{ "venue": "SomeAmm", "input_mint": "MintA", "output_mint": "MintB" }],
        }));
        let e = crate::adapters::generic::GenericAdapter
            .extract(&raw)
            .unwrap();
        ExecutionContext::new(ProviderId::Generic, "test_ctx").with_extraction(e)
    }

    #[test]
    fn response_only_lineage_has_no_transaction_or_settlement_claims() {
        let bundle = build_lineage(&response_only_ctx()).unwrap();

        assert_eq!(bundle.quote.out_amount.as_deref(), Some("200"));
        assert!(!bundle.transaction_construction.present);
        assert!(!bundle.settlement.applicable);
        assert!(bundle.unresolved.iter().any(|u| u.field == "settlement"));
        // No transaction stage means no byte-level links at all.
        assert!(!bundle
            .links
            .iter()
            .any(|l| l.to_stage == Stage::TransactionConstruction));
    }

    /// A response-derived intent agrees with the response by construction, and
    /// the link has to say so rather than reading as corroboration.
    #[test]
    fn intent_response_link_is_marked_as_an_echo() {
        let raw = RawProviderArtifact::from_value(serde_json::json!({
            "inputMint": "MintA",
            "outputMint": "MintB",
            "inAmount": "100",
            "outAmount": "200",
            "otherAmountThreshold": "199",
            "slippageBps": 50,
            "routePlan": [],
            "requestId": "r1",
        }));
        let e = crate::adapters::dflow::DflowAdapter.extract(&raw).unwrap();
        let ctx = ExecutionContext::new(ProviderId::Dflow, "t").with_extraction(e);
        let bundle = build_lineage(&ctx).unwrap();

        let link = bundle
            .links
            .iter()
            .find(|l| l.id == "intent_to_response:input_mint")
            .expect("intent link");
        assert_eq!(link.relationship, "same_value");
        assert!(link.claim_ceiling.contains("not independent corroboration"));
    }

    #[test]
    fn response_route_mint_mismatch_is_recorded_not_hidden() {
        let raw = RawProviderArtifact::from_value(serde_json::json!({
            "input_mint": "MintA",
            "output_mint": "MintB",
            "route": [{ "venue": "X", "input_mint": "MintZ", "output_mint": "MintB" }],
        }));
        let e = crate::adapters::generic::GenericAdapter
            .extract(&raw)
            .unwrap();
        let ctx = ExecutionContext::new(ProviderId::Generic, "t").with_extraction(e);
        let bundle = build_lineage(&ctx).unwrap();

        let link = bundle
            .links
            .iter()
            .find(|l| l.id == "response_to_route:input_mint")
            .unwrap();
        assert_eq!(link.relationship, "value_mismatch");
    }

    #[test]
    fn derived_artifact_id_is_stable_and_content_derived() {
        let a = derive_artifact_id(ProviderId::Dflow, Some("abcdef0123456789"));
        assert_eq!(a, "dflow_abcdef012345");
        assert_eq!(
            a,
            derive_artifact_id(ProviderId::Dflow, Some("abcdef0123456789"))
        );
    }
}
