//! Bracketed DFlow `/order` route-stability runner.
//!
//! Pattern per batch: A1 (50 bps) → T (10 or 100) → A2 (50 bps).
//! Caps: 10 batches / 30 requests. No signing. No submission.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::artifact::sha256_bytes;
use crate::route_fingerprint::{
    classify_route_pair, fingerprint_route_plan, RouteFingerprint, RouteStabilityClass,
};
use crate::transaction;
use crate::tx_compare::{
    diff_canonical_transactions, diff_instruction_payloads, diff_raw_transactions,
    search_candidate_encodings_in_payload, CandidateEncodingHit, CanonicalTransactionDiff,
    PayloadDiffReport, RawTransactionDiff,
};

pub const BRACKET_SCHEMA_VERSION: &str = "1.0.0";
const MAX_BATCHES_HARD_CAP: usize = 10;
const MAX_REQUESTS_HARD_CAP: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketManifest {
    pub schema_version: String,
    pub experiment_id: String,
    pub provider: String,
    pub endpoint_hostname: String,
    pub endpoint_path: String,
    pub pair: String,
    pub input_amount: String,
    pub platform_fee_bps: u32,
    pub anchor_slippage_bps: u32,
    pub treatment_slippage_bps_values: Vec<u32>,
    pub maximum_batches: usize,
    pub maximum_request_count: usize,
    pub pacing_ms: u64,
    pub output_path: String,
    #[serde(default)]
    pub user_public_key: Option<String>,
    #[serde(default)]
    pub user_public_key_env: Option<String>,
    pub notes: String,
}

impl BracketManifest {
    pub fn load_path(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read bracket manifest {}", path.display()))?;
        let m: Self = serde_json::from_str(&text)?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != BRACKET_SCHEMA_VERSION {
            bail!(
                "unsupported bracket schema_version '{}'",
                self.schema_version
            );
        }
        if self.provider != "dflow" || self.endpoint_path != "order" {
            bail!("bracket runner supports dflow /order only");
        }
        if self.endpoint_hostname != "dev-quote-api.dflow.net" {
            bail!("unsafe endpoint_hostname '{}'", self.endpoint_hostname);
        }
        if self.maximum_batches == 0 || self.maximum_batches > MAX_BATCHES_HARD_CAP {
            bail!("maximum_batches must be 1..={MAX_BATCHES_HARD_CAP}");
        }
        if self.maximum_request_count == 0 || self.maximum_request_count > MAX_REQUESTS_HARD_CAP {
            bail!("maximum_request_count must be 1..={MAX_REQUESTS_HARD_CAP}");
        }
        if self.maximum_batches * 3 > self.maximum_request_count {
            bail!("maximum_batches * 3 exceeds maximum_request_count");
        }
        if self.treatment_slippage_bps_values.is_empty() {
            bail!("treatment_slippage_bps_values must be non-empty");
        }
        if self.user_public_key.is_none() && self.user_public_key_env.is_none() {
            bail!("user_public_key or user_public_key_env required");
        }
        let blob = serde_json::to_string(self)?;
        for needle in [
            "authorization:",
            "bearer ",
            "private_key",
            "api_key=",
            "secret_key",
        ] {
            if blob.to_lowercase().contains(needle) {
                bail!("manifest appears to contain secret material ({needle})");
            }
        }
        Ok(())
    }

    pub fn resolve_user_public_key(&self) -> Result<String> {
        if let Some(env_name) = &self.user_public_key_env {
            if let Ok(v) = std::env::var(env_name) {
                let v = v.trim().to_string();
                if !v.is_empty() {
                    return Ok(v);
                }
            }
        }
        self.user_public_key
            .clone()
            .context("user_public_key missing and env unset")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketRequestRecord {
    pub role: String,
    pub slippage_bps: u32,
    pub request_timestamp_utc: String,
    pub response_timestamp_utc: String,
    pub http_status: u16,
    pub raw_path: String,
    pub raw_sha256: String,
    pub meta_path: String,
    pub transaction_present: bool,
    pub transaction_b64_path: Option<String>,
    pub execution_mode: Option<String>,
    pub out_amount: Option<String>,
    pub other_amount_threshold: Option<String>,
    pub last_valid_block_height: Option<u64>,
    pub route_fingerprint: Option<RouteFingerprint>,
    pub parse_ok: bool,
    pub alt_resolve_ok: Option<bool>,
    pub program_set: Vec<String>,
    pub transaction_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketBatchResult {
    pub batch_index: usize,
    pub treatment_slippage_bps: u32,
    pub pattern: String,
    pub requests: Vec<BracketRequestRecord>,
    pub complete: bool,
    pub exact_route_stable: bool,
    pub route_class_a1_t: Option<String>,
    pub route_class_t_a2: Option<String>,
    pub route_class_a1_a2: Option<String>,
    pub topology_stable_a1_t: Option<bool>,
    pub topology_stable_t_a2: Option<bool>,
    pub eligible_for_instruction_diff: bool,
    pub ineligibility_reasons: Vec<String>,
    pub raw_tx_diff_a1_t: Option<RawTransactionDiff>,
    pub canonical_tx_diff_a1_t: Option<CanonicalTransactionDiff>,
    pub payload_diff_a1_t: Option<PayloadDiffReport>,
    pub candidate_encodings: Vec<CandidateEncodingHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketExperimentReport {
    pub schema_version: String,
    pub experiment_id: String,
    pub endpoint: String,
    pub route_fingerprint_definition: String,
    pub attempted_batches: usize,
    pub total_requests: usize,
    pub complete_batches: usize,
    pub exact_route_stable_batches: usize,
    pub topology_stable_batches: usize,
    pub eligible_instruction_diff_batches: usize,
    pub treatment_split: BTreeMap<String, usize>,
    pub route_fingerprint_distribution: BTreeMap<String, usize>,
    pub batches: Vec<BracketBatchResult>,
    pub evidence_ceiling: Vec<String>,
    pub notes: String,
}

pub async fn run_route_bracket_experiment(
    manifest_path: &Path,
    base_dir: &Path,
    rpc_url: &str,
    resolve_alts: bool,
) -> Result<BracketExperimentReport> {
    let manifest = BracketManifest::load_path(manifest_path)?;
    let user_pk = manifest.resolve_user_public_key()?;
    let (input_mint, output_mint, _) = crate::pairs::resolve_pair(&manifest.pair)?;

    let out_dir = resolve_against(base_dir, &manifest.output_path);
    fs::create_dir_all(&out_dir)?;
    for sub in ["raw", "meta", "tx", "batches"] {
        fs::create_dir_all(out_dir.join(sub))?;
    }

    // Freeze fingerprint definition text before any inspection of results.
    let fingerprint_def = concat!(
        "route_fingerprint v1.0.0: for each routePlan leg in original order, ",
        "join venue|marketKey|inputMint|outputMint|allocation_bps|inAmount|route_leg_type ",
        "with '||' separators; allocation_bps = floor(leg.inAmount * 10000 / top-level inAmount); ",
        "route_leg_type = dynamic if leg.data present else single_market; legs never sorted."
    );
    fs::write(
        out_dir.join("route_fingerprint_definition.txt"),
        fingerprint_def,
    )?;

    let mut batches = Vec::new();
    let mut total_requests = 0usize;
    let mut treatment_split: BTreeMap<String, usize> = BTreeMap::new();
    let mut fp_dist: BTreeMap<String, usize> = BTreeMap::new();

    let treatments = &manifest.treatment_slippage_bps_values;
    for batch_index in 0..manifest.maximum_batches {
        if total_requests + 3 > manifest.maximum_request_count {
            break;
        }
        let treatment = treatments[batch_index % treatments.len()];
        *treatment_split.entry(treatment.to_string()).or_insert(0) += 1;

        let pattern = format!(
            "{} / {} / {}",
            manifest.anchor_slippage_bps, treatment, manifest.anchor_slippage_bps
        );
        let roles = [
            ("A1", manifest.anchor_slippage_bps),
            ("T", treatment),
            ("A2", manifest.anchor_slippage_bps),
        ];

        let mut records = Vec::new();
        for (role, slip) in roles {
            if batch_index > 0 || role != "A1" {
                tokio::time::sleep(Duration::from_millis(manifest.pacing_ms)).await;
            }
            let rec = fetch_order_record(
                &manifest,
                &out_dir,
                base_dir,
                batch_index,
                role,
                slip,
                input_mint,
                output_mint,
                &user_pk,
                rpc_url,
                resolve_alts,
            )
            .await?;
            total_requests += 1;
            if let Some(fp) = &rec.route_fingerprint {
                *fp_dist.entry(fp.sha256.clone()).or_insert(0) += 1;
            }
            records.push(rec);
        }

        let batch = analyze_batch(
            batch_index,
            treatment,
            &pattern,
            records,
            base_dir,
            rpc_url,
            resolve_alts,
        )
        .await?;
        fs::write(
            out_dir
                .join("batches")
                .join(format!("batch_{batch_index:02}.json")),
            serde_json::to_string_pretty(&batch)?,
        )?;
        batches.push(batch);
    }

    let complete_batches = batches.iter().filter(|b| b.complete).count();
    let exact_route_stable_batches = batches.iter().filter(|b| b.exact_route_stable).count();
    let topology_stable_batches = batches
        .iter()
        .filter(|b| b.topology_stable_a1_t == Some(true) && b.topology_stable_t_a2 == Some(true))
        .count();
    let eligible_instruction_diff_batches = batches
        .iter()
        .filter(|b| b.eligible_for_instruction_diff)
        .count();

    let mut evidence_ceiling = vec![
        "Bracketed live /order constructions only; no signing or settlement.".into(),
        "Route changes across sequential requests are not attributed to slippageBps.".into(),
        "Instruction encoding matches remain Candidate encoding relationship only.".into(),
    ];
    if exact_route_stable_batches == 0 {
        evidence_ceiling.push(
            "No exact-route-stable bracket obtained within the declared request budget.".into(),
        );
    }
    if eligible_instruction_diff_batches == 0 {
        evidence_ceiling.push(
            "No bracket cleared the market-stability and completeness gate for instruction-level comparison."
                .into(),
        );
    }

    let report = BracketExperimentReport {
        schema_version: BRACKET_SCHEMA_VERSION.into(),
        experiment_id: manifest.experiment_id.clone(),
        endpoint: format!(
            "https://{}/{}",
            manifest.endpoint_hostname, manifest.endpoint_path
        ),
        route_fingerprint_definition: fingerprint_def.into(),
        attempted_batches: batches.len(),
        total_requests,
        complete_batches,
        exact_route_stable_batches,
        topology_stable_batches,
        eligible_instruction_diff_batches,
        treatment_split,
        route_fingerprint_distribution: fp_dist,
        batches,
        evidence_ceiling,
        notes: manifest.notes.clone(),
    };

    // Single source of truth for every empirical value published downstream.
    // Written from the finished report so the article, figures and lab all read
    // one file instead of recomputing the same arithmetic three times.
    crate::evidence_extract::write(&report, base_dir, &out_dir)?;

    fs::write(
        out_dir.join("experiment_report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    fs::write(
        out_dir.join("experiment_report.md"),
        render_bracket_markdown(&report),
    )?;
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
async fn fetch_order_record(
    manifest: &BracketManifest,
    out_dir: &Path,
    base_dir: &Path,
    batch_index: usize,
    role: &str,
    slippage_bps: u32,
    input_mint: &str,
    output_mint: &str,
    user_pk: &str,
    rpc_url: &str,
    resolve_alts: bool,
) -> Result<BracketRequestRecord> {
    let url = format!(
        "https://{}/{}?inputMint={}&outputMint={}&amount={}&slippageBps={}&platformFeeBps={}&userPublicKey={}",
        manifest.endpoint_hostname,
        manifest.endpoint_path,
        input_mint,
        output_mint,
        manifest.input_amount,
        slippage_bps,
        manifest.platform_fee_bps,
        user_pk
    );
    let request_timestamp_utc = chrono::Utc::now().to_rfc3339();
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;
    let http_status = resp.status().as_u16();
    let body = resp.text().await?;
    let response_timestamp_utc = chrono::Utc::now().to_rfc3339();

    let stem = format!("b{batch_index:02}_{role}_{slippage_bps}");
    let raw_path = out_dir.join("raw").join(format!("{stem}.json"));
    fs::write(&raw_path, &body)?;
    let raw_sha256 = sha256_bytes(body.as_bytes());
    let meta_path = out_dir.join("meta").join(format!("{stem}.json"));
    fs::write(
        &meta_path,
        serde_json::to_string_pretty(&json!({
            "batch_index": batch_index,
            "role": role,
            "slippage_bps": slippage_bps,
            "request_timestamp_utc": request_timestamp_utc,
            "response_timestamp_utc": response_timestamp_utc,
            "http_status": http_status,
            "endpoint_path": "order",
            "user_public_key_present": true,
        }))?,
    )?;

    let json: Value = serde_json::from_str(&body).unwrap_or(json!({"parse_error": true}));
    let parse_ok = json.get("parse_error").is_none() && json.get("outAmount").is_some();
    let route_fingerprint = if parse_ok {
        fingerprint_route_plan(&json)
    } else {
        None
    };

    let mut transaction_present = false;
    let mut transaction_b64_path = None;
    let mut program_set = Vec::new();
    let mut transaction_version = None;
    let mut alt_resolve_ok = None;

    if let Some(b64) = json.get("transaction").and_then(|t| t.as_str()) {
        transaction_present = true;
        let tx_path = out_dir.join("tx").join(format!("{stem}.b64"));
        fs::write(&tx_path, b64)?;
        transaction_b64_path = Some(relativize(base_dir, &tx_path));
        match transaction::decode_base64_transaction(b64) {
            Ok(decoded) => {
                transaction_version = Some(match decoded.transaction_type.as_str() {
                    "v0_with_alt" => "v0".into(),
                    other => other.to_string(),
                });
                program_set = decoded
                    .instructions
                    .iter()
                    .map(|i| i.program_id.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                if resolve_alts {
                    alt_resolve_ok =
                        Some(resolve_alts_ok(rpc_url, &decoded).await.unwrap_or(false));
                } else {
                    alt_resolve_ok = Some(true);
                }
            }
            Err(_) => {
                alt_resolve_ok = Some(false);
            }
        }
    }

    Ok(BracketRequestRecord {
        role: role.into(),
        slippage_bps,
        request_timestamp_utc,
        response_timestamp_utc,
        http_status,
        raw_path: relativize(base_dir, &raw_path),
        raw_sha256,
        meta_path: relativize(base_dir, &meta_path),
        transaction_present,
        transaction_b64_path,
        execution_mode: json
            .get("executionMode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        out_amount: json
            .get("outAmount")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        other_amount_threshold: json
            .get("otherAmountThreshold")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        last_valid_block_height: json.get("lastValidBlockHeight").and_then(|v| v.as_u64()),
        route_fingerprint,
        parse_ok,
        alt_resolve_ok,
        program_set,
        transaction_version,
    })
}

async fn resolve_alts_ok(
    rpc_url: &str,
    decoded: &crate::transaction::DecodedTransaction,
) -> Result<bool> {
    if decoded.address_lookup_table_references.is_empty() {
        return Ok(true);
    }
    for alt in &decoded.address_lookup_table_references {
        crate::lookup_tables::resolve_lookup_table(rpc_url, &alt.lookup_table_account).await?;
    }
    Ok(true)
}

async fn analyze_batch(
    batch_index: usize,
    treatment: u32,
    pattern: &str,
    records: Vec<BracketRequestRecord>,
    base_dir: &Path,
    rpc_url: &str,
    resolve_alts: bool,
) -> Result<BracketBatchResult> {
    let a1 = &records[0];
    let t = &records[1];
    let a2 = &records[2];

    let mut reasons = Vec::new();
    let complete = records
        .iter()
        .all(|r| r.http_status == 200 && r.transaction_present && r.parse_ok);
    if !complete {
        reasons.push("incomplete: missing HTTP 200, transaction, or parse".into());
    }

    let (exact_route_stable, class_a1_t, class_t_a2, class_a1_a2) = match (
        &a1.route_fingerprint,
        &t.route_fingerprint,
        &a2.route_fingerprint,
    ) {
        (Some(fa), Some(ft), Some(fb)) => {
            let c1 = classify_route_pair(fa, ft);
            let c2 = classify_route_pair(ft, fb);
            let c3 = classify_route_pair(fa, fb);
            let exact = c1 == RouteStabilityClass::ExactRouteStable
                && c2 == RouteStabilityClass::ExactRouteStable
                && c3 == RouteStabilityClass::ExactRouteStable;
            if !exact {
                reasons.push(format!(
                    "route not exact-stable (A1-T={}, T-A2={}, A1-A2={})",
                    c1.as_str(),
                    c2.as_str(),
                    c3.as_str()
                ));
            }
            (
                exact,
                Some(c1.as_str().to_string()),
                Some(c2.as_str().to_string()),
                Some(c3.as_str().to_string()),
            )
        }
        _ => {
            reasons.push("route fingerprint missing".into());
            (false, None, None, None)
        }
    };

    if a1.program_set != t.program_set || t.program_set != a2.program_set {
        reasons.push("program set mismatch".into());
    }
    if a1.transaction_version != t.transaction_version
        || t.transaction_version != a2.transaction_version
    {
        reasons.push("transaction version mismatch".into());
    }
    if records.iter().any(|r| r.alt_resolve_ok == Some(false)) {
        reasons.push("ALT resolution failure".into());
    }

    let mut topology_stable_a1_t = None;
    let mut topology_stable_t_a2 = None;
    let mut raw_tx_diff_a1_t = None;
    let mut canonical_tx_diff_a1_t = None;
    let mut payload_diff_a1_t = None;
    let mut candidate_encodings = Vec::new();

    if let (Some(a1_b64_path), Some(t_b64_path), Some(a2_b64_path)) = (
        &a1.transaction_b64_path,
        &t.transaction_b64_path,
        &a2.transaction_b64_path,
    ) {
        let a1_b64 = fs::read_to_string(base_dir.join(a1_b64_path))?;
        let t_b64 = fs::read_to_string(base_dir.join(t_b64_path))?;
        let a2_b64 = fs::read_to_string(base_dir.join(a2_b64_path))?;

        raw_tx_diff_a1_t = diff_raw_transactions(&a1_b64, &t_b64).ok();
        if let Ok(d) = diff_canonical_transactions(&a1_b64, &t_b64, rpc_url, resolve_alts).await {
            topology_stable_a1_t = Some(d.topology_stable);
            if !d.topology_stable {
                reasons.push("topology not stable A1 vs T".into());
            }
            canonical_tx_diff_a1_t = Some(d);
        } else {
            reasons.push("canonical diff A1 vs T failed".into());
        }
        if let Ok(d) = diff_canonical_transactions(&t_b64, &a2_b64, rpc_url, resolve_alts).await {
            topology_stable_t_a2 = Some(d.topology_stable);
            if !d.topology_stable {
                reasons.push("topology not stable T vs A2".into());
            }
        } else {
            reasons.push("canonical diff T vs A2 failed".into());
        }

        let eligible_for_instruction_diff = complete
            && exact_route_stable
            && a1.program_set == t.program_set
            && t.program_set == a2.program_set
            && a1.transaction_version == t.transaction_version
            && t.transaction_version == a2.transaction_version
            && records.iter().all(|r| r.alt_resolve_ok != Some(false))
            && topology_stable_a1_t == Some(true)
            && topology_stable_t_a2 == Some(true);

        if eligible_for_instruction_diff {
            payload_diff_a1_t = diff_instruction_payloads(&a1_b64, &t_b64).ok();
            let delta_owned = match (&t.out_amount, &t.other_amount_threshold) {
                (Some(out), Some(th)) => match (out.parse::<u128>(), th.parse::<u128>()) {
                    (Ok(o), Ok(h)) => Some(o.saturating_sub(h).to_string()),
                    _ => None,
                },
                _ => None,
            };
            let mut owned_amounts: Vec<(String, String)> = Vec::new();
            if let Some(th) = &t.other_amount_threshold {
                owned_amounts.push(("otherAmountThreshold".into(), th.clone()));
            }
            if let Some(out) = &t.out_amount {
                owned_amounts.push(("outAmount".into(), out.clone()));
            }
            if let Some(d) = delta_owned {
                owned_amounts.push(("outAmount_minus_threshold".into(), d));
            }
            let refs: Vec<(&str, &str)> = owned_amounts
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            candidate_encodings = search_candidate_encodings_in_payload(
                &format!("batch{batch_index}_T{treatment}"),
                &a1_b64,
                &t_b64,
                &refs,
            )
            .unwrap_or_default();
            reasons.clear();
        } else if reasons.is_empty() {
            reasons.push("eligibility gate not satisfied".into());
        }

        return Ok(BracketBatchResult {
            batch_index,
            treatment_slippage_bps: treatment,
            pattern: pattern.into(),
            requests: records,
            complete,
            exact_route_stable,
            route_class_a1_t: class_a1_t,
            route_class_t_a2: class_t_a2,
            route_class_a1_a2: class_a1_a2,
            topology_stable_a1_t,
            topology_stable_t_a2,
            eligible_for_instruction_diff,
            ineligibility_reasons: reasons,
            raw_tx_diff_a1_t,
            canonical_tx_diff_a1_t,
            payload_diff_a1_t,
            candidate_encodings,
        });
    }

    reasons.push("missing transaction bytes for comparison".into());
    Ok(BracketBatchResult {
        batch_index,
        treatment_slippage_bps: treatment,
        pattern: pattern.into(),
        requests: records,
        complete,
        exact_route_stable,
        route_class_a1_t: class_a1_t,
        route_class_t_a2: class_t_a2,
        route_class_a1_a2: class_a1_a2,
        topology_stable_a1_t,
        topology_stable_t_a2,
        eligible_for_instruction_diff: false,
        ineligibility_reasons: reasons,
        raw_tx_diff_a1_t,
        canonical_tx_diff_a1_t,
        payload_diff_a1_t,
        candidate_encodings,
    })
}

fn render_bracket_markdown(report: &BracketExperimentReport) -> String {
    let mut md = String::new();
    md.push_str("# Bracketed route-stability experiment\n\n");
    md.push_str(&format!(
        "Experiment `{}` · `{}`\n\n",
        report.experiment_id, report.endpoint
    ));
    md.push_str(&format!(
        "Attempted batches: {} · requests: {} · complete: {} · exact-route-stable: {} · topology-stable: {} · eligible instruction-diff: {}\n\n",
        report.attempted_batches,
        report.total_requests,
        report.complete_batches,
        report.exact_route_stable_batches,
        report.topology_stable_batches,
        report.eligible_instruction_diff_batches
    ));
    md.push_str("## Route fingerprint definition (frozen)\n\n");
    md.push_str(&format!("{}\n\n", report.route_fingerprint_definition));
    md.push_str("## Treatment split\n\n");
    for (k, v) in &report.treatment_split {
        md.push_str(&format!("- treatment {k} bps: {v} batches\n"));
    }
    md.push_str("\n## Batches\n\n");
    for b in &report.batches {
        md.push_str(&format!(
            "### Batch {} (`{}`)\n\n- complete: {}\n- exact_route_stable: {}\n- topology A1-T / T-A2: {:?} / {:?}\n- eligible: {}\n- reasons: {}\n\n",
            b.batch_index,
            b.pattern,
            b.complete,
            b.exact_route_stable,
            b.topology_stable_a1_t,
            b.topology_stable_t_a2,
            b.eligible_for_instruction_diff,
            if b.ineligibility_reasons.is_empty() {
                "none".into()
            } else {
                b.ineligibility_reasons.join("; ")
            }
        ));
        if let Some(p) = &b.payload_diff_a1_t {
            md.push_str(&format!(
                "- payload difference A1 vs T: {}\n",
                p.any_payload_difference
            ));
        }
        if !b.candidate_encodings.is_empty() {
            md.push_str("- candidate encodings:\n");
            for h in &b.candidate_encodings {
                md.push_str(&format!(
                    "  - {} ix {} off {} width {} value {} ({})\n",
                    h.treatment_value,
                    h.instruction_index,
                    h.byte_offset,
                    h.width_bytes,
                    h.matched_value,
                    h.classification
                ));
            }
        }
        md.push('\n');
    }
    md.push_str("## Evidence ceiling\n\n");
    for e in &report.evidence_ceiling {
        md.push_str(&format!("- {e}\n"));
    }
    md.push_str(&format!("\nNotes: {}\n", report.notes));
    md
}

fn resolve_against(base: &Path, maybe_relative: &str) -> PathBuf {
    let p = PathBuf::from(maybe_relative);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

fn relativize(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
