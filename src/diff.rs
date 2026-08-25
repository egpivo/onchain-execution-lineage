//! Diff two LineageBundles without auto-promoting unique fields to fingerprints.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::lineage_model::LineageBundle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffClass {
    ProviderGeneric,
    AppCandidate,
    ManagedInterfaceCandidate,
    WalletSpecific,
    PairSizeSpecific,
    TransactionSpecific,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    pub field: String,
    pub left: String,
    pub right: String,
    pub class: DiffClass,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageDiff {
    pub schema_version: String,
    pub left_artifact_id: String,
    pub right_artifact_id: String,
    pub shared_programs: Vec<String>,
    pub programs_only_left: Vec<String>,
    pub programs_only_right: Vec<String>,
    pub shared_accounts: Vec<String>,
    pub accounts_only_left: Vec<String>,
    pub accounts_only_right: Vec<String>,
    pub entries: Vec<DiffEntry>,
}

pub fn diff_bundles(left: &LineageBundle, right: &LineageBundle) -> LineageDiff {
    let lp: BTreeSet<_> = left
        .transaction_construction
        .program_ids
        .iter()
        .cloned()
        .collect();
    let rp: BTreeSet<_> = right
        .transaction_construction
        .program_ids
        .iter()
        .cloned()
        .collect();
    let shared_programs: Vec<_> = lp.intersection(&rp).cloned().collect();
    let programs_only_left: Vec<_> = lp.difference(&rp).cloned().collect();
    let programs_only_right: Vec<_> = rp.difference(&lp).cloned().collect();

    let la = account_set(left);
    let ra = account_set(right);
    let shared_accounts: Vec<_> = la.intersection(&ra).cloned().collect();
    let accounts_only_left: Vec<_> = la.difference(&ra).cloned().collect();
    let accounts_only_right: Vec<_> = ra.difference(&la).cloned().collect();

    let mut entries = Vec::new();
    push_cmp(
        &mut entries,
        "fee_payer",
        opt(&left.transaction_construction.fee_payer),
        opt(&right.transaction_construction.fee_payer),
        DiffClass::WalletSpecific,
        "fee payer is wallet-specific unless sponsor policy is separately evidenced",
    );
    push_cmp(
        &mut entries,
        "transaction_type",
        opt(&left.transaction_construction.transaction_type),
        opt(&right.transaction_construction.transaction_type),
        DiffClass::TransactionSpecific,
        "legacy vs v0 / ALT usage",
    );
    push_cmp(
        &mut entries,
        "transaction_present",
        bool_str(left.transaction_construction.present),
        bool_str(right.transaction_construction.present),
        DiffClass::ProviderGeneric,
        "whether a transaction field/payload was available",
    );
    push_cmp(
        &mut entries,
        "num_instructions",
        num(left.transaction_construction.num_instructions),
        num(right.transaction_construction.num_instructions),
        DiffClass::TransactionSpecific,
        "compiled instruction count",
    );
    push_cmp(
        &mut entries,
        "num_lookup_tables",
        num(left.transaction_construction.num_lookup_tables),
        num(right.transaction_construction.num_lookup_tables),
        DiffClass::TransactionSpecific,
        "ALT reference count",
    );
    push_cmp(
        &mut entries,
        "loaded_account_count",
        num(left.execution.loaded_account_count),
        num(right.execution.loaded_account_count),
        DiffClass::TransactionSpecific,
        "static + ALT-loaded account vector length",
    );
    push_cmp(
        &mut entries,
        "route_label",
        opt(&left.route.provider_route_label),
        opt(&right.route.provider_route_label),
        DiffClass::ManagedInterfaceCandidate,
        "provider/UI route label — not an automatic app fingerprint",
    );
    push_cmp(
        &mut entries,
        "compute_budget_present",
        bool_str(left.execution.compute_budget_present),
        bool_str(right.execution.compute_budget_present),
        DiffClass::ProviderGeneric,
        "compute-budget presence is not delivery proof",
    );
    push_cmp(
        &mut entries,
        "settlement_applicable",
        bool_str(left.settlement.applicable),
        bool_str(right.settlement.applicable),
        DiffClass::Unresolved,
        "settlement fields only apply when a signature is examined",
    );

    // Never auto-promote a unique program to app fingerprint.
    for p in &programs_only_left {
        entries.push(DiffEntry {
            field: format!("program_only_left:{p}"),
            left: p.clone(),
            right: String::new(),
            class: DiffClass::AppCandidate,
            note: "unique to left; remains a candidate until repeated observations and negative controls exist".into(),
        });
    }
    for p in &programs_only_right {
        entries.push(DiffEntry {
            field: format!("program_only_right:{p}"),
            left: String::new(),
            right: p.clone(),
            class: DiffClass::AppCandidate,
            note: "unique to right; remains a candidate until repeated observations and negative controls exist".into(),
        });
    }

    LineageDiff {
        schema_version: crate::evidence::LINEAGE_SCHEMA_VERSION.to_string(),
        left_artifact_id: left.capture.artifact_id.clone(),
        right_artifact_id: right.capture.artifact_id.clone(),
        shared_programs,
        programs_only_left,
        programs_only_right,
        shared_accounts,
        accounts_only_left,
        accounts_only_right,
        entries,
    }
}

fn account_set(b: &LineageBundle) -> BTreeSet<String> {
    b.decoded_transaction
        .as_ref()
        .map(|d| d.static_account_keys.iter().cloned().collect())
        .unwrap_or_default()
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}
fn num(v: Option<usize>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}
fn bool_str(v: bool) -> String {
    v.to_string()
}

fn push_cmp(
    entries: &mut Vec<DiffEntry>,
    field: &str,
    left: String,
    right: String,
    class: DiffClass,
    note: &str,
) {
    if left == right {
        return;
    }
    entries.push(DiffEntry {
        field: field.into(),
        left,
        right,
        class,
        note: note.into(),
    });
}
