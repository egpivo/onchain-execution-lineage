//! Legacy DFlow normalization: provider extraction → [`LineageBundle`].
//!
//! Kept so the existing `trace` / `experiment` / `route-bracket` paths keep
//! working unchanged. It no longer knows any DFlow field names — those live in
//! [`crate::adapters::dflow`], and this module only maps the neutral extraction
//! onto the older bundle shape.

use anyhow::Result;
use serde_json::Value;

use super::ProviderAdapter;
use crate::adapters::dflow::DflowAdapter as DflowExtraction;
use crate::adapters::{ProviderAdapter as _, RawProviderArtifact};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::LineageBundle;

pub struct DflowAdapter;

impl ProviderAdapter for DflowAdapter {
    fn name(&self) -> &'static str {
        "dflow"
    }

    fn detect(&self, value: &Value) -> bool {
        DflowExtraction::detects(value)
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        let raw = RawProviderArtifact::from_value(value.clone());
        let e = DflowExtraction.extract(&raw)?;

        bundle.capture.provider = "dflow".into();
        bundle.quote.input_mint = e.response.input_mint.clone();
        bundle.quote.output_mint = e.response.output_mint.clone();
        bundle.quote.in_amount = e.response.in_amount.clone();
        bundle.quote.out_amount = e.response.out_amount.clone();
        // The legacy bundle has one minimum slot; minOutAmount wins, matching
        // the behaviour this field had before the adapter split.
        bundle.quote.min_out_amount = e
            .response
            .min_out_amount
            .clone()
            .or_else(|| e.response.other_amount_threshold.clone());
        bundle.quote.request_or_quote_id = e.response.request_or_quote_id.clone();

        if let Some(fee) = &e.response.platform_fee {
            bundle.fee.platform_fee_visible = fee.visible.clone();
            bundle.fee.fee_bps = fee.fee_bps;
            bundle.fee.fee_account = fee.fee_account.clone();
            bundle.fee.fee_mint = fee.fee_mint.clone();
            bundle.fee.mode = fee.mode.clone();
        }

        if let Some(route) = &e.route {
            // Leg observations carry over; the route *label* does not. DFlow
            // names no router, and a single leg's venue is not one.
            bundle.route.legs.extend(route.legs.iter().cloned());
        }

        if let Some(flag) = e.extensions.get("for_jito_bundle").and_then(Value::as_bool) {
            bundle.delivery.for_jito_bundle_flag = Some(flag);
        }

        match &e.transaction {
            Some(t) if t.present => {
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = t.encoding.clone();
                if t.encoding.is_none() {
                    bundle.push_unresolved("transaction", "non-string transaction payload");
                }
            }
            _ => bundle.transaction_construction.present = false,
        }
        for u in &e.unsupported {
            bundle.push_unresolved(u.field.clone(), u.reason.clone());
        }

        if let Some(mode) = &e.response.execution_mode {
            bundle
                .raw_extensions
                .insert("execution_mode".into(), serde_json::json!(mode));
        }
        if let Some(ext) = e.extensions.get("dflow") {
            bundle.raw_extensions.insert("dflow".into(), ext.clone());
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
