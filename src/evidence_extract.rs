//! Deterministic public evidence extract for the route-stable bracket run.
//!
//! Everything the article, the figures and the evidence lab publish about this
//! experiment is computed here, once, in Rust, and written alongside the bracket
//! report. Downstream tools are renderers: they read this file and format it.
//!
//! This exists because the empirical values had drifted into three places —
//! the Rust runner recorded raw response fields, a Python script recomputed the
//! quote-stage identity and re-ran the byte search with its own transaction
//! decoder, and the browser recomputed the threshold again. Three
//! implementations of the same arithmetic is three chances to disagree.
//!
//! Scope note: the bracket report's own candidate search answers a narrower
//! question (which amounts appear in the *changed* payloads of an A1-vs-T
//! comparison). The extract answers the published one: across every request of
//! every eligible bracket, searching every instruction payload, where does each
//! searched amount appear. Both are kept — they are different questions.
//!
//! Public-safe by construction: response amounts and byte *positions* only. No
//! payload bytes, no account addresses, no raw transactions, no timestamps.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_sdk::transaction::VersionedTransaction;

use crate::route_bracket::BracketExperimentReport;
use crate::tx_compare::{amount_needles, find_all_subslices};

pub const EVIDENCE_EXTRACT_SCHEMA: &str = "1.0.0";

/// The quantities searched for inside instruction payloads, in a fixed order.
pub const SEARCHED_QUANTITIES: [&str; 3] = [
    "otherAmountThreshold",
    "outAmount",
    "outAmount_minus_threshold",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdObservation {
    pub batch_index: usize,
    pub role: String,
    pub slippage_bps: u32,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub predicted_threshold: String,
    pub exact_match: bool,
    pub floor_variant_matches: bool,
    pub min_out_equals_threshold: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdIdentity {
    pub formula: String,
    pub arithmetic: String,
    pub observations: usize,
    pub exact_matches: usize,
    pub holds_for_all: bool,
    pub floor_variant_matches: usize,
    pub min_out_equals_threshold_all: bool,
    pub detail: Vec<ThresholdObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchSite {
    pub instruction_index: usize,
    pub byte_offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestSearch {
    pub batch_index: usize,
    pub role: String,
    pub slippage_bps: u32,
    pub out_amount: String,
    pub other_amount_threshold: String,
    pub instruction_count: usize,
    pub payload_lengths: Vec<usize>,
    /// Distinct byte positions per searched quantity, encodings collapsed.
    pub sites: Vec<(String, Vec<MatchSite>)>,
    /// Encoding labels that produced a hit, per quantity, before collapsing.
    pub encodings_hit: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorControlPair {
    pub batch_index: usize,
    pub slippage_bps: u32,
    pub q_a1: String,
    pub q_a2: String,
    pub quote_differs: bool,
    pub candidate_carries_own_quote: bool,
    pub route_same: bool,
    pub topology_same: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateResult {
    pub searched_quantities: Vec<String>,
    pub target_quantity: String,
    pub transactions_searched: usize,
    pub searched_every_instruction: bool,
    pub encoding_family: Vec<String>,
    pub quote_matched_in_all: bool,
    pub quote_site_unique_in_all: bool,
    pub quote_sites: Vec<MatchSite>,
    pub threshold_sites_total: usize,
    pub difference_sites_total: usize,
    pub canonical_encoding: String,
    pub canonical_note: String,
    pub evidence_ceiling: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceExtract {
    pub schema_version: String,
    pub experiment_id: String,
    pub generated_by: String,
    pub design: Value,
    pub threshold_identity: ThresholdIdentity,
    pub eligible_batches: Vec<usize>,
    pub eligible_batch_count: usize,
    pub total_batches: usize,
    pub total_requests: usize,
    pub candidate_result: CandidateResult,
    pub per_request_search: Vec<RequestSearch>,
    pub anchor_control: Vec<AnchorControlPair>,
}

/// `M = ceil(Q * (10000 - S) / 10000)`, exact integer arithmetic on base units.
fn predicted_threshold(out_amount: u128, slippage_bps: u32) -> u128 {
    let numerator = out_amount * u128::from(10_000 - slippage_bps);
    numerator.div_ceil(10_000)
}

fn floor_threshold(out_amount: u128, slippage_bps: u32) -> u128 {
    out_amount * u128::from(10_000 - slippage_bps) / 10_000
}

fn read_response(base_dir: &Path, raw_path: &str) -> Result<Value> {
    let path = base_dir.join(raw_path);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read response {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Every distinct byte position where `value` appears under the tested family.
fn search_payloads(instructions: &[Vec<u8>], value: u128) -> (Vec<MatchSite>, Vec<String>) {
    let mut sites = BTreeSet::new();
    let mut encodings = BTreeSet::new();
    for (width, needle, label) in amount_needles(value) {
        let _ = width;
        for (index, data) in instructions.iter().enumerate() {
            for offset in find_all_subslices(data, &needle) {
                sites.insert(MatchSite {
                    instruction_index: index,
                    byte_offset: offset,
                });
                encodings.insert(label.to_string());
            }
        }
    }
    (sites.into_iter().collect(), encodings.into_iter().collect())
}

fn instruction_payloads(b64: &str) -> Result<Vec<Vec<u8>>> {
    let raw = STANDARD.decode(b64.trim())?;
    let vtx: VersionedTransaction = bincode::deserialize(&raw)?;
    Ok(vtx
        .message
        .instructions()
        .iter()
        .map(|ix| ix.data.clone())
        .collect())
}

/// Build the extract from an already-written bracket report.
pub fn build(report: &BracketExperimentReport, base_dir: &Path) -> Result<EvidenceExtract> {
    let mut observations = Vec::new();
    let mut per_request = Vec::new();
    let mut anchor_control = Vec::new();
    let mut eligible_batches = Vec::new();
    let mut total_requests = 0usize;

    for batch in &report.batches {
        total_requests += batch.requests.len();

        // Quote-stage identity: every request in the run, eligible or not.
        for request in &batch.requests {
            let (Some(out_amount), Some(threshold)) =
                (&request.out_amount, &request.other_amount_threshold)
            else {
                continue;
            };
            let q: u128 = out_amount.parse()?;
            let m: u128 = threshold.parse()?;
            let response = read_response(base_dir, &request.raw_path)?;
            let min_out = response.get("minOutAmount").and_then(Value::as_str);

            observations.push(ThresholdObservation {
                batch_index: batch.batch_index,
                role: request.role.clone(),
                slippage_bps: request.slippage_bps,
                out_amount: out_amount.clone(),
                other_amount_threshold: threshold.clone(),
                predicted_threshold: predicted_threshold(q, request.slippage_bps).to_string(),
                exact_match: predicted_threshold(q, request.slippage_bps) == m,
                floor_variant_matches: floor_threshold(q, request.slippage_bps) == m,
                min_out_equals_threshold: min_out == Some(threshold.as_str()),
            });
        }

        if !batch.eligible_for_instruction_diff {
            continue;
        }
        eligible_batches.push(batch.batch_index);

        // Byte search: every request of the bracket, every instruction payload.
        let mut by_role = std::collections::BTreeMap::new();
        for request in &batch.requests {
            let (Some(out_amount), Some(threshold), Some(tx_path)) = (
                &request.out_amount,
                &request.other_amount_threshold,
                &request.transaction_b64_path,
            ) else {
                continue;
            };
            let q: u128 = out_amount.parse()?;
            let m: u128 = threshold.parse()?;
            let b64 = std::fs::read_to_string(base_dir.join(tx_path))?;
            let payloads = instruction_payloads(&b64)?;

            let values = [
                ("otherAmountThreshold", m),
                ("outAmount", q),
                ("outAmount_minus_threshold", q.saturating_sub(m)),
            ];
            let mut sites = Vec::new();
            let mut encodings_hit = Vec::new();
            for (label, value) in values {
                let (found, labels) = search_payloads(&payloads, value);
                sites.push((label.to_string(), found));
                encodings_hit.push((label.to_string(), labels));
            }

            by_role.insert(request.role.clone(), (q, sites.clone()));
            per_request.push(RequestSearch {
                batch_index: batch.batch_index,
                role: request.role.clone(),
                slippage_bps: request.slippage_bps,
                out_amount: out_amount.clone(),
                other_amount_threshold: threshold.clone(),
                instruction_count: payloads.len(),
                payload_lengths: payloads.iter().map(Vec::len).collect(),
                sites,
                encodings_hit,
            });
        }

        // Same-treatment control: the two anchors carry an identical setting.
        if let (Some((q_a1, sites_a1)), Some((q_a2, sites_a2))) =
            (by_role.get("A1"), by_role.get("A2"))
        {
            let quote_sites = |sites: &Vec<(String, Vec<MatchSite>)>| {
                sites
                    .iter()
                    .find(|(label, _)| label == "outAmount")
                    .map(|(_, s)| !s.is_empty())
                    .unwrap_or(false)
            };
            let a1 = batch.requests.iter().find(|r| r.role == "A1");
            let a2 = batch.requests.iter().find(|r| r.role == "A2");
            anchor_control.push(AnchorControlPair {
                batch_index: batch.batch_index,
                slippage_bps: a1.map(|r| r.slippage_bps).unwrap_or_default(),
                q_a1: q_a1.to_string(),
                q_a2: q_a2.to_string(),
                quote_differs: q_a1 != q_a2,
                candidate_carries_own_quote: quote_sites(sites_a1) && quote_sites(sites_a2),
                route_same: a1
                    .and_then(|r| r.route_fingerprint.as_ref())
                    .map(|f| &f.sha256)
                    == a2
                        .and_then(|r| r.route_fingerprint.as_ref())
                        .map(|f| &f.sha256),
                topology_same: a1.map(|r| &r.program_set) == a2.map(|r| &r.program_set),
            });
        }
    }

    let exact_matches = observations.iter().filter(|o| o.exact_match).count();
    let floor_matches = observations
        .iter()
        .filter(|o| o.floor_variant_matches)
        .count();

    let quote_site_sets: Vec<&Vec<MatchSite>> = per_request
        .iter()
        .filter_map(|r| {
            r.sites
                .iter()
                .find(|(label, _)| label == "outAmount")
                .map(|(_, s)| s)
        })
        .collect();
    let mut quote_sites = BTreeSet::new();
    for set in &quote_site_sets {
        quote_sites.extend(set.iter().cloned());
    }
    let count_sites = |label: &str| -> usize {
        per_request
            .iter()
            .filter_map(|r| r.sites.iter().find(|(l, _)| l == label))
            .map(|(_, s)| s.len())
            .sum()
    };

    let threshold_identity = ThresholdIdentity {
        formula: "M = ceil(Q * (10000 - S) / 10000)".into(),
        arithmetic: "exact integer, token base units, no floating point".into(),
        observations: observations.len(),
        exact_matches,
        holds_for_all: exact_matches == observations.len(),
        floor_variant_matches: floor_matches,
        min_out_equals_threshold_all: observations.iter().all(|o| o.min_out_equals_threshold),
        detail: observations,
    };

    let candidate_result = CandidateResult {
        searched_quantities: SEARCHED_QUANTITIES.iter().map(|s| s.to_string()).collect(),
        target_quantity: "otherAmountThreshold".into(),
        transactions_searched: per_request.len(),
        searched_every_instruction: true,
        encoding_family: vec![
            "u64_le".into(),
            "u64_be".into(),
            "u32_le".into(),
            "u32_be".into(),
        ],
        quote_matched_in_all: !quote_site_sets.is_empty()
            && quote_site_sets.iter().all(|s| !s.is_empty()),
        quote_site_unique_in_all: !quote_site_sets.is_empty()
            && quote_site_sets.iter().all(|s| s.len() == 1),
        quote_sites: quote_sites.into_iter().collect(),
        threshold_sites_total: count_sites("otherAmountThreshold"),
        difference_sites_total: count_sites("outAmount_minus_threshold"),
        canonical_encoding: "8-byte little-endian".into(),
        canonical_note: "The matched bytes are an 8-byte little-endian integer whose top four \
             bytes are zero, because every observed amount is below 2^32. A 4-byte read of the \
             same offset therefore also matches: the same physical bytes, not a second finding."
            .into(),
        evidence_ceiling: "Byte equality under one encoding family. Not semantic decoding: no \
             IDL, no official decoder, no protocol schema was used. An empty threshold result \
             means not recovered under this search, not absent from the transaction."
            .into(),
    };

    Ok(EvidenceExtract {
        schema_version: EVIDENCE_EXTRACT_SCHEMA.into(),
        experiment_id: report.experiment_id.clone(),
        generated_by: "onchain-execution-lineage :: route-bracket".into(),
        design: serde_json::json!({
            "provider": "DFlow developer /order",
            "bracket_pattern": "A1 / T / A2",
            "read_only": true,
            "signed": false,
            "submitted": false,
        }),
        threshold_identity,
        eligible_batch_count: eligible_batches.len(),
        eligible_batches,
        total_batches: report.batches.len(),
        total_requests,
        candidate_result,
        per_request_search: per_request,
        anchor_control,
    })
}

/// Where the published copy of the extract lives, relative to the repo root.
///
/// The run directory under `artifacts/experiments/` is gitignored along with the
/// raw captures, so a copy written only there would never reach a reader. The
/// extract is public-safe by construction — response amounts and byte positions,
/// no addresses, no payload bytes, no timestamps — so the published copy goes to
/// the tracked analysis directory and is the source every renderer reads.
pub const PUBLIC_EXTRACT_PATH: &str = "artifacts/analysis/route_stable_evidence_extract.json";

/// Write the extract beside the bracket report and to the published location.
pub fn write(report: &BracketExperimentReport, base_dir: &Path, out_dir: &Path) -> Result<()> {
    let extract = build(report, base_dir)?;
    let serialized = serde_json::to_string_pretty(&extract)? + "\n";

    // Alongside the run, for whoever is looking at that run's artifacts.
    std::fs::write(out_dir.join("evidence_extract.json"), &serialized)?;

    // The tracked copy the article, figures and lab consume.
    let public = base_dir.join(PUBLIC_EXTRACT_PATH);
    if let Some(parent) = public.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&public, &serialized)
        .with_context(|| format!("write published extract {}", public.display()))?;
    Ok(())
}
