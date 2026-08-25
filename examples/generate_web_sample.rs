//! Generates the public-safe artifacts the web viewer ships with.
//!
//! Why synthetic: the real DFlow reference lineage cannot be published. Its
//! context embeds the unsigned transaction and the requester's fee-payer
//! pubkey, which is why `artifacts/lineage/` is gitignored. So the viewer's
//! bundled sample is a synthetic order response — deterministic, no real
//! wallet, no captured data — pushed through the **real** production pipeline:
//! adapter → ExecutionContext → Solana extraction → LineageBuilder → verify.
//!
//! Nothing here computes an empirical result. It constructs an input and
//! serializes what the verifier says about it.
//!
//! The transaction payload is redacted from the written artifacts: the viewer
//! never needs the raw blob, and not shipping blobs keeps the privacy guard
//! simple and categorical.
//!
//! Run via `scripts/build_web.sh`, or directly:
//!
//! ```text
//! cargo run --example generate_web_sample
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
};

use onchain_execution_lineage::adapters::ProviderId;
use onchain_execution_lineage::checks;
use onchain_execution_lineage::evidence_extract::PUBLIC_EXTRACT_PATH;
use onchain_execution_lineage::extract::{self, ExtractInputs};

/// Public mint constants — published addresses, not capture-derived.
const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SOL: &str = "So11111111111111111111111111111111111111112";
const COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";
const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

/// Tracked, public-safe analysis artifacts the site renders. Every one has been
/// audited: no wallet pubkey, no transaction payload, no capture timestamp.
const PUBLIC_DATA_ARTIFACTS: [&str; 4] = [
    "route_stable_evidence_extract.json",
    "route_stable_batch_evidence.json",
    "route_stable_causal_model.json",
    "fee_quote_evidence.json",
];

/// Every key is derived from a fixed byte pattern, so the sample is
/// byte-identical on every machine and belongs to nobody.
fn synthetic_key(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

/// A v0 transaction shaped like a real aggregator swap:
/// compute budget, an unrecognised aggregator program whose payload carries the
/// quoted output amount as a little-endian u64, and a system transfer.
///
/// It references one address lookup table, which offline extraction cannot
/// resolve — the same UNKNOWN the real reference case produces.
fn synthetic_transaction(out_amount: u64) -> Result<String> {
    let payer = synthetic_key(0x11);
    let aggregator = synthetic_key(0x22);
    let pool = synthetic_key(0x33);
    let table_key = synthetic_key(0x44);
    let table_entry = synthetic_key(0x55);
    let recipient = synthetic_key(0x66);

    // Compute-budget: SetComputeUnitLimit (discriminator 0x02) + u32 limit.
    let mut cb_data = vec![0x02];
    cb_data.extend_from_slice(&200_000u32.to_le_bytes());
    let compute_budget = Instruction {
        program_id: COMPUTE_BUDGET.parse()?,
        accounts: vec![],
        data: cb_data,
    };

    // Aggregator payload: a plausible-looking instruction whose bytes contain
    // the quoted amount. This is what makes the candidate byte relationship
    // appear — exactly the relationship the verifier refuses to call semantic.
    let mut swap_data = vec![0x6f, 0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87];
    swap_data.extend_from_slice(&[0u8; 24]);
    swap_data.extend_from_slice(&out_amount.to_le_bytes());
    swap_data.extend_from_slice(&[0xaa; 16]);
    let swap = Instruction {
        program_id: aggregator,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(pool, false),
            AccountMeta::new_readonly(table_entry, false),
        ],
        data: swap_data,
    };

    let transfer = Instruction {
        program_id: SYSTEM_PROGRAM.parse()?,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(recipient, false),
        ],
        data: vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };

    let lookup = AddressLookupTableAccount {
        key: table_key,
        addresses: vec![table_entry, synthetic_key(0x77)],
    };
    let message = v0::Message::try_compile(
        &payer,
        &[compute_budget, swap, transfer],
        &[lookup],
        Hash::new_from_array([0x07; 32]),
    )
    .context("compile synthetic v0 message")?;

    let vtx = VersionedTransaction {
        // Unsigned: an empty signature slot, never a signature.
        signatures: vec![Signature::default()],
        message: VersionedMessage::V0(message),
    };
    Ok(STANDARD.encode(bincode::serialize(&vtx)?))
}

/// A DFlow `/order`-shaped response. Amounts satisfy the documented threshold
/// identity so the DFlow checks have something real to agree with.
fn sample_order_response(out_amount: u64, slippage_bps: u32, route_input_mint: &str) -> Value {
    let threshold = onchain_execution_lineage::checks::dflow::ceil_threshold(
        u128::from(out_amount),
        slippage_bps,
    )
    .expect("threshold fits");
    json!({
        "inputMint": USDC,
        "inAmount": "100000000",
        "outputMint": SOL,
        "outAmount": out_amount.to_string(),
        "otherAmountThreshold": threshold.to_string(),
        "minOutAmount": threshold.to_string(),
        "slippageBps": slippage_bps,
        "platformFee": { "amount": "0", "feeBps": 0, "mode": "outputMint" },
        "priceImpactPct": "0",
        "routePlan": [{
            "venue": "SyntheticAmm",
            "marketKey": synthetic_key(0x33).to_string(),
            "inputMint": route_input_mint,
            "outputMint": SOL,
            "inAmount": "100000000",
            "outAmount": out_amount.to_string(),
            "inputMintDecimals": 6,
            "outputMintDecimals": 9
        }],
        "contextSlot": 400000000u64,
        "executionMode": "sync",
        "lastValidBlockHeight": 400000100u64,
        "prioritizationFeeLamports": 20,
        "computeUnitLimit": 200000,
    })
}

/// Write pretty JSON with a trailing newline, so regenerating the samples is a
/// no-op in git rather than a one-byte diff on every file.
fn write_json<T: serde::Serialize>(path: PathBuf, value: &T) -> Result<()> {
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

/// Write context + lineage + verification for one response, with the raw
/// transaction payload redacted.
async fn write_case(out_dir: &Path, response: Value, label: &str) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    // The pipeline reads the response from a scratch path. Only a redacted
    // copy is written into web/samples, so no payload is ever bundled.
    let scratch = std::env::temp_dir().join(format!("oel_web_sample_{label}"));
    std::fs::create_dir_all(&scratch)?;
    let response_path = scratch.join("order_response.json");
    std::fs::write(&response_path, serde_json::to_string_pretty(&response)?)?;

    let result = extract::extract(ExtractInputs {
        provider: Some(ProviderId::Dflow),
        response_path: Some(&response_path),
        ..Default::default()
    })
    .await?;
    let report = checks::verify(&result.context, &result.lineage);

    let mut context: Value = serde_json::to_value(&result.context)?;
    redact_transaction_payload(&mut context);
    // The pipeline records the scratch path it actually read. Rewrite it to the
    // published location: a machine-specific temp path is both noise and a
    // determinism hazard across machines.
    rewrite_source_paths(
        &mut context,
        &format!("web/samples/{label}/order_response.json"),
    );
    let mut lineage: Value = serde_json::to_value(&result.lineage)?;
    redact_transaction_payload(&mut lineage);

    write_json(out_dir.join("context.json"), &context)?;
    write_json(out_dir.join("lineage.json"), &lineage)?;
    write_json(out_dir.join("verification.json"), &report)?;

    let mut shown_response = response.clone();
    redact_transaction_payload_key(&mut shown_response, "transaction");
    write_json(out_dir.join("order_response.json"), &shown_response)?;
    let _ = std::fs::remove_dir_all(&scratch);

    let s = &report.summary;
    println!(
        "  {label}: PASS={} FAIL={} CANDIDATE={} UNKNOWN={} NOT_APPLICABLE={}",
        s.pass, s.fail, s.candidate, s.unknown, s.not_applicable
    );
    Ok(())
}

/// Point every recorded `source_path` at the published sample location.
fn rewrite_source_paths(value: &mut Value, published: &str) {
    match value {
        Value::Object(map) => {
            if map.contains_key("source_path") {
                map.insert("source_path".to_string(), json!(published));
            }
            for v in map.values_mut() {
                rewrite_source_paths(v, published);
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|v| rewrite_source_paths(v, published)),
        _ => {}
    }
}

/// Replace one named key's value with the redaction marker.
fn redact_transaction_payload_key(value: &mut Value, key: &str) {
    if let Some(map) = value.as_object_mut() {
        if map.contains_key(key) {
            map.insert(
                key.to_string(),
                json!("<redacted: transaction payloads are not published>"),
            );
        }
    }
}

/// Drop the base64 payload wherever it appears. Presentation-only: no
/// observation is derived from it in the viewer.
fn redact_transaction_payload(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in ["transaction_b64", "payload"] {
                if map.contains_key(key) {
                    map.insert(
                        key.to_string(),
                        json!("<redacted: transaction payloads are not published>"),
                    );
                }
            }
            for v in map.values_mut() {
                redact_transaction_payload(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_transaction_payload),
        _ => {}
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let samples = root.join("web/samples");
    std::fs::create_dir_all(&samples)?;

    let out_amount = 1_373_827_780u64;
    let b64 = synthetic_transaction(out_amount)?;

    println!("generating web/samples from the production pipeline:");

    // A healthy artifact: every stage but settlement.
    let mut healthy = sample_order_response(out_amount, 50, USDC);
    healthy["transaction"] = json!(b64);
    write_case(&samples.join("dflow-order"), healthy, "dflow-order").await?;

    // A deliberately inconsistent artifact, so the viewer can show what FAIL
    // looks like: the route prices a different input mint than the quote.
    let mut mismatched = sample_order_response(out_amount, 50, SOL);
    mismatched["transaction"] = json!(b64);
    mismatched["otherAmountThreshold"] = json!("999999999");
    mismatched["minOutAmount"] = json!("111111111");
    write_case(
        &samples.join("dflow-order-mismatch"),
        mismatched,
        "dflow-order-mismatch",
    )
    .await?;

    // Tracked public artifacts, copied verbatim so the site needs no network.
    // Tests assert every copy stays byte-identical to its source.
    let data = root.join("web/data");
    std::fs::create_dir_all(&data)?;
    for name in PUBLIC_DATA_ARTIFACTS {
        let source = root.join("artifacts/analysis").join(name);
        let dest = data.join(name);
        std::fs::copy(&source, &dest)
            .with_context(|| format!("copy {} -> {}", source.display(), dest.display()))?;
        println!("  copied web/data/{name}");
    }
    // Kept alongside the samples too: the lineage viewer's bundled reference.
    std::fs::copy(
        root.join(PUBLIC_EXTRACT_PATH),
        samples.join("route_stable_evidence_extract.json"),
    )?;

    Ok(())
}
