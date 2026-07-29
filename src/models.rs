//! Typed models for DFlow's no-key developer quote endpoint.
//!
//! Field shape confirmed live against dev-quote-api.dflow.net on
//! 2026-07-29 -- not guessed from documentation. Notably, this response
//! has no `transaction` field: this endpoint is quote-only, distinct from
//! an order/swap endpoint that would return a signable payload. That
//! absence is itself a real finding, recorded in the field lineage output.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoutePlanLeg {
    pub venue: String,
    #[serde(rename = "marketKey")]
    pub market_key: String,
    #[serde(rename = "inputMint")]
    pub input_mint: String,
    #[serde(rename = "outputMint")]
    pub output_mint: String,
    #[serde(rename = "inAmount")]
    pub in_amount: String,
    #[serde(rename = "outAmount")]
    pub out_amount: String,
    #[serde(rename = "inputMintDecimals")]
    pub input_mint_decimals: u8,
    #[serde(rename = "outputMintDecimals")]
    pub output_mint_decimals: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DFlowQuoteResponse {
    #[serde(rename = "inputMint")]
    pub input_mint: String,
    #[serde(rename = "inAmount")]
    pub in_amount: String,
    #[serde(rename = "outputMint")]
    pub output_mint: String,
    #[serde(rename = "outAmount")]
    pub out_amount: String,
    #[serde(rename = "otherAmountThreshold")]
    pub other_amount_threshold: String,
    #[serde(rename = "minOutAmount")]
    pub min_out_amount: String,
    #[serde(rename = "slippageBps")]
    pub slippage_bps: u32,
    #[serde(rename = "platformFee")]
    pub platform_fee: Option<serde_json::Value>,
    #[serde(rename = "outTransferFee")]
    pub out_transfer_fee: Option<serde_json::Value>,
    #[serde(rename = "priceImpactPct")]
    pub price_impact_pct: String,
    #[serde(rename = "routePlan")]
    pub route_plan: Vec<RoutePlanLeg>,
    #[serde(rename = "contextSlot")]
    pub context_slot: u64,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "forJitoBundle")]
    pub for_jito_bundle: bool,

    /// Deliberately optional and checked at runtime -- if a future DFlow
    /// surface (e.g. an authenticated order/swap endpoint) does return
    /// this field, we want to know, not silently ignore it via a strict
    /// schema mismatch.
    pub transaction: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureMetadata {
    pub captured_at_utc: String,
    pub pair_label: String,
    pub input_amount_usd: f64,
    pub slippage_bps: u32,
    pub raw_response_sha256: String,
    pub had_transaction_field: bool,
    pub endpoint: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_quote_json() -> serde_json::Value {
        serde_json::json!({
            "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "inAmount": "1000000000",
            "outputMint": "So11111111111111111111111111111111111111112",
            "outAmount": "1",
            "otherAmountThreshold": "1",
            "minOutAmount": "1",
            "slippageBps": 50,
            "platformFee": null,
            "outTransferFee": null,
            "priceImpactPct": "0",
            "routePlan": [{
                "venue": "Tessera V",
                "marketKey": "11111111111111111111111111111111",
                "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                "outputMint": "So11111111111111111111111111111111111111112",
                "inAmount": "1000000000",
                "outAmount": "1",
                "inputMintDecimals": 6,
                "outputMintDecimals": 9
            }],
            "contextSlot": 1,
            "requestId": "00000000-0000-0000-0000-000000000000",
            "forJitoBundle": false
        })
    }

    #[test]
    fn transaction_field_present_when_base64_string() {
        let mut value = minimal_quote_json();
        value["transaction"] = serde_json::json!("AQAAAA...synthetic...");
        let parsed: DFlowQuoteResponse = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.transaction.as_deref(), Some("AQAAAA...synthetic..."));
    }

    #[test]
    fn transaction_null_deserializes_as_none() {
        let mut value = minimal_quote_json();
        value["transaction"] = serde_json::Value::Null;
        let parsed: DFlowQuoteResponse = serde_json::from_value(value).unwrap();
        assert!(parsed.transaction.is_none());
    }

    #[test]
    fn absent_transaction_key_deserializes_as_none() {
        let value = minimal_quote_json();
        assert!(!value.as_object().unwrap().contains_key("transaction"));
        let parsed: DFlowQuoteResponse = serde_json::from_value(value).unwrap();
        assert!(parsed.transaction.is_none());
    }
}
