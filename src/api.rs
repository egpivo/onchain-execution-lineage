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
    fetch_quote_at(DEV_QUOTE_ENDPOINT, req).await
}

/// Same as [`fetch_quote`], but against an arbitrary quote URL.
/// Used by tests (mock HTTP) and by capture when pointing at a fixture server.
pub async fn fetch_quote_at(
    endpoint: &str,
    req: &QuoteRequest,
) -> Result<(DFlowQuoteResponse, String, String)> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        endpoint, req.input_mint, req.output_mint, req.amount_atomic, req.slippage_bps
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .context("request to DFlow dev-quote-api failed")?;

    let status = resp.status();
    let raw_text = resp.text().await.context("failed to read response body")?;

    if !status.is_success() {
        anyhow::bail!(
            "DFlow dev-quote-api returned non-success status {}: {}",
            status,
            raw_text
        );
    }

    let mut hasher = Sha256::new();
    hasher.update(raw_text.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let parsed: DFlowQuoteResponse =
        serde_json::from_str(&raw_text).context("failed to deserialize DFlow quote response")?;

    Ok((parsed, raw_text, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIXTURE: &str = include_str!("../tests/fixtures/dev_quote_usdc_sol_no_tx.json");

    fn sample_req() -> QuoteRequest {
        QuoteRequest {
            input_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            output_mint: "So11111111111111111111111111111111111111112".into(),
            amount_atomic: 1_000_000_000,
            slippage_bps: 50,
        }
    }

    #[tokio::test]
    async fn fetch_quote_at_parses_success_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quote"))
            .and(query_param("amount", "1000000000"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
            .mount(&server)
            .await;

        let endpoint = format!("{}/quote", server.uri());
        let (parsed, raw, hash) = fetch_quote_at(&endpoint, &sample_req()).await.unwrap();

        let mut hasher = Sha256::new();
        hasher.update(raw.as_bytes());
        assert_eq!(format!("{:x}", hasher.finalize()), hash);
        assert!(parsed.transaction.is_none());
        assert_eq!(parsed.request_id, "332e8a00-0a5f-4266-a139-d657227e0dbf");
        assert_eq!(parsed.route_plan[0].venue, "Tessera V");
    }

    #[tokio::test]
    async fn fetch_quote_at_rejects_non_success_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quote"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;

        let endpoint = format!("{}/quote", server.uri());
        let err = fetch_quote_at(&endpoint, &sample_req()).await.unwrap_err();
        assert!(err.to_string().contains("non-success status"));
    }

    #[tokio::test]
    async fn fetch_quote_at_rejects_invalid_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quote"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
            .mount(&server)
            .await;

        let endpoint = format!("{}/quote", server.uri());
        let err = fetch_quote_at(&endpoint, &sample_req()).await.unwrap_err();
        assert!(err.to_string().contains("deserialize"));
    }
}
