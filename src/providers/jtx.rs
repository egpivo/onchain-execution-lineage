//! Legacy JTX normalization: provider extraction → [`LineageBundle`].
//!
//! Field knowledge lives in [`crate::adapters::jtx`].

use anyhow::Result;
use serde_json::Value;

use super::ProviderAdapter;
use crate::adapters::jtx::JtxAdapter as JtxExtraction;
use crate::adapters::{ProviderAdapter as _, RawProviderArtifact};
use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::LineageBundle;

pub struct JtxAdapter;

impl ProviderAdapter for JtxAdapter {
    fn name(&self) -> &'static str {
        "jtx"
    }

    fn detect(&self, value: &Value) -> bool {
        JtxExtraction::detects(value)
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        let raw = RawProviderArtifact::from_value(value.clone());
        let e = JtxExtraction.extract(&raw)?;

        bundle.capture.provider = "jtx".into();
        bundle.quote.input_mint = e.response.input_mint.clone();
        bundle.quote.output_mint = e.response.output_mint.clone();
        bundle.quote.in_amount = e.response.in_amount.clone();
        bundle.quote.out_amount = e.response.out_amount.clone();
        bundle.quote.min_out_amount = e.response.min_out_amount.clone();
        bundle.quote.request_or_quote_id = e.response.request_or_quote_id.clone();

        if let Some(route) = &e.route {
            bundle.route.provider_route_label = route.provider_route_label.clone();
            bundle.route.legs.extend(route.legs.iter().cloned());
        }

        match &e.transaction {
            Some(t) if t.present => {
                bundle.transaction_construction.present = true;
                bundle.transaction_construction.encoding = t.encoding.clone();
            }
            _ => bundle.transaction_construction.present = false,
        }
        for u in &e.unsupported {
            if u.field == "transaction" {
                bundle.push_unresolved(u.field.clone(), u.reason.clone());
            }
        }

        if let Some(ext) = e.extensions.get("jtx") {
            bundle.raw_extensions.insert("jtx".into(), ext.clone());
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
