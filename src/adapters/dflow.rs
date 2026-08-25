//! DFlow adapter — the first complete provider integration.
//!
//! This module is the only place in the crate that may read DFlow's native
//! field names. Field shapes here were confirmed against live captures
//! (`dev-quote-api.dflow.net/quote`, 2026-07-29; the `/order` surface,
//! 2026-07-31) and against the recorded artifacts under
//! `artifacts/experiments/`, not from documentation.
//!
//! Generic Solana mechanics do not belong here: this adapter never decodes the
//! transaction it finds, it only reports that a payload exists and hands the
//! bytes on.

use anyhow::Result;
use serde_json::Value;

use super::{
    as_string, as_u32, as_u64, json_to_string, transaction_ref, ExecutionIntent,
    PlatformFeeObservation, ProviderAdapter, ProviderExtraction, ProviderId, ProviderResponse,
    RawProviderArtifact,
};
use crate::lineage_model::{RouteLegObservation, RouteObservation};

/// Every DFlow key this adapter understands. Anything outside this list is
/// preserved verbatim under `extensions["dflow"]`.
const KNOWN_FIELDS: &[&str] = &[
    "inputMint",
    "inAmount",
    "outputMint",
    "outAmount",
    "otherAmountThreshold",
    "minOutAmount",
    "slippageBps",
    "platformFee",
    "outTransferFee",
    "priceImpactPct",
    "routePlan",
    "contextSlot",
    "requestId",
    "forJitoBundle",
    "transaction",
    "executionMode",
    "lastValidBlockHeight",
    "computeUnitLimit",
    "prioritizationFeeLamports",
    "prioritizationType",
    "revertMint",
    "userPublicKey",
];

pub struct DflowAdapter;

impl DflowAdapter {
    /// Shape test shared by the adapter and the legacy `providers::dflow` shim.
    pub fn detects(body: &Value) -> bool {
        let has_route = body.get("routePlan").is_some();
        // `/quote` carries requestId; `/order` carries executionMode and/or an
        // inline transaction and may omit requestId entirely.
        let quote_shaped = body.get("requestId").is_some();
        let order_shaped = body.get("executionMode").is_some()
            || body.get("lastValidBlockHeight").is_some()
            || (body.get("transaction").and_then(|t| t.as_str()).is_some()
                && body.get("outAmount").is_some());
        has_route && (quote_shaped || order_shaped)
    }
}

impl ProviderAdapter for DflowAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Dflow
    }

    fn detect(&self, raw: &RawProviderArtifact) -> bool {
        Self::detects(&raw.body)
    }

    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction> {
        let body = &raw.body;
        let mut out = ProviderExtraction::empty(ProviderId::Dflow);

        out.surface = raw.surface.clone().or_else(|| {
            // Inferred, not asserted: an inline transaction only ever appeared
            // on the order surface in the recorded captures.
            if body.get("transaction").is_some() || body.get("executionMode").is_some() {
                Some("order".into())
            } else if body.get("requestId").is_some() {
                Some("quote".into())
            } else {
                None
            }
        });

        out.response = ProviderResponse {
            input_mint: as_string(body, "inputMint"),
            output_mint: as_string(body, "outputMint"),
            in_amount: as_string(body, "inAmount"),
            out_amount: as_string(body, "outAmount"),
            min_out_amount: as_string(body, "minOutAmount"),
            other_amount_threshold: as_string(body, "otherAmountThreshold"),
            slippage_bps: as_u32(body, "slippageBps"),
            price_impact_pct: as_string(body, "priceImpactPct"),
            context_slot: as_u64(body, "contextSlot"),
            request_or_quote_id: as_string(body, "requestId"),
            execution_mode: as_string(body, "executionMode"),
            last_valid_block_height: as_u64(body, "lastValidBlockHeight"),
            compute_unit_limit: as_u64(body, "computeUnitLimit"),
            prioritization_fee_lamports: as_u64(body, "prioritizationFeeLamports"),
            platform_fee: platform_fee(body),
            error: as_string(body, "error"),
        };

        // The response echoes the request parameters. That is the provider's
        // account of the request, so the intent is marked as an echo.
        let intent = ExecutionIntent {
            input_mint: out.response.input_mint.clone(),
            output_mint: out.response.output_mint.clone(),
            in_amount: out.response.in_amount.clone(),
            slippage_bps: out.response.slippage_bps,
            user_public_key: as_string(body, "userPublicKey"),
            recovered_from: "provider_response_echo".into(),
        };
        if intent != ExecutionIntent::default() {
            out.intent = Some(intent);
        }

        if let Some(legs) = body.get("routePlan").and_then(|r| r.as_array()) {
            let mut route = RouteObservation::default();
            for leg in legs {
                route.legs.push(RouteLegObservation {
                    venue_or_label: as_string(leg, "venue").unwrap_or_else(|| "unknown".into()),
                    input_mint: as_string(leg, "inputMint"),
                    output_mint: as_string(leg, "outputMint"),
                    in_amount: as_string(leg, "inAmount"),
                    out_amount: as_string(leg, "outAmount"),
                    market_key: as_string(leg, "marketKey"),
                });
            }
            // DFlow names no router; the label is the leg venue only when the
            // route is a single hop.
            if route.legs.len() == 1 {
                route.provider_route_label = Some(route.legs[0].venue_or_label.clone());
            }
            out.route = Some(route);
        }

        let tx = transaction_ref(body, "transaction");
        if !tx.present {
            out.push_unsupported(
                "transaction",
                match body.get("transaction") {
                    Some(Value::Null) => "field present but null",
                    None => "field absent on this DFlow surface (quote-only or omitted)",
                    _ => unreachable!("transaction_ref reports present for other shapes"),
                },
            );
        }
        out.transaction = Some(tx);

        if let Some(mode) = out.response.execution_mode.clone() {
            out.extensions
                .insert("execution_mode".into(), serde_json::json!(mode));
        }
        if let Some(flag) = body.get("forJitoBundle").and_then(Value::as_bool) {
            out.extensions
                .insert("for_jito_bundle".into(), serde_json::json!(flag));
        }
        if let Some(pt) = body.get("prioritizationType") {
            out.extensions
                .insert("prioritization_type".into(), pt.clone());
        }
        let leftovers = super::unknown_fields(body, KNOWN_FIELDS);
        if !leftovers.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            out.extensions.insert("dflow".into(), leftovers);
        }

        Ok(out)
    }
}

fn platform_fee(body: &Value) -> Option<PlatformFeeObservation> {
    let fee = body.get("platformFee")?;
    if fee.is_null() {
        return Some(PlatformFeeObservation {
            present: false,
            visible: Some("null".into()),
            ..Default::default()
        });
    }
    Some(PlatformFeeObservation {
        present: true,
        visible: Some(fee.to_string()),
        fee_bps: fee.get("feeBps").and_then(Value::as_u64).map(|b| b as u32),
        amount: fee.get("amount").and_then(json_to_string),
        fee_account: as_string(fee, "feeAccount"),
        fee_mint: as_string(fee, "feeMint"),
        mode: as_string(fee, "mode"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_response() -> Value {
        serde_json::json!({
            "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "inAmount": "100000000",
            "outputMint": "So11111111111111111111111111111111111111112",
            "outAmount": "1373827780",
            "otherAmountThreshold": "1366958642",
            "minOutAmount": "1366958642",
            "slippageBps": 50,
            "platformFee": { "amount": "0", "feeBps": 0, "mode": "outputMint" },
            "priceImpactPct": "0",
            "routePlan": [{
                "venue": "ZeroFi",
                "marketKey": "BwNJma5cJzn9jobrsc4d7UuWHekjai1B7oUdRzRnVqAM",
                "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                "outputMint": "So11111111111111111111111111111111111111112",
                "inAmount": "100000000",
                "outAmount": "1373827780",
                "inputMintDecimals": 6,
                "outputMintDecimals": 9
            }],
            "contextSlot": 436439879u64,
            "executionMode": "sync",
            "transaction": "AQAB",
            "lastValidBlockHeight": 414496162u64,
            "prioritizationFeeLamports": 20,
            "computeUnitLimit": 200000,
        })
    }

    #[test]
    fn extracts_order_response_into_neutral_fields() {
        let raw = RawProviderArtifact::from_value(order_response());
        assert!(DflowAdapter.detect(&raw));
        let e = DflowAdapter.extract(&raw).unwrap();

        assert_eq!(e.provider, ProviderId::Dflow);
        assert_eq!(e.surface.as_deref(), Some("order"));
        assert_eq!(e.response.out_amount.as_deref(), Some("1373827780"));
        // Kept apart, not collapsed.
        assert_eq!(e.response.min_out_amount.as_deref(), Some("1366958642"));
        assert_eq!(
            e.response.other_amount_threshold.as_deref(),
            Some("1366958642")
        );
        assert_eq!(e.response.slippage_bps, Some(50));
        assert_eq!(e.response.execution_mode.as_deref(), Some("sync"));
        assert_eq!(e.route.as_ref().unwrap().legs.len(), 1);
        assert_eq!(
            e.route.as_ref().unwrap().provider_route_label.as_deref(),
            Some("ZeroFi")
        );
        let tx = e.transaction.unwrap();
        assert!(tx.present);
        assert_eq!(tx.payload.as_deref(), Some("AQAB"));
        assert_eq!(
            e.intent.as_ref().unwrap().recovered_from,
            "provider_response_echo"
        );
    }

    #[test]
    fn incomplete_response_records_unsupported_transaction() {
        let mut body = order_response();
        body.as_object_mut().unwrap().remove("transaction");
        body.as_object_mut().unwrap().remove("executionMode");
        body["requestId"] = serde_json::json!("00000000-0000-0000-0000-000000000000");
        let raw = RawProviderArtifact::from_value(body);

        let e = DflowAdapter.extract(&raw).unwrap();
        assert_eq!(e.surface.as_deref(), Some("quote"));
        assert!(!e.transaction.unwrap().present);
        assert!(e
            .unsupported
            .iter()
            .any(|u| u.field == "transaction" && u.reason.contains("absent")));
    }

    #[test]
    fn null_platform_fee_is_not_the_same_as_absent() {
        let mut body = order_response();
        body["platformFee"] = Value::Null;
        let e = DflowAdapter
            .extract(&RawProviderArtifact::from_value(body))
            .unwrap();
        let fee = e.response.platform_fee.unwrap();
        assert!(!fee.present);
        assert_eq!(fee.visible.as_deref(), Some("null"));

        let mut body = order_response();
        body.as_object_mut().unwrap().remove("platformFee");
        let e = DflowAdapter
            .extract(&RawProviderArtifact::from_value(body))
            .unwrap();
        assert!(e.response.platform_fee.is_none());
    }

    #[test]
    fn unknown_fields_are_preserved_namespaced() {
        let mut body = order_response();
        body["someFutureField"] = serde_json::json!(7);
        let e = DflowAdapter
            .extract(&RawProviderArtifact::from_value(body))
            .unwrap();
        assert_eq!(e.extensions["dflow"]["someFutureField"], 7);
    }
}
