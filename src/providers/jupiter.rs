//! Legacy Jupiter normalization: provider extraction → [`LineageBundle`].
//!
//! Field knowledge lives in [`crate::adapters::jupiter`]. Support is partial
//! there and stays partial here.

use anyhow::Result;
use serde_json::Value;

use super::ProviderAdapter;
use crate::adapters::jupiter::JupiterAdapter as JupiterExtraction;
use crate::adapters::{ProviderAdapter as _, RawProviderArtifact};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::LineageBundle;

pub struct JupiterAdapter;

impl ProviderAdapter for JupiterAdapter {
    fn name(&self) -> &'static str {
        "jupiter"
    }

    fn detect(&self, value: &Value) -> bool {
        JupiterExtraction::detects(value)
            || (value.get("transaction").is_some()
                && value.get("inAmount").is_some()
                && value.get("otherAmountThreshold").is_some()
                && value.get("requestId").is_none()
                && value.get("routePlan").is_some())
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        let raw = RawProviderArtifact::from_value(value.clone());
        let e = JupiterExtraction.extract(&raw)?;

        bundle.capture.provider = "jupiter".into();
        bundle.quote.input_mint = e.response.input_mint.clone();
        bundle.quote.output_mint = e.response.output_mint.clone();
        bundle.quote.in_amount = e.response.in_amount.clone();
        bundle.quote.out_amount = e.response.out_amount.clone();
        bundle.quote.min_out_amount = e.response.other_amount_threshold.clone();
        bundle.quote.request_or_quote_id = e.response.request_or_quote_id.clone();

        if let Some(route) = &e.route {
            bundle.route.provider_route_label = route.provider_route_label.clone();
            bundle.route.legs.extend(route.legs.iter().cloned());
        }

        bundle.transaction_construction.present =
            e.transaction.as_ref().map(|t| t.present).unwrap_or(false);
        bundle.transaction_construction.encoding =
            e.transaction.as_ref().and_then(|t| t.encoding.clone());

        if let Some(ext) = e.extensions.get("jupiter") {
            bundle.raw_extensions.insert("jupiter".into(), ext.clone());
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
