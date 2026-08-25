use anyhow::Result;
use serde_json::Value;

use super::{take_string, ProviderAdapter};
use crate::lineage_model::LineageBundle;

/// Last-resort adapter: preserve the whole object under raw_extensions.
pub struct GenericAdapter;

impl ProviderAdapter for GenericAdapter {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn detect(&self, _value: &Value) -> bool {
        true
    }

    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()> {
        if bundle.capture.provider.is_empty() {
            bundle.capture.provider = "generic".into();
        }
        bundle.quote.input_mint = bundle
            .quote
            .input_mint
            .clone()
            .or_else(|| take_string(value, "inputMint"));
        bundle.quote.output_mint = bundle
            .quote
            .output_mint
            .clone()
            .or_else(|| take_string(value, "outputMint"));
        bundle
            .raw_extensions
            .insert("generic".into(), value.clone());
        bundle.push_unresolved(
            "provider_schema",
            "no specialized adapter matched; fields retained under raw_extensions.generic",
        );
        Ok(())
    }
}
