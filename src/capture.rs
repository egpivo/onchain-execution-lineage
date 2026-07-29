//! Orchestrates a quote capture: call the API, persist the raw response,
//! the typed/parsed response, and capture metadata -- all read-only, no
//! signing, no submission, no wallet involved at any point.

use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;

use crate::api::{fetch_quote, QuoteRequest};
use crate::models::CaptureMetadata;

pub async fn run_capture(
    pair_label: &str,
    input_mint: &str,
    output_mint: &str,
    amount_atomic: u64,
    amount_usd: f64,
    slippage_bps: u32,
    out_dir: &PathBuf,
) -> Result<PathBuf> {
    fs::create_dir_all(out_dir).context("failed to create captures directory")?;

    let req = QuoteRequest {
        input_mint: input_mint.to_string(),
        output_mint: output_mint.to_string(),
        amount_atomic,
        slippage_bps,
    };

    let (parsed, raw_text, hash) = fetch_quote(&req).await?;
    let captured_at = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

    let meta = CaptureMetadata {
        captured_at_utc: captured_at.clone(),
        pair_label: pair_label.to_string(),
        input_amount_usd: amount_usd,
        slippage_bps,
        raw_response_sha256: hash,
        had_transaction_field: parsed.transaction.is_some(),
        endpoint: crate::api::DEV_QUOTE_ENDPOINT.to_string(),
    };

    let capture_id = format!("{}_{}", pair_label, captured_at);
    let raw_path = out_dir.join(format!("{}_raw.json", capture_id));
    let parsed_path = out_dir.join(format!("{}_parsed.json", capture_id));
    let meta_path = out_dir.join(format!("{}_meta.json", capture_id));

    fs::write(&raw_path, &raw_text).context("failed to write raw response")?;
    fs::write(&parsed_path, serde_json::to_string_pretty(&parsed)?)
        .context("failed to write parsed response")?;
    fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)
        .context("failed to write capture metadata")?;

    println!("captured {} -> {}", pair_label, raw_path.display());
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
