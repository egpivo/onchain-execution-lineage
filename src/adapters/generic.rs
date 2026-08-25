//! Generic adapter — makes the verifier useful before a dedicated adapter exists.
//!
//! Accepts two shapes:
//!
//! 1. An artifact already written in this crate's neutral vocabulary
//!    (`{"intent":…, "response":…, "route":…, "transaction":…, "signature":…}`),
//!    typically produced by a caller that did its own normalization.
//! 2. Anything else. Snake_case and camelCase neutral-looking keys are read
//!    where they are unambiguous; the whole body is preserved under
//!    `extensions["generic"]` and the gaps are declared as unsupported.
//!
//! The generic adapter never guesses provider semantics. If a field's meaning
//! depends on which provider produced it, it is left unset.

use anyhow::Result;
use serde_json::Value;

use super::{
    as_string, as_u32, as_u64, transaction_ref, ExecutionIntent, ProviderAdapter,
    ProviderExtraction, ProviderId, ProviderResponse, RawProviderArtifact, UnsignedTransactionRef,
};
use crate::lineage_model::{RouteLegObservation, RouteObservation};

pub struct GenericAdapter;

/// First present key wins, so both casings are accepted without preferring one.
fn first_string(body: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| as_string(body, k))
}

impl ProviderAdapter for GenericAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Generic
    }

    fn detect(&self, _raw: &RawProviderArtifact) -> bool {
        true
    }

    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction> {
        let body = &raw.body;
        let mut out = ProviderExtraction::empty(ProviderId::Generic);
        out.surface = raw.surface.clone();

        // Shape 1: pre-normalized envelope.
        let pre_normalized = body.get("response").is_some()
            && (body.get("intent").is_some()
                || body.get("route").is_some()
                || body.get("transaction").is_some());

        if pre_normalized {
            if let Some(v) = body.get("intent") {
                out.intent = serde_json::from_value(v.clone()).ok();
            }
            if let Some(v) = body.get("response") {
                out.response = serde_json::from_value(v.clone()).unwrap_or_default();
            }
            if let Some(v) = body.get("route") {
                out.route = serde_json::from_value(v.clone()).ok();
            }
            out.transaction = match body.get("transaction") {
                Some(Value::String(_)) | None | Some(Value::Null) => {
                    Some(transaction_ref(body, "transaction"))
                }
                Some(v) => serde_json::from_value::<UnsignedTransactionRef>(v.clone()).ok(),
            };
            out.signature = as_string(body, "signature");
            return Ok(out);
        }

        // Shape 2: minimally described input.
        out.response = ProviderResponse {
            input_mint: first_string(body, &["input_mint", "inputMint"]),
            output_mint: first_string(body, &["output_mint", "outputMint"]),
            in_amount: first_string(body, &["in_amount", "inAmount"]),
            out_amount: first_string(body, &["out_amount", "outAmount"]),
            min_out_amount: first_string(body, &["min_out_amount", "minOutAmount"]),
            other_amount_threshold: first_string(
                body,
                &["other_amount_threshold", "otherAmountThreshold"],
            ),
            slippage_bps: as_u32(body, "slippage_bps").or_else(|| as_u32(body, "slippageBps")),
            price_impact_pct: None,
            context_slot: as_u64(body, "context_slot").or_else(|| as_u64(body, "contextSlot")),
            request_or_quote_id: first_string(body, &["request_id", "requestId", "quoteId"]),
            execution_mode: None,
            last_valid_block_height: None,
            compute_unit_limit: None,
            prioritization_fee_lamports: None,
            // Fee semantics are provider-defined; a generic adapter cannot read them.
            platform_fee: None,
            error: as_string(body, "error"),
        };

        let intent = ExecutionIntent {
            input_mint: out.response.input_mint.clone(),
            output_mint: out.response.output_mint.clone(),
            in_amount: out.response.in_amount.clone(),
            slippage_bps: out.response.slippage_bps,
            user_public_key: first_string(body, &["user_public_key", "userPublicKey"]),
            recovered_from: "generic_field_names".into(),
        };
        if intent != ExecutionIntent::default() {
            out.intent = Some(intent);
        }

        if let Some(legs) = body
            .get("route")
            .or_else(|| body.get("routePlan"))
            .or_else(|| body.get("route_plan"))
            .and_then(|r| r.as_array())
        {
            let mut route = RouteObservation::default();
            for leg in legs {
                route.legs.push(RouteLegObservation {
                    venue_or_label: first_string(leg, &["venue", "label", "venue_or_label"])
                        .unwrap_or_else(|| "unknown".into()),
                    input_mint: first_string(leg, &["input_mint", "inputMint"]),
                    output_mint: first_string(leg, &["output_mint", "outputMint"]),
                    in_amount: first_string(leg, &["in_amount", "inAmount"]),
                    out_amount: first_string(leg, &["out_amount", "outAmount"]),
                    market_key: first_string(leg, &["market_key", "marketKey"]),
                });
            }
            out.route = Some(route);
        }

        out.transaction = Some(transaction_ref(body, "transaction"));
        out.signature = as_string(body, "signature");

        out.push_unsupported(
            "provider_semantics",
            "no dedicated adapter matched; only unambiguous neutral field names were read",
        );
        out.extensions.insert("generic".into(), body.clone());

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_minimally_described_input() {
        let body = serde_json::json!({
            "input_mint": "A",
            "output_mint": "B",
            "in_amount": "100",
            "out_amount": "200",
            "slippage_bps": 50,
            "transaction": "AQAB",
            "signature": "sig1",
            "route": [{ "venue": "SomeAmm" }],
        });
        let e = GenericAdapter
            .extract(&RawProviderArtifact::from_value(body))
            .unwrap();

        assert_eq!(e.provider, ProviderId::Generic);
        assert_eq!(e.response.in_amount.as_deref(), Some("100"));
        assert_eq!(e.route.unwrap().legs[0].venue_or_label, "SomeAmm");
        assert_eq!(e.transaction.unwrap().payload.as_deref(), Some("AQAB"));
        assert_eq!(e.signature.as_deref(), Some("sig1"));
        assert!(e
            .unsupported
            .iter()
            .any(|u| u.field == "provider_semantics"));
        // Nothing is dropped.
        assert!(e.extensions.contains_key("generic"));
    }

    #[test]
    fn accepts_pre_normalized_envelope() {
        let body = serde_json::json!({
            "intent": { "input_mint": "A", "recovered_from": "caller_supplied" },
            "response": { "out_amount": "200", "other_amount_threshold": "199" },
            "route": { "provider_route_label": "x", "legs": [] },
            "transaction": "AQAB",
        });
        let e = GenericAdapter
            .extract(&RawProviderArtifact::from_value(body))
            .unwrap();

        assert_eq!(e.intent.unwrap().recovered_from, "caller_supplied");
        assert_eq!(e.response.other_amount_threshold.as_deref(), Some("199"));
        assert_eq!(e.route.unwrap().provider_route_label.as_deref(), Some("x"));
        // A pre-normalized caller is trusted to have done its own normalization,
        // so nothing is declared unsupported.
        assert!(e.unsupported.is_empty());
    }
}
