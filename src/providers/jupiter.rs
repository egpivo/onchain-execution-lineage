use anyhow::Result;
use serde_json::Value;

use super::{collect_unknown, take_string, ProviderAdapter};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::{LineageBundle, RouteLegObservation};

pub struct JupiterAdapter;

impl ProviderAdapter for JupiterAdapter {
    fn name(&self) -> &'static str {
        "jupiter"
    }

    fn detect(&self, value: &Value) -> bool {
        // Jupiter Ultra /order or classic quote shapes.
        value.get("router").is_some()
            || (value.get("routePlan").is_some()
                && value.get("requestId").is_none()
                && value.get("swapMode").is_some())
            || value.get("transaction").is_some()
                && value.get("inAmount").is_some()
                && value.get("otherAmountThreshold").is_some()
                && value.get("requestId").is_none()
                && value.get("routePlan").is_some()
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        bundle.capture.provider = "jupiter".into();
        bundle.quote.input_mint = take_string(value, "inputMint");
        bundle.quote.output_mint = take_string(value, "outputMint");
        bundle.quote.in_amount = take_string(value, "inAmount");
        bundle.quote.out_amount = take_string(value, "outAmount");
        bundle.quote.min_out_amount = take_string(value, "otherAmountThreshold");
        bundle.quote.request_or_quote_id =
            take_string(value, "quoteId").or_else(|| take_string(value, "requestId"));
        bundle.route.provider_route_label = take_string(value, "router");

        if let Some(legs) = value.get("routePlan").and_then(|r| r.as_array()) {
            for leg in legs {
                let swap = leg.get("swapInfo").unwrap_or(leg);
                bundle.route.legs.push(RouteLegObservation {
                    venue_or_label: take_string(swap, "label")
                        .or_else(|| take_string(swap, "ammKey"))
                        .unwrap_or_else(|| "unknown".into()),
                    input_mint: take_string(swap, "inputMint"),
                    output_mint: take_string(swap, "outputMint"),
                    in_amount: take_string(swap, "inAmount"),
                    out_amount: take_string(swap, "outAmount"),
                    market_key: take_string(swap, "ammKey"),
                });
            }
        }

        match value.get("transaction") {
            Some(v) if v.as_str().is_some() => {
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = Some("base64".into());
            }
            Some(v) if v.is_null() => bundle.transaction_construction.present = false,
            None => bundle.transaction_construction.present = false,
            Some(_) => bundle.transaction_construction.present = true,
        }

        if let Some(obj) = value.as_object() {
            let known = [
                "inputMint",
                "inAmount",
                "outputMint",
                "outAmount",
                "otherAmountThreshold",
                "swapMode",
                "slippageBps",
                "priceImpactPct",
                "routePlan",
                "contextSlot",
                "transaction",
                "router",
                "quoteId",
                "requestId",
                "platformFee",
            ];
            let ext = collect_unknown(obj, &known);
            if !ext.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                bundle.raw_extensions.insert("jupiter".into(), ext);
            }
        }

        if let Some(router) = &bundle.route.provider_route_label {
            bundle.push_claim(
                AttributionClaim::new(
                    "route",
                    "claimed_by_provider",
                    router.clone(),
                    EvidenceLevel::DirectObservation,
                    &bundle.capture.artifact_id,
                    "Jupiter router field — provider claim for this request",
                )
                .with_field("router"),
            );
        }

        Ok(())
    }
}
