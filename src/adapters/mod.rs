//! Provider adapter boundary.
//!
//! Provider-specific API semantics stop here. An adapter takes a
//! [`RawProviderArtifact`] (whatever the provider actually returned) and emits
//! a [`ProviderExtraction`] built only from provider-neutral field names. Core
//! code downstream of this module must never read `outAmount`,
//! `otherAmountThreshold`, `routePlan` or any other provider-native key.
//!
//! Raw provider-specific leftovers are preserved under
//! [`ProviderExtraction::extensions`], namespaced by provider. Generic code may
//! print them; it must not branch on them.
//!
//! This is the *new* boundary. [`crate::providers`] is the older trait that
//! writes straight into a [`crate::lineage_model::LineageBundle`]; it is kept
//! for the existing `trace` / `experiment` paths and now delegates its field
//! extraction here so DFlow and Jupiter field names live in one place.

pub mod dflow;
pub mod generic;
pub mod jtx;
pub mod jupiter;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::artifact::sha256_bytes;
use crate::lineage_model::RouteObservation;

/// Providers with a first-class adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Dflow,
    Jupiter,
    Jtx,
    Generic,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Dflow => "dflow",
            ProviderId::Jupiter => "jupiter",
            ProviderId::Jtx => "jtx",
            ProviderId::Generic => "generic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dflow" => Some(ProviderId::Dflow),
            "jupiter" | "jup" => Some(ProviderId::Jupiter),
            "jtx" => Some(ProviderId::Jtx),
            "generic" => Some(ProviderId::Generic),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A raw provider artifact exactly as captured, plus where it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawProviderArtifact {
    pub body: Value,
    /// Provider surface if known: `order`, `quote`, `swap`, …
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    /// SHA-256 of the bytes the body was parsed from, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at_utc: Option<String>,
}

impl RawProviderArtifact {
    pub fn from_value(body: Value) -> Self {
        Self {
            body,
            surface: None,
            source_path: None,
            sha256: None,
            captured_at_utc: None,
        }
    }

    /// Parse raw bytes and record their hash, so provenance is content-derived
    /// rather than supplied by the caller.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let body: Value = serde_json::from_slice(bytes)?;
        Ok(Self {
            body,
            surface: None,
            source_path: None,
            sha256: Some(sha256_bytes(bytes)),
            captured_at_utc: None,
        })
    }

    pub fn with_source_path(mut self, path: impl Into<String>) -> Self {
        self.source_path = Some(path.into());
        self
    }

    pub fn with_surface(mut self, surface: impl Into<String>) -> Self {
        self.surface = Some(surface.into());
        self
    }
}

/// What the caller asked for, to the extent the artifact still shows it.
///
/// Most quote/order responses echo the request parameters. That echo is the
/// provider's account of the request, not an independent record of it, which is
/// what `recovered_from` says.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExecutionIntent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_public_key: Option<String>,
    /// e.g. `provider_response_echo`, `manifest`, `caller_supplied`.
    pub recovered_from: String,
}

/// Normalized platform-fee observation. `visible` keeps the provider's own
/// rendering so a null field and an absent field stay distinguishable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlatformFeeObservation {
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_bps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Provider response in provider-neutral vocabulary.
///
/// `min_out_amount` and `other_amount_threshold` are kept apart on purpose:
/// DFlow returns both, and whether they agree is a checkable fact, not
/// something to collapse during normalization.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out_amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_out_amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_amount_threshold: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_impact_pct: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_or_quote_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_valid_block_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_unit_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prioritization_fee_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_fee: Option<PlatformFeeObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How a provider handed over the unsigned transaction, if at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnsignedTransactionRef {
    pub present: bool,
    /// `base64`, `base64_external_ref`, or a provider-declared encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Inline payload when the provider embedded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// Pointer to a sibling file when the capture was sanitized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<String>,
}

/// A field the adapter saw but deliberately did not normalize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnsupportedField {
    pub field: String,
    pub reason: String,
}

/// Everything an adapter can say about a raw artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderExtraction {
    pub provider: ProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<ExecutionIntent>,
    pub response: ProviderResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<RouteObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<UnsignedTransactionRef>,
    /// Present only when the artifact itself names a landed transaction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Provider-specific leftovers, namespaced. Core code must not branch here.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub extensions: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unsupported: Vec<UnsupportedField>,
}

impl ProviderExtraction {
    pub fn empty(provider: ProviderId) -> Self {
        Self {
            provider,
            surface: None,
            intent: None,
            response: ProviderResponse::default(),
            route: None,
            transaction: None,
            signature: None,
            extensions: BTreeMap::new(),
            unsupported: Vec::new(),
        }
    }

    pub fn push_unsupported(&mut self, field: impl Into<String>, reason: impl Into<String>) {
        self.unsupported.push(UnsupportedField {
            field: field.into(),
            reason: reason.into(),
        });
    }
}

pub trait ProviderAdapter {
    fn provider_id(&self) -> ProviderId;

    /// True when this adapter recognises the artifact's shape.
    fn detect(&self, raw: &RawProviderArtifact) -> bool;

    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction>;
}

pub fn adapter_for(provider: ProviderId) -> Box<dyn ProviderAdapter> {
    match provider {
        ProviderId::Dflow => Box::new(dflow::DflowAdapter),
        ProviderId::Jupiter => Box::new(jupiter::JupiterAdapter),
        ProviderId::Jtx => Box::new(jtx::JtxAdapter),
        ProviderId::Generic => Box::new(generic::GenericAdapter),
    }
}

/// Ordered detection: specific adapters first, generic last.
pub fn detect_adapter(raw: &RawProviderArtifact) -> Box<dyn ProviderAdapter> {
    // Order matters: JTX envelopes wrap a provider quote, so they are tested
    // before the shapes they wrap.
    let candidates: Vec<Box<dyn ProviderAdapter>> = vec![
        Box::new(jtx::JtxAdapter),
        Box::new(dflow::DflowAdapter),
        Box::new(jupiter::JupiterAdapter),
    ];
    for adapter in candidates {
        if adapter.detect(raw) {
            return adapter;
        }
    }
    Box::new(generic::GenericAdapter)
}

/// Extract with an explicit provider, or by detection when `provider` is None.
pub fn extract(
    provider: Option<ProviderId>,
    raw: &RawProviderArtifact,
) -> Result<ProviderExtraction> {
    let adapter = match provider {
        Some(p) => adapter_for(p),
        None => detect_adapter(raw),
    };
    adapter.extract(raw)
}

// ---- shared JSON helpers ------------------------------------------------

/// Read a field as a string, accepting JSON numbers so `"100"` and `100`
/// normalize the same way. Base-unit amounts are kept as strings throughout:
/// they routinely exceed f64's exact-integer range.
pub(crate) fn as_string(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(json_to_string)
}

pub(crate) fn json_to_string(x: &Value) -> Option<String> {
    x.as_str()
        .map(|s| s.to_string())
        .or_else(|| x.as_u64().map(|n| n.to_string()))
        .or_else(|| x.as_i64().map(|n| n.to_string()))
}

pub(crate) fn as_u32(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n as u32)
}

pub(crate) fn as_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

pub(crate) fn unknown_fields(body: &Value, known: &[&str]) -> Value {
    let mut ext = serde_json::Map::new();
    if let Some(obj) = body.as_object() {
        for (k, v) in obj {
            if !known.contains(&k.as_str()) {
                ext.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(ext)
}

/// Classify the `transaction` field of a response body.
pub(crate) fn transaction_ref(body: &Value, key: &str) -> UnsignedTransactionRef {
    match body.get(key) {
        None | Some(Value::Null) => UnsignedTransactionRef {
            present: false,
            encoding: None,
            payload: None,
            external_ref: None,
        },
        Some(Value::String(s)) if s.starts_with('<') => UnsignedTransactionRef {
            present: true,
            encoding: Some("base64_external_ref".into()),
            payload: None,
            external_ref: Some(s.clone()),
        },
        Some(Value::String(s)) => UnsignedTransactionRef {
            present: true,
            encoding: Some("base64".into()),
            payload: Some(s.clone()),
            external_ref: None,
        },
        Some(_) => UnsignedTransactionRef {
            present: true,
            encoding: None,
            payload: None,
            external_ref: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_round_trips() {
        for id in [
            ProviderId::Dflow,
            ProviderId::Jupiter,
            ProviderId::Jtx,
            ProviderId::Generic,
        ] {
            assert_eq!(ProviderId::parse(id.as_str()), Some(id));
        }
        assert_eq!(ProviderId::parse("nope"), None);
    }

    #[test]
    fn raw_artifact_hashes_its_own_bytes() {
        let bytes = br#"{"inputMint":"A"}"#;
        let raw = RawProviderArtifact::from_bytes(bytes).unwrap();
        assert_eq!(raw.sha256.unwrap(), sha256_bytes(bytes));
    }

    #[test]
    fn transaction_ref_distinguishes_absent_null_and_string() {
        let absent = transaction_ref(&serde_json::json!({}), "transaction");
        assert!(!absent.present);
        let null = transaction_ref(&serde_json::json!({ "transaction": null }), "transaction");
        assert!(!null.present);
        let s = transaction_ref(&serde_json::json!({ "transaction": "AQAB" }), "transaction");
        assert!(s.present);
        assert_eq!(s.payload.as_deref(), Some("AQAB"));
        let ext = transaction_ref(
            &serde_json::json!({ "transaction": "<see sibling file>" }),
            "transaction",
        );
        assert_eq!(ext.encoding.as_deref(), Some("base64_external_ref"));
        assert!(ext.payload.is_none());
    }
}
