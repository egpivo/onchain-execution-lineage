//! Jupiter adapter — deliberately partial.
//!
//! Only fields this repository has actually observed in a Jupiter artifact are
//! normalized. Everything else is reported through
//! [`ProviderExtraction::unsupported`] so downstream checks return UNKNOWN
//! rather than inventing parity with DFlow.
//!
//! Not supported today: order/quote surface distinction, execution mode,
//! prioritization metadata, platform-fee accounting, and the `minOutAmount` /
//! `otherAmountThreshold` split (Jupiter returns only the latter).

use anyhow::Result;
use serde_json::Value;

use super::{
    as_string, as_u32, as_u64, transaction_ref, ExecutionIntent, ProviderAdapter,
    ProviderExtraction, ProviderId, ProviderResponse, RawProviderArtifact,
};
use crate::lineage_model::{RouteLegObservation, RouteObservation};

const KNOWN_FIELDS: &[&str] = &[
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

pub struct JupiterAdapter;

impl JupiterAdapter {
    pub fn detects(body: &Value) -> bool {
        body.get("router").is_some()
            || (body.get("routePlan").is_some()
                && body.get("requestId").is_none()
                && body.get("swapMode").is_some())
    }
}

impl ProviderAdapter for JupiterAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Jupiter
    }

    fn detect(&self, raw: &RawProviderArtifact) -> bool {
        Self::detects(&raw.body)
    }

    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction> {
        let body = &raw.body;
        let mut out = ProviderExtraction::empty(ProviderId::Jupiter);
        out.surface = raw.surface.clone();

        out.response = ProviderResponse {
            input_mint: as_string(body, "inputMint"),
            output_mint: as_string(body, "outputMint"),
            in_amount: as_string(body, "inAmount"),
            out_amount: as_string(body, "outAmount"),
            // Jupiter has no separate minOutAmount field in any artifact this
            // repo has captured; leaving it None is the honest answer.
            min_out_amount: None,
            other_amount_threshold: as_string(body, "otherAmountThreshold"),
            slippage_bps: as_u32(body, "slippageBps"),
            price_impact_pct: as_string(body, "priceImpactPct"),
            context_slot: as_u64(body, "contextSlot"),
            request_or_quote_id: as_string(body, "quoteId")
                .or_else(|| as_string(body, "requestId")),
            execution_mode: None,
            last_valid_block_height: None,
            compute_unit_limit: None,
            prioritization_fee_lamports: None,
            platform_fee: None,
            error: as_string(body, "error"),
        };
        out.push_unsupported(
            "minOutAmount",
            "no Jupiter artifact in this repository carries a separate minimum-out field",
        );
        if body.get("platformFee").is_some() {
            out.push_unsupported(
                "platformFee",
                "Jupiter fee accounting is not verified against a captured artifact yet",
            );
        }

        let intent = ExecutionIntent {
            input_mint: out.response.input_mint.clone(),
            output_mint: out.response.output_mint.clone(),
            in_amount: out.response.in_amount.clone(),
            slippage_bps: out.response.slippage_bps,
            user_public_key: None,
            recovered_from: "provider_response_echo".into(),
        };
        if intent != ExecutionIntent::default() {
            out.intent = Some(intent);
        }

        if let Some(legs) = body.get("routePlan").and_then(|r| r.as_array()) {
            let mut route = RouteObservation {
                provider_route_label: as_string(body, "router"),
                legs: Vec::new(),
            };
            for leg in legs {
                // Classic quotes nest under swapInfo; Ultra flattens.
                let swap = leg.get("swapInfo").unwrap_or(leg);
                route.legs.push(RouteLegObservation {
                    venue_or_label: as_string(swap, "label")
                        .or_else(|| as_string(swap, "ammKey"))
                        .unwrap_or_else(|| "unknown".into()),
                    input_mint: as_string(swap, "inputMint"),
                    output_mint: as_string(swap, "outputMint"),
                    in_amount: as_string(swap, "inAmount"),
                    out_amount: as_string(swap, "outAmount"),
                    market_key: as_string(swap, "ammKey"),
                });
            }
            out.route = Some(route);
        }

        out.transaction = Some(transaction_ref(body, "transaction"));

        let leftovers = super::unknown_fields(body, KNOWN_FIELDS);
        if !leftovers.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            out.extensions.insert("jupiter".into(), leftovers);
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_only_the_supported_subset() {
        let body = serde_json::json!({
            "inputMint": "So11111111111111111111111111111111111111112",
            "inAmount": "1000000",
            "outputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "outAmount": "250000",
            "otherAmountThreshold": "248750",
            "swapMode": "ExactIn",
            "slippageBps": 50,
            "router": "metis",
            "routePlan": [{ "swapInfo": {
                "ammKey": "AMM1",
                "label": "Orca",
                "inputMint": "So11111111111111111111111111111111111111112",
                "outputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                "inAmount": "1000000",
                "outAmount": "250000"
            }}],
        });
        let raw = RawProviderArtifact::from_value(body);
        assert!(JupiterAdapter.detect(&raw));

        let e = JupiterAdapter.extract(&raw).unwrap();
        assert_eq!(e.provider, ProviderId::Jupiter);
        assert_eq!(e.response.other_amount_threshold.as_deref(), Some("248750"));
        assert!(e.response.min_out_amount.is_none());
        assert!(e.response.execution_mode.is_none());
        assert_eq!(
            e.route.as_ref().unwrap().provider_route_label.as_deref(),
            Some("metis")
        );
        assert_eq!(e.route.as_ref().unwrap().legs[0].venue_or_label, "Orca");
        assert!(e.unsupported.iter().any(|u| u.field == "minOutAmount"));
        assert!(!e.transaction.unwrap().present);
    }

    #[test]
    fn does_not_claim_dflow_shaped_responses() {
        let body = serde_json::json!({
            "routePlan": [],
            "requestId": "abc",
            "outAmount": "1",
        });
        assert!(!JupiterAdapter.detect(&RawProviderArtifact::from_value(body)));
    }
}
