//! Provider adapters: normalize heterogeneous quote/order JSON into
//! [`crate::lineage_model::LineageBundle`] pieces without economic conclusions.

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

pub(crate) fn collect_unknown(obj: &serde_json::Map<String, Value>, known: &[&str]) -> Value {
    let mut ext = serde_json::Map::new();
    for (k, v) in obj {
        if !known.contains(&k.as_str()) {
            ext.insert(k.clone(), v.clone());
        }
    }
    Value::Object(ext)
}
