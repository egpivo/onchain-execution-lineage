use anyhow::Result;
use serde_json::Value;

use super::{collect_unknown, take_string, ProviderAdapter};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::{LineageBundle, RouteLegObservation};

pub struct DflowAdapter;

impl ProviderAdapter for DflowAdapter {
    fn name(&self) -> &'static str {
        "dflow"
    }

    fn detect(&self, value: &Value) -> bool {
        // Developer quote/order style: routePlan array + requestId, or explicit
        // contextSlot from DFlow quote surfaces.
        value.get("routePlan").is_some() && value.get("requestId").is_some()
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        let obj = value.as_object().cloned().unwrap_or_default();
        bundle.capture.provider = "dflow".into();
        bundle.quote.input_mint = take_string(value, "inputMint");
        bundle.quote.output_mint = take_string(value, "outputMint");
        bundle.quote.in_amount = take_string(value, "inAmount");
        bundle.quote.out_amount = take_string(value, "outAmount");
        bundle.quote.min_out_amount = take_string(value, "minOutAmount")
            .or_else(|| take_string(value, "otherAmountThreshold"));
        bundle.quote.request_or_quote_id = take_string(value, "requestId");

        if let Some(fee) = value.get("platformFee") {
            if fee.is_null() {
                bundle.fee.platform_fee_visible = Some("null".into());
            } else {
                bundle.fee.platform_fee_visible = Some(fee.to_string());
                if let Some(bps) = fee.get("feeBps").and_then(|b| b.as_u64()) {
                    bundle.fee.fee_bps = Some(bps as u32);
                }
                bundle.fee.fee_account = take_string(fee, "feeAccount");
                bundle.fee.fee_mint = take_string(fee, "feeMint");
                bundle.fee.mode = take_string(fee, "mode");
            }
        }

        if let Some(legs) = value.get("routePlan").and_then(|r| r.as_array()) {
            for leg in legs {
                bundle.route.legs.push(RouteLegObservation {
                    venue_or_label: take_string(leg, "venue").unwrap_or_else(|| "unknown".into()),
                    input_mint: take_string(leg, "inputMint"),
                    output_mint: take_string(leg, "outputMint"),
                    in_amount: take_string(leg, "inAmount"),
                    out_amount: take_string(leg, "outAmount"),
                    market_key: take_string(leg, "marketKey"),
                });
            }
        }

        if let Some(flag) = value.get("forJitoBundle").and_then(|v| v.as_bool()) {
            bundle.delivery.for_jito_bundle_flag = Some(flag);
        }

        match value.get("transaction") {
            None => {
                bundle.transaction_construction.present = false;
                bundle.push_unresolved(
                    "transaction",
                    "field absent on this DFlow surface (quote-only or omitted)",
                );
            }
            Some(v) if v.is_null() => {
                bundle.transaction_construction.present = false;
                bundle.push_unresolved("transaction", "field present but null");
            }
            Some(v) if v.as_str().is_some() => {
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = Some("base64".into());
            }
            Some(_) => {
                bundle.transaction_construction.present = true;
                bundle.push_unresolved("transaction", "non-string transaction payload");
            }
        }

        let known = [
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
        ];
        let ext = collect_unknown(&obj, &known);
        if !ext.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            bundle.raw_extensions.insert("dflow".into(), ext);
        }

        if let Some(id) = &bundle.quote.request_or_quote_id {
            bundle.push_claim(
                AttributionClaim::new(
                    "quote",
                    "has_request_id",
                    id.clone(),
                    EvidenceLevel::DirectObservation,
                    &bundle.capture.artifact_id,
                    "requestId present in DFlow JSON response",
                )
                .with_field("requestId"),
            );
        }

        Ok(())
    }
}
