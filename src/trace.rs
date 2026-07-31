//! Build a LineageBundle from artifacts + optional transaction / signature.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::artifact::ArtifactManifest;
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::{CaptureMetadata, LineageBundle};
use crate::lookup_tables;
use crate::program_registry::known_programs;
use crate::providers;
use crate::rpc;
use crate::settlement;
use crate::transaction::{self, DecodedTransaction};

pub struct TraceInputs<'a> {
    pub manifest: Option<&'a ArtifactManifest>,
    pub provider_json_path: Option<&'a Path>,
    pub transaction_b64_path: Option<&'a Path>,
    pub signature: Option<&'a str>,
    pub rpc_url: &'a str,
    pub resolve_alts: bool,
    pub enrich_settlement: bool,
}

pub async fn build_trace(inputs: TraceInputs<'_>) -> Result<LineageBundle> {
    let capture = if let Some(m) = inputs.manifest {
        m.validate()?;
        CaptureMetadata {
            artifact_id: m.artifact_id.clone(),
            provider: m.provider.clone(),
            surface: m.surface.clone(),
            captured_at_utc: m.captured_at_utc.clone(),
            pair: m.pair.clone(),
        }
    } else {
        CaptureMetadata {
            artifact_id: "ad_hoc".into(),
            provider: "unknown".into(),
            surface: "cli".into(),
            captured_at_utc: chrono::Utc::now().to_rfc3339(),
            pair: "unknown".into(),
        }
    };

    let mut bundle = LineageBundle::new(capture);

    if let Some(path) = inputs.provider_json_path {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read provider json {}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        let adapter = providers::normalize_provider_json(&value, &mut bundle)?;
        bundle
            .raw_extensions
            .insert("_adapter".into(), serde_json::json!(adapter));
    }

    let mut decoded: Option<DecodedTransaction> = None;

    if let Some(path) = inputs.transaction_b64_path {
        let b64 = fs::read_to_string(path)
            .with_context(|| format!("read transaction file {}", path.display()))?;
        decoded = Some(transaction::decode_base64_transaction(&b64)?);
    } else if let Some(sig) = inputs.signature {
        let b64 = rpc::fetch_transaction_base64(inputs.rpc_url, sig).await?;
        decoded = Some(transaction::decode_base64_transaction(&b64)?);
        bundle.settlement.applicable = true;
        bundle.settlement.signature = Some(sig.to_string());
    }

    if let Some(ref dec) = decoded {
        apply_decoded_transaction(&mut bundle, dec);

        if inputs.resolve_alts && !dec.address_lookup_table_references.is_empty() {
            let mut tables = BTreeMap::new();
            for alt in &dec.address_lookup_table_references {
                match lookup_tables::resolve_lookup_table(inputs.rpc_url, &alt.lookup_table_account)
                    .await
                {
                    Ok(addrs) => {
                        tables.insert(alt.lookup_table_account.clone(), addrs);
                    }
                    Err(e) => {
                        bundle.push_unresolved(
                            format!("alt:{}", alt.lookup_table_account),
                            e.to_string(),
                        );
                    }
                }
            }
            if !tables.is_empty() {
                if let Ok(map) = crate::instruction_map::build_instruction_account_map(dec, &tables)
                {
                    bundle.execution.loaded_account_count = Some(map.total_account_vector_len);
                    bundle.raw_extensions.insert(
                        "loaded_address_summary".into(),
                        serde_json::json!({
                            "total_static_keys": map.total_static_keys,
                            "total_loaded_from_alts": map.total_loaded_from_alts,
                            "total_addresses_in_referenced_tables": map.total_addresses_in_referenced_tables,
                        }),
                    );

                    // Owner-derived candidate integrator markers.
                    let addresses: Vec<String> = map
                        .loaded_addresses
                        .iter()
                        .map(|a| a.address.clone())
                        .collect();
                    if let Ok(facts) = rpc::fetch_account_facts(inputs.rpc_url, &addresses).await {
                        let mut annotated = map;
                        crate::instruction_map::annotate_with_account_facts(&mut annotated, &facts);
                        let markers = crate::instruction_map::owner_derived_markers(
                            &annotated,
                            "candidate_integrator_program",
                        );
                        for (addr, owner) in markers {
                            bundle.push_claim(
                                AttributionClaim::new(
                                    addr,
                                    "owned_by_candidate_integrator",
                                    owner,
                                    EvidenceLevel::Candidate,
                                    &bundle.capture.artifact_id,
                                    "account owner matches a candidate integrator program label; origin unconfirmed",
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    if inputs.enrich_settlement {
        let sig = inputs
            .signature
            .map(|s| s.to_string())
            .or_else(|| bundle.settlement.signature.clone());
        if let Some(sig) = sig {
            settlement::enrich_settlement(&mut bundle, inputs.rpc_url, &sig).await?;
        }
    }

    if !bundle.settlement.applicable {
        bundle.push_unresolved(
            "settlement",
            "not applicable — artifact is unsigned or no signature was supplied",
        );
        bundle.assert_unsigned_has_no_settlement_claims()?;
    }

    // Always-unresolved interface-level items unless separately evidenced.
    bundle.push_unresolved(
        "private_route_selection_policy",
        "router internal policy is not observable from quote JSON or transaction bytes",
    );
    bundle.push_unresolved(
        "delivery_channel",
        "Jito tip match or forJitoBundle flag is infrastructure evidence, not confirmed delivery path",
    );

    Ok(bundle)
}

fn apply_decoded_transaction(bundle: &mut LineageBundle, dec: &DecodedTransaction) {
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

    bundle.decoded_transaction = Some(DecodedTransaction {
        transaction_type: dec.transaction_type.clone(),
        num_signature_slots: dec.num_signature_slots,
        fee_payer: dec.fee_payer.clone(),
        recent_blockhash: dec.recent_blockhash.clone(),
        static_account_keys: dec.static_account_keys.clone(),
        address_lookup_table_references: dec.address_lookup_table_references.clone(),
        instructions: dec.instructions.clone(),
        candidate_jito_tip_transfers: dec.candidate_jito_tip_transfers.clone(),
        unknown_program_ids: dec.unknown_program_ids.clone(),
    });
}
