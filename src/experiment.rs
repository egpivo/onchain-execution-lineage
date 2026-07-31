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
    pub transaction_present: bool,
    pub transaction_byte_length: Option<usize>,
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

        let mut bundle = LineageBundle::new(CaptureMetadata {
            artifact_id: format!("{}_{key}", manifest.experiment_id),
            provider: manifest.provider.clone(),
            surface: format!("experiment:{}", manifest.experiment_id),
            captured_at_utc: chrono::Utc::now().to_rfc3339(),
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
        runs.push(TreatmentRun {
            treatment_value: key.clone(),
            is_baseline,
            raw_path: relativize(base_dir, &raw_path),
            raw_sha256: hash,
            lineage_path: relativize(base_dir, &lineage_path),
            transaction_present: bundle.transaction_construction.present,
            transaction_byte_length: tx_len,
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

    let mechanism = build_mechanism_buckets(&manifest, baseline, &bundles);
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
) -> MechanismBuckets {
    let mut changed: Vec<String> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();

    let base_out = baseline.quote.out_amount.clone().unwrap_or_default();
    let outs: Vec<_> = bundles
        .values()
        .map(|b| b.quote.out_amount.clone().unwrap_or_default())
        .collect();
    if outs.iter().any(|o| o != &base_out) {
        changed.push("gross outAmount".into());
    } else {
        unchanged.push("gross outAmount".into());
    }

    let base_fee = format!("{:?}", baseline.fee.platform_fee_visible);
    if bundles
        .values()
        .any(|b| format!("{:?}", b.fee.platform_fee_visible) != base_fee)
    {
        changed.push("platformFee".into());
    } else {
        unchanged.push("platformFee".into());
    }

    let base_min = baseline.quote.min_out_amount.clone().unwrap_or_default();
    if bundles
        .values()
        .any(|b| b.quote.min_out_amount.clone().unwrap_or_default() != base_min)
    {
        changed.push("otherAmountThreshold / minOutAmount".into());
    } else {
        unchanged.push("otherAmountThreshold / minOutAmount".into());
    }

    let base_route = route_signature(baseline);
    if bundles.values().any(|b| route_signature(b) != base_route) {
        changed.push("route plan".into());
    } else {
        unchanged.push("route plan".into());
    }

    let base_tx = baseline.transaction_construction.present;
    if bundles
        .values()
        .any(|b| b.transaction_construction.present != base_tx)
    {
        changed.push("unsigned transaction presence".into());
    } else {
        unchanged.push("unsigned transaction presence".into());
    }

    let base_programs = &baseline.transaction_construction.program_labels;
    if bundles
        .values()
        .any(|b| &b.transaction_construction.program_labels != base_programs)
    {
        changed.push("program set".into());
    } else if !base_programs.is_empty() {
        unchanged.push("program set".into());
    }

    let base_ix = baseline.transaction_construction.num_instructions;
    if bundles
        .values()
        .any(|b| b.transaction_construction.num_instructions != base_ix)
    {
        changed.push("instruction count".into());
    } else if base_ix.is_some() {
        unchanged.push("instruction count".into());
    }

    let base_alt = baseline.transaction_construction.num_lookup_tables;
    if bundles
        .values()
        .any(|b| b.transaction_construction.num_lookup_tables != base_alt)
    {
        changed.push("ALT usage".into());
    } else if base_alt.is_some() {
        unchanged.push("ALT usage".into());
    }

    // Instruction data hashes from diffs / decoded txs.
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
            if changed.iter().any(|c| c.contains("platformFee")) {
                candidate.push(
                    "fee-account / platformFee field injection may be request-configured (observational)"
                        .into(),
                );
            }
            candidate.push(
                "expected net amount inferred from quote fields only — not realized fee capture"
                    .into(),
            );
        }
        TreatmentVariable::SlippageBps => {
            candidate.push(
                "slippage may be encoded in otherAmountThreshold and/or instruction data (observational)"
                    .into(),
            );
        }
        TreatmentVariable::InputAmount => {
            candidate.push(
                "route topology / program set may vary with size when the provider re-routes (observational)"
                    .into(),
            );
        }
    }

    MechanismBuckets {
        changed,
        unchanged,
        candidate_mechanism: candidate,
        unresolved: vec![
            "whether the treatment affects private router ranking".into(),
            "app-specific fee account identity without settlement linkage".into(),
        ],
        not_observable_without_settlement: vec![
            "realized output".into(),
            "delivery path".into(),
            "landed execution".into(),
        ],
    }
}

fn route_signature(b: &LineageBundle) -> String {
    b.route
        .legs
        .iter()
        .map(|l| {
            format!(
                "{}:{}:{}",
                l.venue_or_label,
                l.market_key.clone().unwrap_or_default(),
                l.out_amount.clone().unwrap_or_default()
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

    md.push_str("## Runs\n\n");
    md.push_str("| value | baseline | tx present | raw sha256 |\n|---|---|---|---|\n");
    for r in &report.runs {
        md.push_str(&format!(
            "| {} | {} | {} | `{}` |\n",
            r.treatment_value, r.is_baseline, r.transaction_present, r.raw_sha256
        ));
    }
    md.push('\n');
    md.push_str(&format!("Notes: {}\n", report.notes));
    md
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
}
