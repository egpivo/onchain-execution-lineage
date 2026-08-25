//! Live network integration tests.
//!
//! Default `cargo test` skips these. Run explicitly:
//!
//! ```bash
//! cargo test --test live_network -- --ignored --nocapture
//! ```

use onchain_execution_lineage::api::{fetch_quote, QuoteRequest, DEV_QUOTE_ENDPOINT};
use onchain_execution_lineage::capture::run_capture;
use onchain_execution_lineage::lookup_tables::resolve_lookup_table;
use onchain_execution_lineage::rpc::fetch_transaction_base64;
use onchain_execution_lineage::transaction::decode_base64_transaction;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

/// Real Jupiter Aggregator v6 mainnet tx used to verify the decoder
/// (same signature cited in the 2026-08-18 writeup).
const JUPITER_V6_SIG: &str =
    "3nUMNetJ1FiSfVtWMXTFbsJEUtAHK5stLtR9nLaMEYL3E69UsERpo8QXiUETwDMR2NHH9SwDSSbpApzix9P6NLaB";
const JUPITER_FEE_PAYER: &str = "53wT5Y8iDGnYmxUu4XwSxnE5E3WgchnQENuRCEzm2MHZ";

#[tokio::test]
#[ignore = "requires network; run with: cargo test --test live_network -- --ignored"]
async fn live_dev_quote_is_quote_only_surface() {
    let (parsed, raw, hash) = fetch_quote(&QuoteRequest {
        input_mint: USDC_MINT.to_string(),
        output_mint: SOL_MINT.to_string(),
        amount_atomic: 1_000_000_000,
        slippage_bps: 50,
    })
    .await
    .expect("dev-quote-api request failed");

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    assert_eq!(format!("{:x}", hasher.finalize()), hash);

    let raw_value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(
        !raw_value.as_object().unwrap().contains_key("transaction"),
        "live raw body must not contain a transaction key; got keys {:?}",
        raw_value.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert!(parsed.transaction.is_none());
    assert!(!parsed.request_id.is_empty());
    assert!(!parsed.route_plan.is_empty());
    assert_eq!(parsed.input_mint, USDC_MINT);
    assert_eq!(parsed.output_mint, SOL_MINT);
    assert!(DEV_QUOTE_ENDPOINT.contains("dev-quote-api.dflow.net"));
}

#[tokio::test]
#[ignore = "requires network; run with: cargo test --test live_network -- --ignored"]
async fn live_capture_writes_raw_parsed_and_meta() {
    let dir = std::env::temp_dir().join(format!("oel_live_capture_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let parsed_path = run_capture(
        "USDC_SOL",
        USDC_MINT,
        SOL_MINT,
        1_000_000_000,
        1000.0,
        50,
        &PathBuf::from(&dir),
    )
    .await
    .expect("live capture failed");

    let stem = parsed_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace("_parsed.json", "");
    let raw_path = dir.join(format!("{stem}_raw.json"));
    let meta_path = dir.join(format!("{stem}_meta.json"));

    assert!(raw_path.exists());
    assert!(parsed_path.exists());
    assert!(meta_path.exists());

    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(meta["had_transaction_field"], false);
    assert_eq!(meta["endpoint"], DEV_QUOTE_ENDPOINT);
    assert_eq!(meta["pair_label"], "USDC_SOL");

    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&raw_path).unwrap()).unwrap();
    assert!(!raw.as_object().unwrap().contains_key("transaction"));

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "requires network; run with: cargo test --test live_network -- --ignored"]
async fn live_fetch_decode_and_resolve_alt_on_jupiter_tx() {
    let b64 = fetch_transaction_base64(MAINNET_RPC, JUPITER_V6_SIG)
        .await
        .expect("getTransaction failed");
    let decoded = decode_base64_transaction(&b64).expect("decode failed");

    assert_eq!(decoded.transaction_type, "v0_with_alt");
    assert_eq!(decoded.fee_payer.as_deref(), Some(JUPITER_FEE_PAYER));
    assert!(
        decoded.unknown_program_ids.is_empty(),
        "unexpected unknown programs: {:?}",
        decoded.unknown_program_ids
    );
    assert!(!decoded.address_lookup_table_references.is_empty());

    let labels: Vec<_> = decoded
        .instructions
        .iter()
        .map(|ix| ix.program_label.as_str())
        .collect();
    assert!(labels.contains(&"jupiter_aggregator_v6"));
    assert!(labels.contains(&"compute_budget"));
    assert!(labels.contains(&"system_program"));

    let alt = &decoded.address_lookup_table_references[0];
    let addresses = resolve_lookup_table(MAINNET_RPC, &alt.lookup_table_account)
        .await
        .expect("ALT resolve failed");
    assert_eq!(
        addresses.len(),
        169,
        "expected 169 addresses in the cited Jupiter ALT"
    );
}
