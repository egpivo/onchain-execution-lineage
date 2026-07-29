//! Rust client over DFlow's public no-key developer quote endpoint.
//!
//! This is a client for a public, documented-as-testing integration
//! surface -- not an official DFlow-published Rust SDK. DFlow does not
//! publish one; this module is this project's own reqwest-based client.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::models::DFlowQuoteResponse;

pub const DEV_QUOTE_ENDPOINT: &str = "https://dev-quote-api.dflow.net/quote";

pub struct QuoteRequest {
    pub input_mint: String,
    pub output_mint: String,
    pub amount_atomic: u64,
    pub slippage_bps: u32,
}

/// Returns the typed response, the raw response body text, and its
/// SHA-256 hash (so the raw bytes can be verified against the parsed
/// struct later without re-fetching).
pub async fn fetch_quote(req: &QuoteRequest) -> Result<(DFlowQuoteResponse, String, String)> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        DEV_QUOTE_ENDPOINT, req.input_mint, req.output_mint, req.amount_atomic, req.slippage_bps
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .context("request to DFlow dev-quote-api failed")?;

    let status = resp.status();
    let raw_text = resp.text().await.context("failed to read response body")?;

    if !status.is_success() {
        anyhow::bail!("DFlow dev-quote-api returned non-success status {}: {}", status, raw_text);
    }

    let mut hasher = Sha256::new();
    hasher.update(raw_text.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let parsed: DFlowQuoteResponse =
        serde_json::from_str(&raw_text).context("failed to deserialize DFlow quote response")?;

    Ok((parsed, raw_text, hash))
}
