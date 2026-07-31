use anyhow::Result;
use serde_json::Value;

use super::{collect_unknown, take_string, ProviderAdapter};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::{LineageBundle, RouteLegObservation};

pub struct JtxAdapter;

impl ProviderAdapter for JtxAdapter {
    fn name(&self) -> &'static str {
        "jtx"
    }

    fn detect(&self, value: &Value) -> bool {
        // Sanitized capture envelope or nested quote body from api.jtx.com.
        value.get("capture_method").is_some()
            || value.pointer("/response/body/quote/orderId").is_some()
            || (value.get("quote").is_some() && value.get("transaction").is_some())
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        bundle.capture.provider = "jtx".into();
        let quote = value
            .pointer("/response/body/quote")
            .or_else(|| value.get("quote"))
            .cloned()
            .unwrap_or_else(|| value.clone());

        bundle.quote.input_mint = take_string(&quote, "inputMint");
        bundle.quote.output_mint = take_string(&quote, "outputMint");
        bundle.quote.in_amount = take_string(&quote, "inAmount");
        bundle.quote.out_amount =
            take_string(&quote, "outAmountQuoted").or_else(|| take_string(&quote, "outAmount"));
        bundle.quote.min_out_amount = take_string(&quote, "minOutAmount");
        bundle.quote.request_or_quote_id = take_string(&quote, "orderId");

        if let Some(route) = quote.get("route").and_then(|r| r.as_array()) {
            for leg in route {
                bundle.route.legs.push(RouteLegObservation {
                    venue_or_label: take_string(leg, "venue").unwrap_or_else(|| "unknown".into()),
                    input_mint: take_string(leg, "inputMint"),
                    output_mint: take_string(leg, "outputMint"),
                    in_amount: take_string(leg, "inAmount"),
                    out_amount: take_string(leg, "outAmount"),
                    market_key: None,
                });
            }
            if let Some(first) = bundle.route.legs.first() {
                bundle.route.provider_route_label = Some(first.venue_or_label.clone());
            }
        }

        let tx = value
            .pointer("/response/body/transaction")
            .or_else(|| value.get("transaction"));
        match tx {
            Some(v) if v.as_str().map(|s| s.starts_with('<')).unwrap_or(false) => {
                // Sanitized placeholder pointing at a sibling base64 file.
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = Some("base64_external_ref".into());
                bundle.push_unresolved(
                    "transaction",
                    "sanitized capture references external base64 file; decode that file separately",
                );
            }
            Some(v) if v.as_str().is_some() => {
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = Some("base64".into());
            }
            Some(v) if v.is_null() => {
                bundle.transaction_construction.present = false;
            }
            None => bundle.transaction_construction.present = false,
            Some(_) => bundle.transaction_construction.present = true,
        }

        if let Some(obj) = value.as_object() {
            let known = [
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
            let ext = collect_unknown(obj, &known);
            if !ext.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                bundle.raw_extensions.insert("jtx".into(), ext);
            }
        }

        if let Some(label) = &bundle.route.provider_route_label {
            bundle.push_claim(
                AttributionClaim::new(
                    "route",
                    "claimed_by_interface",
                    label.clone(),
                    EvidenceLevel::DirectObservation,
                    &bundle.capture.artifact_id,
                    "venue/label from JTX quote JSON route array — interface claim, not on-chain proof",
                )
                .with_field("quote.route[].venue"),
            );
        }

        Ok(())
    }
}
