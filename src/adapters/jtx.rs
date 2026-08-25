//! JTX adapter.
//!
//! Moved verbatim from the legacy `providers::jtx` normalization — same field
//! knowledge, same detection rules, now expressed as a
//! [`ProviderExtraction`] instead of direct mutation of a lineage bundle. No
//! new semantics were added in the move.
//!
//! JTX captures are sanitized envelopes: the quote body sits under
//! `/response/body/quote`, and the transaction is often a placeholder pointing
//! at a sibling base64 file rather than an inline payload.

use anyhow::Result;
use serde_json::Value;

use super::{
    as_string, transaction_ref, ExecutionIntent, ProviderAdapter, ProviderExtraction, ProviderId,
    ProviderResponse, RawProviderArtifact, UnsignedTransactionRef,
};
use crate::lineage_model::{RouteLegObservation, RouteObservation};

const KNOWN_FIELDS: &[&str] = &[
    "capture_method",
    "capture_tool",
    "phase",
    "target_params",
    "request",
    "response",
    "wallet_balance_at_capture_time",
    "quote",
    "transaction",
    "clientRequestId",
];

pub struct JtxAdapter;

impl JtxAdapter {
    pub fn detects(body: &Value) -> bool {
        body.get("capture_method").is_some()
            || body.pointer("/response/body/quote/orderId").is_some()
            || (body.get("quote").is_some() && body.get("transaction").is_some())
    }
}

impl ProviderAdapter for JtxAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Jtx
    }

    fn detect(&self, raw: &RawProviderArtifact) -> bool {
        Self::detects(&raw.body)
    }

    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction> {
        let body = &raw.body;
        let mut out = ProviderExtraction::empty(ProviderId::Jtx);
        out.surface = raw.surface.clone();

        let quote = body
            .pointer("/response/body/quote")
            .or_else(|| body.get("quote"))
            .cloned()
            .unwrap_or_else(|| body.clone());

        out.response = ProviderResponse {
            input_mint: as_string(&quote, "inputMint"),
            output_mint: as_string(&quote, "outputMint"),
            in_amount: as_string(&quote, "inAmount"),
            out_amount: as_string(&quote, "outAmountQuoted")
                .or_else(|| as_string(&quote, "outAmount")),
            min_out_amount: as_string(&quote, "minOutAmount"),
            // JTX quotes carry no otherAmountThreshold; the minimum is the only
            // bound the capture reports.
            other_amount_threshold: None,
            slippage_bps: super::as_u32(&quote, "slippageBps"),
            price_impact_pct: None,
            context_slot: None,
            request_or_quote_id: as_string(&quote, "orderId"),
            execution_mode: None,
            last_valid_block_height: None,
            compute_unit_limit: None,
            prioritization_fee_lamports: None,
            platform_fee: None,
            error: None,
        };
        out.push_unsupported(
            "otherAmountThreshold",
            "JTX quote captures carry no separate threshold field",
        );

        let intent = ExecutionIntent {
            input_mint: out.response.input_mint.clone(),
            output_mint: out.response.output_mint.clone(),
            in_amount: out.response.in_amount.clone(),
            slippage_bps: out.response.slippage_bps,
            user_public_key: None,
            recovered_from: "provider_response_echo".into(),
        };
        if intent != ExecutionIntent::default() {
            out.intent = Some(intent);
        }

        if let Some(legs) = quote.get("route").and_then(|r| r.as_array()) {
            let mut route = RouteObservation::default();
            for leg in legs {
                route.legs.push(RouteLegObservation {
                    venue_or_label: as_string(leg, "venue").unwrap_or_else(|| "unknown".into()),
                    input_mint: as_string(leg, "inputMint"),
                    output_mint: as_string(leg, "outputMint"),
                    in_amount: as_string(leg, "inAmount"),
                    out_amount: as_string(leg, "outAmount"),
                    market_key: None,
                });
            }
            // The interface names its own router through the first leg's venue.
            route.provider_route_label = route.legs.first().map(|l| l.venue_or_label.clone());
            out.route = Some(route);
        }

        // The transaction may live at the envelope root or under the response.
        let tx_holder = if body.pointer("/response/body/transaction").is_some() {
            body.pointer("/response/body").cloned().unwrap_or_default()
        } else {
            body.clone()
        };
        let tx: UnsignedTransactionRef = transaction_ref(&tx_holder, "transaction");
        if tx.external_ref.is_some() {
            out.push_unsupported(
                "transaction",
                "sanitized capture references an external base64 file; pass it with \
                 --transaction to decode",
            );
        }
        out.transaction = Some(tx);

        let leftovers = super::unknown_fields(body, KNOWN_FIELDS);
        if !leftovers.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            out.extensions.insert("jtx".into(), leftovers);
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> Value {
        serde_json::json!({
            "capture_method": "browser",
            "response": { "body": {
                "quote": {
                    "orderId": "o1",
                    "inputMint": "MintA",
                    "outputMint": "MintB",
                    "inAmount": "100",
                    "outAmountQuoted": "200",
                    "minOutAmount": "199",
                    "route": [{ "venue": "DFlow JIT Router", "inputMint": "MintA",
                                "outputMint": "MintB" }]
                },
                "transaction": "<see sibling base64 file>"
            }}
        })
    }

    #[test]
    fn reads_sanitized_capture_envelope() {
        let raw = RawProviderArtifact::from_value(capture());
        assert!(JtxAdapter.detect(&raw));

        let e = JtxAdapter.extract(&raw).unwrap();
        assert_eq!(e.provider, ProviderId::Jtx);
        assert_eq!(e.response.request_or_quote_id.as_deref(), Some("o1"));
        assert_eq!(e.response.out_amount.as_deref(), Some("200"));
        assert_eq!(
            e.route.as_ref().unwrap().provider_route_label.as_deref(),
            Some("DFlow JIT Router")
        );
    }

    #[test]
    fn external_transaction_reference_is_not_a_payload() {
        let e = JtxAdapter
            .extract(&RawProviderArtifact::from_value(capture()))
            .unwrap();
        let tx = e.transaction.unwrap();
        assert!(tx.present);
        assert_eq!(tx.encoding.as_deref(), Some("base64_external_ref"));
        assert!(tx.payload.is_none());
        assert!(e.unsupported.iter().any(|u| u.field == "transaction"));
    }
}
