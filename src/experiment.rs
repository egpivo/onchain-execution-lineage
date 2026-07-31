//! Bounded read-only provider experiments.
//!
//! Not a scheduler, not a trading simulator. A finite, declared set of
//! provider requests produces mechanism evidence (quote fields, optional
//! unsigned transactions, lineage diffs) — never fills, balances, or PnL.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::DEV_QUOTE_ENDPOINT;
use crate::artifact::sha256_bytes;
use crate::diff::{diff_bundles, LineageDiff};
use crate::evidence::LINEAGE_SCHEMA_VERSION;
use crate::lineage_model::{CaptureMetadata, LineageBundle};
use crate::pairs;
use crate::providers;
use crate::transaction;

pub const EXPERIMENT_SCHEMA_VERSION: &str = "1.0.0";
const MAX_REQUESTS_HARD_CAP: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentMode {
    Fixture,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreatmentVariable {
    PlatformFeeBps,
    SlippageBps,
    InputAmount,
}

impl TreatmentVariable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlatformFeeBps => "platform_fee_bps",
            Self::SlippageBps => "slippage_bps",
            Self::InputAmount => "input_amount",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentManifest {
    pub schema_version: String,
    pub experiment_id: String,
    pub provider: String,
    pub endpoint_type: String,
    pub mode: ExperimentMode,
    pub pair: String,
    pub input_amount: String,
    pub fixed_parameters: BTreeMap<String, Value>,
    pub treatment_variable: TreatmentVariable,
    pub treatment_values: Vec<Value>,
    pub baseline_value: Value,
    pub maximum_request_count: usize,
    pub output_path: String,
    #[serde(default)]
    pub fixture_dir: Option<String>,
    #[serde(default)]
    pub endpoint_hostname: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreatmentRun {
    pub treatment_value: String,
    pub is_baseline: bool,
    pub raw_path: String,
    pub raw_sha256: String,
    pub lineage_path: String,
    /// UTC timestamp when this treatment was recorded (local wall clock).
    pub response_timestamp_utc: String,
    /// Final route-leg `outAmount` (last leg); within-response gross side.
    pub route_leg_gross_output: Option<String>,
    pub platform_fee_amount: Option<String>,
    /// Top-level quote `outAmount` (net quote field when a fee is present).
    pub net_out_amount: Option<String>,
    /// Whether `route_leg_gross_output - platform_fee_amount == net_out_amount`.
    pub within_response_accounting_identity: Option<bool>,
    pub other_amount_threshold: Option<String>,
    /// `(net_out - threshold) * 10000 / net_out` when both parse as integers.
    pub implied_threshold_distance_bps: Option<u64>,
    pub route_venue: Option<String>,
    pub route_market_key: Option<String>,
    pub transaction_present: bool,
    pub transaction_byte_length: Option<usize>,
    /// `"quote-stage only"` when unsigned transaction bytes are absent.
    pub transaction_presence_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MechanismBuckets {
    pub changed: Vec<String>,
    pub unchanged: Vec<String>,
    pub candidate_mechanism: Vec<String>,
    pub unresolved: Vec<String>,
    pub not_observable_without_settlement: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentReport {
    pub schema_version: String,
    pub experiment_id: String,
    pub treatment_variable: TreatmentVariable,
    pub baseline_value: String,
    pub runs: Vec<TreatmentRun>,
    pub diffs_vs_baseline: BTreeMap<String, LineageDiff>,
    pub mechanism: MechanismBuckets,
    pub notes: String,
}

impl ExperimentManifest {
    pub fn load_path(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read experiment manifest {}", path.display()))?;
        let m: Self = serde_json::from_str(&text).context("failed to parse experiment manifest")?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EXPERIMENT_SCHEMA_VERSION {
            bail!(
                "unsupported experiment schema_version '{}'; supported '{}'",
                self.schema_version,
                EXPERIMENT_SCHEMA_VERSION
            );
        }
        if self.provider != "dflow" {
            bail!(
                "unknown provider '{}' (public experiments support dflow only)",
                self.provider
            );
        }
        if self.endpoint_type != "developer" {
            bail!(
                "unsafe or unsupported endpoint_type '{}' (public experiments: developer only)",
                self.endpoint_type
            );
        }
        if self.maximum_request_count == 0 || self.maximum_request_count > MAX_REQUESTS_HARD_CAP {
            bail!(
                "maximum_request_count must be 1..={MAX_REQUESTS_HARD_CAP}, got {}",
                self.maximum_request_count
            );
        }
        if self.treatment_values.is_empty() {
            bail!("treatment_values must be non-empty");
        }
        if self.treatment_values.len() > self.maximum_request_count {
            bail!(
                "treatment_values length {} exceeds maximum_request_count {}",
                self.treatment_values.len(),
                self.maximum_request_count
            );
        }
        let baseline = value_key(&self.baseline_value);
        if !self
            .treatment_values
            .iter()
            .any(|v| value_key(v) == baseline)
        {
            bail!("baseline_value must appear in treatment_values");
        }
        if matches!(self.mode, ExperimentMode::Fixture) && self.fixture_dir.is_none() {
            bail!("fixture mode requires fixture_dir");
        }
        if let Some(host) = &self.endpoint_hostname {
            if !(host == "dev-quote-api.dflow.net" || host.starts_with("127.0.0.1")) {
                bail!("unsafe endpoint_hostname '{host}'");
            }
        }
        let blob = serde_json::to_string(self)?;
        for needle in [
            "authorization:",
            "bearer ",
            "cookie:",
            "private_key",
            "api_key=",
        ] {
            if blob.to_lowercase().contains(needle) {
                bail!("manifest appears to contain secret material ({needle})");
            }
        }
        Ok(())
    }
}

pub async fn run_experiment(manifest_path: &Path, base_dir: &Path) -> Result<ExperimentReport> {
    let manifest = ExperimentManifest::load_path(manifest_path)?;
    let out_dir = resolve_against(base_dir, &manifest.output_path);
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(out_dir.join("raw"))?;
    fs::create_dir_all(out_dir.join("lineage"))?;

    let (input_mint, output_mint, _) = pairs::resolve_pair(&manifest.pair)?;
    let mut runs = Vec::new();
    let mut bundles: BTreeMap<String, LineageBundle> = BTreeMap::new();

    for value in &manifest.treatment_values {
        let key = value_key(value);
        let raw_text = load_or_fetch_response(&manifest, base_dir, value, input_mint, output_mint)
            .await
            .with_context(|| format!("treatment value {key}"))?;

        let raw_path = out_dir.join("raw").join(format!("{key}.json"));
        fs::write(&raw_path, &raw_text)?;
        let hash = sha256_bytes(raw_text.as_bytes());

        let json: Value = serde_json::from_str(&raw_text)
            .with_context(|| format!("treatment {key} response is not JSON"))?;

        let captured_at = chrono::Utc::now().to_rfc3339();
        let mut bundle = LineageBundle::new(CaptureMetadata {
            artifact_id: format!("{}_{key}", manifest.experiment_id),
            provider: manifest.provider.clone(),
            surface: format!("experiment:{}", manifest.experiment_id),
            captured_at_utc: captured_at.clone(),
            pair: manifest.pair.clone(),
        });
        providers::normalize_provider_json(&json, &mut bundle)?;

        let mut tx_len = None;
        if let Some(b64) = json.get("transaction").and_then(|t| t.as_str()) {
            if let Ok(decoded) = transaction::decode_base64_transaction(b64) {
                apply_tx_fields(&mut bundle, &decoded, b64.len());
                tx_len = Some(b64.len());
            } else {
                bundle.push_unresolved(
                    "transaction",
                    "transaction field present but failed to decode",
                );
            }
        }

        let lineage_path = out_dir.join("lineage").join(format!("{key}.json"));
        fs::write(&lineage_path, bundle.to_canonical_json()?)?;

        let is_baseline = key == value_key(&manifest.baseline_value);
        let obs = observe_response_fields(&json, &bundle);
        let tx_present = bundle.transaction_construction.present;
        runs.push(TreatmentRun {
            treatment_value: key.clone(),
            is_baseline,
            raw_path: relativize(base_dir, &raw_path),
            raw_sha256: hash,
            lineage_path: relativize(base_dir, &lineage_path),
            response_timestamp_utc: captured_at,
            route_leg_gross_output: obs.route_leg_gross_output,
            platform_fee_amount: obs.platform_fee_amount,
            net_out_amount: obs.net_out_amount,
            within_response_accounting_identity: obs.within_response_accounting_identity,
            other_amount_threshold: obs.other_amount_threshold,
            implied_threshold_distance_bps: obs.implied_threshold_distance_bps,
            route_venue: obs.route_venue,
            route_market_key: obs.route_market_key,
            transaction_present: tx_present,
            transaction_byte_length: tx_len,
            transaction_presence_note: if tx_present {
                "unsigned transaction bytes present".into()
            } else {
                "quote-stage only".into()
            },
        });
        bundles.insert(key, bundle);
    }

    let baseline_key = value_key(&manifest.baseline_value);
    let baseline = bundles
        .get(&baseline_key)
        .context("baseline bundle missing after runs")?;

    let mut diffs = BTreeMap::new();
    for (key, bundle) in &bundles {
        if key == &baseline_key {
            continue;
        }
        diffs.insert(key.clone(), diff_bundles(baseline, bundle));
    }

    let mechanism = build_mechanism_buckets(&manifest, baseline, &bundles, &runs);
    let report = ExperimentReport {
        schema_version: LINEAGE_SCHEMA_VERSION.to_string(),
        experiment_id: manifest.experiment_id.clone(),
        treatment_variable: manifest.treatment_variable,
        baseline_value: baseline_key,
        runs,
        diffs_vs_baseline: diffs,
        mechanism,
        notes: manifest.notes.clone(),
    };

    let report_json = out_dir.join("experiment_report.json");
    let report_md = out_dir.join("experiment_report.md");
    fs::write(&report_json, serde_json::to_string_pretty(&report)?)?;
    fs::write(&report_md, render_mechanism_markdown(&report))?;
    Ok(report)
}

async fn load_or_fetch_response(
    manifest: &ExperimentManifest,
    base_dir: &Path,
    value: &Value,
    input_mint: &str,
    output_mint: &str,
) -> Result<String> {
    match manifest.mode {
        ExperimentMode::Fixture => {
            let dir = manifest
                .fixture_dir
                .as_ref()
                .context("fixture_dir required")?;
            let path = resolve_against(base_dir, dir).join(format!("{}.json", value_key(value)));
            fs::read_to_string(&path).with_context(|| format!("missing fixture {}", path.display()))
        }
        ExperimentMode::Live => {
            let (amount, slippage, fee_bps) = resolve_request_params(manifest, value)?;
            let endpoint = match &manifest.endpoint_hostname {
                Some(host) if host.starts_with("127.0.0.1") => {
                    format!("http://{host}/quote")
                }
                Some(host) if host == "dev-quote-api.dflow.net" => DEV_QUOTE_ENDPOINT.to_string(),
                None => DEV_QUOTE_ENDPOINT.to_string(),
                Some(host) => bail!("unsafe endpoint_hostname '{host}'"),
            };
            let mut url = format!(
                "{endpoint}?inputMint={input_mint}&outputMint={output_mint}&amount={amount}&slippageBps={slippage}"
            );
            if let Some(fee) = fee_bps {
                url.push_str(&format!("&platformFeeBps={fee}"));
            }
            let client = reqwest::Client::new();
            let resp = client.get(&url).send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                bail!("live quote failed {status}: {text}");
            }
            Ok(text)
        }
    }
}

fn resolve_request_params(
    manifest: &ExperimentManifest,
    value: &Value,
) -> Result<(u64, u32, Option<u32>)> {
    let mut amount: u64 = manifest
        .input_amount
        .parse()
        .context("input_amount must be u64 string")?;
    let mut slippage = manifest
        .fixed_parameters
        .get("slippage_bps")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as u32;
    let mut fee = manifest
        .fixed_parameters
        .get("platform_fee_bps")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    match manifest.treatment_variable {
        TreatmentVariable::PlatformFeeBps => {
            fee = Some(as_u32(value)?);
        }
        TreatmentVariable::SlippageBps => {
            slippage = as_u32(value)?;
        }
        TreatmentVariable::InputAmount => {
            amount = as_u64(value)?;
        }
    }
    Ok((amount, slippage, fee))
}

fn apply_tx_fields(
    bundle: &mut LineageBundle,
    decoded: &crate::transaction::DecodedTransaction,
    b64_len: usize,
) {
    bundle.transaction_construction.present = true;
    bundle.transaction_construction.encoding = Some("base64".into());
    bundle.transaction_construction.transaction_type = Some(decoded.transaction_type.clone());
    bundle.transaction_construction.fee_payer = decoded.fee_payer.clone();
    bundle.transaction_construction.num_instructions = Some(decoded.instructions.len());
    bundle.transaction_construction.num_lookup_tables =
        Some(decoded.address_lookup_table_references.len());
    bundle.transaction_construction.program_ids = decoded
        .instructions
        .iter()
        .map(|i| i.program_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    bundle.transaction_construction.program_labels = decoded
        .instructions
        .iter()
        .map(|i| i.program_label.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    bundle.execution.invoked_programs = bundle.transaction_construction.program_ids.clone();
    bundle.execution.unknown_program_ids = decoded.unknown_program_ids.clone();
    bundle.execution.compute_budget_present = decoded
        .instructions
        .iter()
        .any(|i| i.program_label == "compute_budget");
    bundle
        .raw_extensions
        .insert("transaction_b64_len".into(), json!(b64_len));
    bundle.decoded_transaction = Some(decoded.clone());
}

fn build_mechanism_buckets(
    manifest: &ExperimentManifest,
    baseline: &LineageBundle,
    bundles: &BTreeMap<String, LineageBundle>,
    runs: &[TreatmentRun],
) -> MechanismBuckets {
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();

    let base_out = baseline.quote.out_amount.clone().unwrap_or_default();
    if bundles
        .values()
        .any(|b| b.quote.out_amount.clone().unwrap_or_default() != base_out)
    {
        match manifest.treatment_variable {
            TreatmentVariable::PlatformFeeBps => changed.push(
                "top-level outAmount differed across sequential requests (cross-request quote movement; not attributed solely to fee)"
                    .into(),
            ),
            _ => changed.push("top-level outAmount".into()),
        }
    } else {
        unchanged.push("top-level outAmount: unchanged in this run".into());
    }

    let base_fee = format!("{:?}", baseline.fee.platform_fee_visible);
    if bundles
        .values()
        .any(|b| format!("{:?}", b.fee.platform_fee_visible) != base_fee)
    {
        changed.push("platformFee".into());
    } else {
        unchanged.push("platformFee: unchanged in this run".into());
    }

    let base_min = baseline.quote.min_out_amount.clone().unwrap_or_default();
    if bundles
        .values()
        .any(|b| b.quote.min_out_amount.clone().unwrap_or_default() != base_min)
    {
        changed.push("otherAmountThreshold / minOutAmount".into());
    } else {
        unchanged.push("otherAmountThreshold / minOutAmount: unchanged in this run".into());
    }

    let base_topo = route_topology_signature(baseline);
    if bundles
        .values()
        .any(|b| route_topology_signature(b) != base_topo)
    {
        changed.push("route venue/marketKey".into());
    } else {
        unchanged.push("route venue/marketKey: unchanged in this run".into());
    }

    let accounting: Vec<_> = runs
        .iter()
        .filter_map(|r| r.within_response_accounting_identity)
        .collect();
    if !accounting.is_empty() {
        if accounting.iter().all(|v| *v) {
            unchanged.push(
                "within-response accounting identity: holds for every treatment in this run".into(),
            );
        } else if accounting.iter().any(|v| *v) {
            changed.push(
                "within-response accounting identity: holds for some treatments, not all".into(),
            );
        } else {
            changed.push(
                "within-response accounting identity: does not hold for observed treatments".into(),
            );
        }
    }

    let base_tx = baseline.transaction_construction.present;
    if bundles
        .values()
        .any(|b| b.transaction_construction.present != base_tx)
    {
        changed.push("unsigned transaction presence".into());
    } else if !base_tx {
        unchanged.push("unsigned transaction presence: quote-stage only".into());
    } else {
        unchanged.push("unsigned transaction presence: unchanged in this run".into());
    }

    let base_programs = &baseline.transaction_construction.program_labels;
    if bundles
        .values()
        .any(|b| &b.transaction_construction.program_labels != base_programs)
    {
        changed.push("program set".into());
    } else if !base_programs.is_empty() {
        unchanged.push("program set: unchanged in this run".into());
    }

    let base_ix = baseline.transaction_construction.num_instructions;
    if bundles
        .values()
        .any(|b| b.transaction_construction.num_instructions != base_ix)
    {
        changed.push("instruction count".into());
    } else if base_ix.is_some() {
        unchanged.push("instruction count: unchanged in this run".into());
    }

    let base_alt = baseline.transaction_construction.num_lookup_tables;
    if bundles
        .values()
        .any(|b| b.transaction_construction.num_lookup_tables != base_alt)
    {
        changed.push("ALT usage".into());
    } else if base_alt.is_some() {
        unchanged.push("ALT usage: unchanged in this run".into());
    }

    let mut hash_changed = false;
    for b in bundles.values() {
        if let (Some(bd), Some(td)) = (&baseline.decoded_transaction, &b.decoded_transaction) {
            let bh: Vec<_> = bd.instructions.iter().map(|i| &i.data_sha256).collect();
            let th: Vec<_> = td.instructions.iter().map(|i| &i.data_sha256).collect();
            if bh != th {
                hash_changed = true;
            }
        }
    }
    if hash_changed {
        changed.push("instruction data hashes".into());
    }

    let mut candidate = Vec::new();
    match manifest.treatment_variable {
        TreatmentVariable::PlatformFeeBps => {
            candidate.push(
                "within-response: platformFee.amount relates route-leg gross output to top-level net outAmount when the accounting identity holds"
                    .into(),
            );
            candidate.push(
                "cross-request: do not attribute the full baseline→treatment outAmount delta to the fee alone; live quotes are sequential and market state may move"
                    .into(),
            );
            candidate.push(
                "quote-stage only when unsigned transaction bytes are absent — fee mint/account encoding into instructions is not observable here"
                    .into(),
            );
        }
        TreatmentVariable::SlippageBps => {
            candidate.push(
                "slippageBps may be reflected in otherAmountThreshold / minOutAmount relative to top-level outAmount (observational)"
                    .into(),
            );
            candidate.push(
                "cross-request: outAmount movement across sequential quotes is not attributed solely to slippageBps"
                    .into(),
            );
        }
        TreatmentVariable::InputAmount => {
            candidate.push(
                "route venue/marketKey may vary with size when the provider re-routes (observational)"
                    .into(),
            );
        }
    }

    let mut unresolved =
        vec!["whether the treatment affects private router ranking beyond venue/marketKey".into()];
    if matches!(
        manifest.treatment_variable,
        TreatmentVariable::PlatformFeeBps
    ) {
        unresolved.push("fee mint / fee account identity when absent from quote JSON".into());
    }
    if runs.iter().all(|r| !r.transaction_present) {
        unresolved.push(
            "instruction-level encoding of fee or slippage (requires unsigned transaction bytes)"
                .into(),
        );
    }

    MechanismBuckets {
        changed,
        unchanged,
        candidate_mechanism: candidate,
        unresolved,
        not_observable_without_settlement: vec![
            "realized output".into(),
            "delivery path".into(),
            "landed execution".into(),
        ],
    }
}

struct ResponseObservation {
    route_leg_gross_output: Option<String>,
    platform_fee_amount: Option<String>,
    net_out_amount: Option<String>,
    within_response_accounting_identity: Option<bool>,
    other_amount_threshold: Option<String>,
    implied_threshold_distance_bps: Option<u64>,
    route_venue: Option<String>,
    route_market_key: Option<String>,
}

fn observe_response_fields(json: &Value, bundle: &LineageBundle) -> ResponseObservation {
    let net_out_amount =
        take_amount_string(json, "outAmount").or_else(|| bundle.quote.out_amount.clone());
    let other_amount_threshold = take_amount_string(json, "otherAmountThreshold")
        .or_else(|| take_amount_string(json, "minOutAmount"))
        .or_else(|| bundle.quote.min_out_amount.clone());

    let (route_venue, route_market_key, route_leg_gross_output) =
        if let Some(last) = bundle.route.legs.last() {
            (
                Some(last.venue_or_label.clone()),
                last.market_key.clone(),
                last.out_amount.clone(),
            )
        } else if let Some(legs) = json.get("routePlan").and_then(|r| r.as_array()) {
            match legs.last() {
                Some(leg) => (
                    take_string_field(leg, "venue"),
                    take_string_field(leg, "marketKey"),
                    take_amount_string(leg, "outAmount"),
                ),
                None => (None, None, None),
            }
        } else {
            (None, None, None)
        };

    let platform_fee_amount = match json.get("platformFee") {
        None | Some(Value::Null) => Some("0".into()),
        // Object present without amount → unknown for accounting (None).
        Some(fee) => take_amount_string(fee, "amount"),
    };

    let within_response_accounting_identity = match (
        &route_leg_gross_output,
        &platform_fee_amount,
        &net_out_amount,
    ) {
        (Some(g), Some(f), Some(n)) => Some(within_response_accounting_holds(g, f, n)),
        _ => None,
    };

    let implied_threshold_distance_bps = match (&net_out_amount, &other_amount_threshold) {
        (Some(out), Some(th)) => implied_threshold_distance_bps(out, th),
        _ => None,
    };

    ResponseObservation {
        route_leg_gross_output,
        platform_fee_amount,
        net_out_amount,
        within_response_accounting_identity,
        other_amount_threshold,
        implied_threshold_distance_bps,
        route_venue,
        route_market_key,
    }
}

fn within_response_accounting_holds(gross: &str, fee: &str, net: &str) -> bool {
    match (
        gross.parse::<u128>(),
        fee.parse::<u128>(),
        net.parse::<u128>(),
    ) {
        (Ok(g), Ok(f), Ok(n)) => g.saturating_sub(f) == n,
        _ => false,
    }
}

fn implied_threshold_distance_bps(out_amount: &str, threshold: &str) -> Option<u64> {
    let out = out_amount.parse::<u128>().ok()?;
    let th = threshold.parse::<u128>().ok()?;
    if out == 0 {
        return None;
    }
    let bps = out
        .saturating_sub(th)
        .saturating_mul(10_000)
        .checked_div(out)?;
    u64::try_from(bps).ok()
}

fn take_amount_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn take_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Venue + marketKey only — excludes leg amounts so fee/slippage quote movement
/// is not mislabeled as a route topology change.
fn route_topology_signature(b: &LineageBundle) -> String {
    b.route
        .legs
        .iter()
        .map(|l| {
            format!(
                "{}:{}",
                l.venue_or_label,
                l.market_key.clone().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn render_mechanism_markdown(report: &ExperimentReport) -> String {
    let mut md = String::new();
    md.push_str("# Experiment mechanism report\n\n");
    md.push_str(&format!(
        "Experiment `{}` · treatment `{}` · baseline `{}`\n\n",
        report.experiment_id,
        report.treatment_variable.as_str(),
        report.baseline_value
    ));
    md.push_str(
        "> Controlled experiments are not simulated fills. No balances, PnL, or landed execution are claimed.\n\n",
    );
    if matches!(report.treatment_variable, TreatmentVariable::PlatformFeeBps) {
        md.push_str(
            "> Fee experiments distinguish **within-response accounting identity** \
             (`route-leg gross − platformFee.amount == top-level outAmount`) from \
             **cross-request quote movement**. Do not attribute the full baseline delta \
             to the fee; live requests are sequential and market state may move.\n\n",
        );
    }

    md.push_str("## Per-treatment response observations\n\n");
    md.push_str(
        "| value | baseline | timestamp (UTC) | route-leg gross | platformFee.amount | net outAmount | accounting identity | threshold | implied threshold bps | venue | marketKey | transaction |\n",
    );
    md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in &report.runs {
        md.push_str(&format!(
            "| {} | {} | `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.treatment_value,
            r.is_baseline,
            r.response_timestamp_utc,
            opt_cell(&r.route_leg_gross_output),
            opt_cell(&r.platform_fee_amount),
            opt_cell(&r.net_out_amount),
            match r.within_response_accounting_identity {
                Some(true) => "holds",
                Some(false) => "does not hold",
                None => "n/a",
            },
            opt_cell(&r.other_amount_threshold),
            r.implied_threshold_distance_bps
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into()),
            opt_cell(&r.route_venue),
            opt_cell(&r.route_market_key),
            r.transaction_presence_note,
        ));
    }
    md.push('\n');

    md.push_str("## Changed\n\n");
    append_list(&mut md, &report.mechanism.changed);
    md.push_str("## Unchanged\n\n");
    append_list(&mut md, &report.mechanism.unchanged);
    md.push_str("## Candidate mechanism\n\n");
    append_list(&mut md, &report.mechanism.candidate_mechanism);
    md.push_str("## Unresolved\n\n");
    append_list(&mut md, &report.mechanism.unresolved);
    md.push_str("## Not observable without settlement\n\n");
    append_list(&mut md, &report.mechanism.not_observable_without_settlement);

    md.push_str("## Runs (artifact index)\n\n");
    md.push_str("| value | baseline | tx present | note | raw sha256 |\n|---|---|---|---|---|\n");
    for r in &report.runs {
        md.push_str(&format!(
            "| {} | {} | {} | {} | `{}` |\n",
            r.treatment_value,
            r.is_baseline,
            r.transaction_present,
            r.transaction_presence_note,
            r.raw_sha256
        ));
    }
    md.push('\n');
    md.push_str(&format!("Notes: {}\n", report.notes));
    md
}

fn opt_cell(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "n/a".into())
}

fn append_list(md: &mut String, items: &[String]) {
    if items.is_empty() {
        md.push_str("_None._\n\n");
        return;
    }
    for i in items {
        md.push_str(&format!("- {i}\n"));
    }
    md.push('\n');
}

fn value_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn as_u32(v: &Value) -> Result<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .map(|x| x as u32)
            .context("expected unsigned number"),
        Value::String(s) => s.parse().context("expected u32 string"),
        _ => bail!("expected number or string"),
    }
}

fn as_u64(v: &Value) -> Result<u64> {
    match v {
        Value::Number(n) => n.as_u64().context("expected unsigned number"),
        Value::String(s) => s.parse().context("expected u64 string"),
        _ => bail!("expected number or string"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ExperimentManifest {
        ExperimentManifest {
            schema_version: EXPERIMENT_SCHEMA_VERSION.into(),
            experiment_id: "fee_injection_synthetic".into(),
            provider: "dflow".into(),
            endpoint_type: "developer".into(),
            mode: ExperimentMode::Fixture,
            pair: "USDC/SOL".into(),
            input_amount: "1000000000".into(),
            fixed_parameters: BTreeMap::from([("slippage_bps".into(), json!(50))]),
            treatment_variable: TreatmentVariable::PlatformFeeBps,
            treatment_values: vec![json!(0), json!(2), json!(10)],
            baseline_value: json!(0),
            maximum_request_count: 3,
            output_path: "artifacts/experiments/fee_injection_synthetic".into(),
            fixture_dir: Some("tests/fixtures/experiments/fee_injection".into()),
            endpoint_hostname: Some("dev-quote-api.dflow.net".into()),
            notes: "synthetic".into(),
        }
    }

    #[test]
    fn rejects_unsupported_schema() {
        let mut m = sample_manifest();
        m.schema_version = "9.0.0".into();
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[test]
    fn rejects_missing_baseline() {
        let mut m = sample_manifest();
        m.baseline_value = json!(99);
        assert!(m.validate().unwrap_err().to_string().contains("baseline"));
    }

    #[test]
    fn rejects_unbounded_treatments() {
        let mut m = sample_manifest();
        m.maximum_request_count = 2;
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("maximum_request_count"));
    }

    #[test]
    fn rejects_secret_material() {
        let mut m = sample_manifest();
        m.notes = "Authorization: Bearer x".into();
        assert!(m.validate().unwrap_err().to_string().contains("secret"));
    }

    #[test]
    fn rejects_unknown_provider() {
        let mut m = sample_manifest();
        m.provider = "not-a-provider".into();
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unknown provider"));
    }

    #[test]
    fn within_response_accounting_identity_checks_gross_minus_fee() {
        assert!(within_response_accounting_holds(
            "1358675205",
            "271735",
            "1358403470"
        ));
        assert!(!within_response_accounting_holds(
            "1358675205",
            "271735",
            "1358675205"
        ));
    }

    #[test]
    fn implied_threshold_distance_matches_request_style_bps() {
        // 50 bps of 1_000_000 → threshold 995_000
        assert_eq!(
            implied_threshold_distance_bps("1000000", "995000"),
            Some(50)
        );
    }
}
