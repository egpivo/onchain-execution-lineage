//! Deterministic DFlow route fingerprints for bracketed construction experiments.
//!
//! Fingerprint definition is frozen here before inspecting live results.
//! Leg order is preserved (never sorted).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// One leg contribution to the route fingerprint, in original routePlan order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteLegFingerprint {
    pub venue: String,
    pub market_key: String,
    pub input_mint: String,
    pub output_mint: String,
    /// Share of top-level `inAmount` in basis points (floor).
    /// DFlow RoutePlanLeg has no separate allocationPct field; `inAmount`
    /// share is the documented allocation proxy used here.
    pub allocation_bps: u64,
    /// Raw leg inAmount retained so allocation ties can still be distinguished.
    pub in_amount: String,
    /// `dynamic` when leg includes `data`; otherwise `single_market`.
    pub route_leg_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteFingerprint {
    pub schema_version: String,
    pub legs: Vec<RouteLegFingerprint>,
    /// Canonical string form (leg order preserved).
    pub canonical: String,
    pub sha256: String,
}

pub const ROUTE_FINGERPRINT_SCHEMA: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteStabilityClass {
    ExactRouteStable,
    SameVenuesDifferentMarketKeys,
    SameMarketsDifferentAllocation,
    FullyDifferentRoute,
}

impl RouteStabilityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactRouteStable => "exact_route_stable",
            Self::SameVenuesDifferentMarketKeys => "same_venues_different_market_keys",
            Self::SameMarketsDifferentAllocation => "same_markets_different_allocation",
            Self::FullyDifferentRoute => "fully_different_route",
        }
    }
}

/// Build a route fingerprint from a DFlow order/quote JSON body.
///
/// Includes, in original order: venue, marketKey, input/output mint,
/// allocation_bps (from inAmount share), raw inAmount, and leg type.
pub fn fingerprint_route_plan(json: &Value) -> Option<RouteFingerprint> {
    let legs_json = json.get("routePlan")?.as_array()?;
    if legs_json.is_empty() {
        return None;
    }
    let top_in =
        parse_u128(json.get("inAmount").and_then(|v| v.as_str()).unwrap_or("0")).unwrap_or(0);

    let mut legs = Vec::new();
    for leg in legs_json {
        let in_amount = leg
            .get("inAmount")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let in_u = parse_u128(&in_amount).unwrap_or(0);
        let allocation_bps = if top_in == 0 {
            0
        } else {
            u64::try_from(in_u.saturating_mul(10_000) / top_in).unwrap_or(u64::MAX)
        };
        let route_leg_type = if leg.get("data").is_some() {
            "dynamic".to_string()
        } else {
            "single_market".to_string()
        };
        legs.push(RouteLegFingerprint {
            venue: leg
                .get("venue")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            market_key: leg
                .get("marketKey")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            input_mint: leg
                .get("inputMint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            output_mint: leg
                .get("outputMint")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            allocation_bps,
            in_amount,
            route_leg_type,
        });
    }

    let canonical = legs
        .iter()
        .map(|l| {
            format!(
                "{}|{}|{}|{}|{}bps|{}|{}",
                l.venue,
                l.market_key,
                l.input_mint,
                l.output_mint,
                l.allocation_bps,
                l.in_amount,
                l.route_leg_type
            )
        })
        .collect::<Vec<_>>()
        .join("||");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let sha256 = format!("{:x}", hasher.finalize());

    Some(RouteFingerprint {
        schema_version: ROUTE_FINGERPRINT_SCHEMA.into(),
        legs,
        canonical,
        sha256,
    })
}

pub fn classify_route_pair(a: &RouteFingerprint, b: &RouteFingerprint) -> RouteStabilityClass {
    if a.canonical == b.canonical {
        return RouteStabilityClass::ExactRouteStable;
    }
    let venues_a: Vec<_> = a.legs.iter().map(|l| l.venue.as_str()).collect();
    let venues_b: Vec<_> = b.legs.iter().map(|l| l.venue.as_str()).collect();
    let markets_a: Vec<_> = a
        .legs
        .iter()
        .map(|l| {
            (
                l.venue.as_str(),
                l.market_key.as_str(),
                l.input_mint.as_str(),
                l.output_mint.as_str(),
            )
        })
        .collect();
    let markets_b: Vec<_> = b
        .legs
        .iter()
        .map(|l| {
            (
                l.venue.as_str(),
                l.market_key.as_str(),
                l.input_mint.as_str(),
                l.output_mint.as_str(),
            )
        })
        .collect();

    if venues_a == venues_b && markets_a != markets_b {
        return RouteStabilityClass::SameVenuesDifferentMarketKeys;
    }
    if markets_a == markets_b {
        return RouteStabilityClass::SameMarketsDifferentAllocation;
    }
    RouteStabilityClass::FullyDifferentRoute
}

fn parse_u128(s: &str) -> Option<u128> {
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_leg_order_and_allocation_bps() {
        let body = json!({
            "inAmount": "100",
            "routePlan": [
                {
                    "venue": "A",
                    "marketKey": "m1",
                    "inputMint": "in",
                    "outputMint": "out",
                    "inAmount": "40",
                    "outAmount": "1"
                },
                {
                    "venue": "B",
                    "marketKey": "m2",
                    "inputMint": "in",
                    "outputMint": "out",
                    "inAmount": "60",
                    "outAmount": "2",
                    "data": "abcd"
                }
            ]
        });
        let fp = fingerprint_route_plan(&body).unwrap();
        assert_eq!(fp.legs[0].allocation_bps, 4000);
        assert_eq!(fp.legs[1].allocation_bps, 6000);
        assert_eq!(fp.legs[0].route_leg_type, "single_market");
        assert_eq!(fp.legs[1].route_leg_type, "dynamic");
        assert!(fp.canonical.starts_with("A|m1|"));
        assert!(fp.canonical.contains("||B|m2|"));
    }

    #[test]
    fn classifies_same_markets_different_allocation() {
        let a = fingerprint_route_plan(&json!({
            "inAmount": "100",
            "routePlan": [{
                "venue": "V", "marketKey": "M", "inputMint": "i", "outputMint": "o",
                "inAmount": "100", "outAmount": "1"
            }]
        }))
        .unwrap();
        let b = fingerprint_route_plan(&json!({
            "inAmount": "100",
            "routePlan": [{
                "venue": "V", "marketKey": "M", "inputMint": "i", "outputMint": "o",
                "inAmount": "90", "outAmount": "1"
            }]
        }))
        .unwrap();
        assert_eq!(
            classify_route_pair(&a, &b),
            RouteStabilityClass::SameMarketsDifferentAllocation
        );
    }
}
