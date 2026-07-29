//! Fixture-backed quote parsing tests (no network).

use dflow_lineage::models::DFlowQuoteResponse;

// Live capture from 2026-07-29 against dev-quote-api.dflow.net.
// Raw body has no `transaction` key at all (quote-only surface).
const LIVE_QUOTE_FIXTURE: &str = include_str!("fixtures/dev_quote_usdc_sol_no_tx.json");

#[test]
fn live_fixture_has_no_transaction_key() {
    let raw: serde_json::Value = serde_json::from_str(LIVE_QUOTE_FIXTURE).unwrap();
    assert!(
        !raw.as_object().unwrap().contains_key("transaction"),
        "fixture must preserve the live finding: no transaction key in raw JSON"
    );
}

#[test]
fn live_fixture_deserializes_as_quote_only() {
    let parsed: DFlowQuoteResponse = serde_json::from_str(LIVE_QUOTE_FIXTURE).unwrap();
    assert!(parsed.transaction.is_none());
    assert!(parsed.platform_fee.is_none());
    assert_eq!(parsed.request_id, "332e8a00-0a5f-4266-a139-d657227e0dbf");
    assert_eq!(parsed.route_plan.len(), 1);
    assert_eq!(parsed.route_plan[0].venue, "Tessera V");
    assert_eq!(
        parsed.route_plan[0].market_key,
        "FLckHLGMJy5gEoXWwcE68Nprde1D4araK4TGLw4pQq2n"
    );
    assert_eq!(parsed.out_amount, "13612487765");
    assert_eq!(parsed.slippage_bps, 50);
    assert!(!parsed.for_jito_bundle);
}
