//! Orchestrates a quote capture: call the API, persist the raw response,
//! the typed/parsed response, and capture metadata -- all read-only, no
//! signing, no submission, no wallet involved at any point.

use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;

use crate::api::{fetch_quote_at, QuoteRequest, DEV_QUOTE_ENDPOINT};
use crate::models::CaptureMetadata;

pub struct CaptureArgs<'a> {
    pub pair_label: &'a str,
    pub input_mint: &'a str,
    pub output_mint: &'a str,
    pub amount_atomic: u64,
    pub amount_usd: f64,
    pub slippage_bps: u32,
    pub out_dir: &'a PathBuf,
    pub endpoint: &'a str,
}

pub async fn run_capture(
    pair_label: &str,
    input_mint: &str,
    output_mint: &str,
    amount_atomic: u64,
    amount_usd: f64,
    slippage_bps: u32,
    out_dir: &PathBuf,
) -> Result<PathBuf> {
    run_capture_with(CaptureArgs {
        pair_label,
        input_mint,
        output_mint,
        amount_atomic,
        amount_usd,
        slippage_bps,
        out_dir,
        endpoint: DEV_QUOTE_ENDPOINT,
    })
    .await
}

pub async fn run_capture_with(args: CaptureArgs<'_>) -> Result<PathBuf> {
    fs::create_dir_all(args.out_dir).context("failed to create captures directory")?;

    let req = QuoteRequest {
        input_mint: args.input_mint.to_string(),
        output_mint: args.output_mint.to_string(),
        amount_atomic: args.amount_atomic,
        slippage_bps: args.slippage_bps,
    };

    let (parsed, raw_text, hash) = fetch_quote_at(args.endpoint, &req).await?;
    let captured_at = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let meta = CaptureMetadata {
        captured_at_utc: captured_at.clone(),
        pair_label: args.pair_label.to_string(),
        input_amount_usd: args.amount_usd,
        slippage_bps: args.slippage_bps,
        raw_response_sha256: hash,
        had_transaction_field: parsed.transaction.is_some(),
        endpoint: args.endpoint.to_string(),
    };

    let capture_id = format!("{}_{}", args.pair_label, captured_at);
    let raw_path = args.out_dir.join(format!("{}_raw.json", capture_id));
    let parsed_path = args.out_dir.join(format!("{}_parsed.json", capture_id));
    let meta_path = args.out_dir.join(format!("{}_meta.json", capture_id));

    fs::write(&raw_path, &raw_text).context("failed to write raw response")?;
    fs::write(&parsed_path, serde_json::to_string_pretty(&parsed)?)
        .context("failed to write parsed response")?;
    fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)
        .context("failed to write capture metadata")?;

    println!("captured {} -> {}", args.pair_label, raw_path.display());
    println!("  requestId: {}", parsed.request_id);
    println!(
        "  route: {}",
        parsed
            .route_plan
            .iter()
            .map(|leg| leg.venue.clone())
            .collect::<Vec<_>>()
            .join(" -> ")
    );
    println!(
        "  transaction field present: {}",
        parsed.transaction.is_some()
    );

    Ok(parsed_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIXTURE: &str = include_str!("../tests/fixtures/dev_quote_usdc_sol_no_tx.json");

    #[tokio::test]
    async fn run_capture_with_writes_raw_parsed_and_meta() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quote"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
            .mount(&server)
            .await;

        let dir = std::env::temp_dir().join(format!("dflow_capture_unit_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let endpoint = format!("{}/quote", server.uri());
        let parsed_path = run_capture_with(CaptureArgs {
            pair_label: "USDC_SOL",
            input_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            output_mint: "So11111111111111111111111111111111111111112",
            amount_atomic: 1_000_000_000,
            amount_usd: 1000.0,
            slippage_bps: 50,
            out_dir: &dir,
            endpoint: &endpoint,
        })
        .await
        .unwrap();

        let stem = parsed_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace("_parsed.json", "");
        let meta: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.join(format!("{stem}_meta.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["had_transaction_field"], false);
        assert_eq!(meta["endpoint"], endpoint);
        assert_eq!(meta["pair_label"], "USDC_SOL");
        assert!(dir.join(format!("{stem}_raw.json")).exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
