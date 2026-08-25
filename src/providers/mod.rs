//! # Compatibility shim — closed to new provider semantics
//!
//! Legacy normalization: quote/order JSON straight into
//! [`crate::lineage_model::LineageBundle`] pieces. Superseded by
//! [`crate::adapters`], which returns a `ProviderExtraction` instead of
//! mutating the lineage model.
//!
//! **The boundary:** all provider-specific extraction lives under
//! `adapters/`. Every adapter here is a mapping shim over its `adapters/`
//! counterpart — `dflow`, `jupiter` and `jtx` delegate outright, and `generic`
//! only fills two fields it can read without provider knowledge. No provider
//! field name is read in this module.
//!
//! **What may change here:** nothing but the mapping from a
//! `ProviderExtraction` onto the older bundle shape. A new provider, a new
//! field, or a new detection rule belongs in `adapters/`, and reaches this
//! module only through delegation.
//!
//! **Why it still exists:** [`crate::experiment`] and
//! [`crate::route_bracket`] call [`normalize_provider_json`] directly, and
//! removing it would change recorded experiment behaviour. It is not on the
//! canonical lineage path — `trace` no longer uses it — so it can be deleted
//! once those two callers move to `adapters/`.

pub mod dflow;
pub mod generic;
pub mod jtx;
pub mod jupiter;

use anyhow::Result;
use serde_json::Value;

use crate::lineage_model::LineageBundle;

pub trait ProviderAdapter {
    fn name(&self) -> &'static str;
    fn detect(&self, value: &Value) -> bool;
    fn normalize(&self, value: &Value, bundle: &mut LineageBundle) -> Result<()>;
}

pub fn all_adapters() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(jtx::JtxAdapter),
        Box::new(dflow::DflowAdapter),
        Box::new(jupiter::JupiterAdapter),
        Box::new(generic::GenericAdapter),
    ]
}

/// Pick the first detecting adapter, else generic.
pub fn normalize_provider_json(value: &Value, bundle: &mut LineageBundle) -> Result<&'static str> {
    for adapter in all_adapters() {
        if adapter.detect(value) {
            adapter.normalize(value, bundle)?;
            return Ok(adapter.name());
        }
    }
    generic::GenericAdapter.normalize(value, bundle)?;
    Ok(generic::GenericAdapter.name())
}

pub(crate) fn take_string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| {
        x.as_str()
            .map(|s| s.to_string())
            .or_else(|| x.as_u64().map(|n| n.to_string()))
            .or_else(|| x.as_i64().map(|n| n.to_string()))
    })
}
