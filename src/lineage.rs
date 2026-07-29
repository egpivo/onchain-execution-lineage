//! Generates the field-lineage CSV: for each quote-level field, whether it
//! is present in the quote response, the serialized transaction, the
//! decoded transaction, and potentially visible after settlement.
//!
//! As of this writing, DFlow's no-key developer quote endpoint (confirmed
//! live, 2026-07-29) does not return a `transaction` field at all -- this
//! is itself the headline finding this module records, not an assumption.
//! The transaction-side columns for DFlow-specific fields are therefore
//! "not_applicable" (no transaction to check), not "not_yet_decoded" (which
//! would wrongly imply a transaction exists and just hasn't been examined
//! yet).

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct LineageRow<'a> {
    field: &'a str,
    present_in_quote_json: &'a str,
    present_in_serialized_transaction: &'a str,
    present_after_decoding: &'a str,
    potentially_visible_after_settlement: &'a str,
    attribution_value: &'a str,
    caveat: &'a str,
}

pub fn write_dev_endpoint_lineage(out_path: &PathBuf) -> Result<()> {
    let rows = vec![
        LineageRow {
            field: "request_id",
            present_in_quote_json: "yes",
            present_in_serialized_transaction: "not_applicable",
            present_after_decoding: "not_applicable",
            potentially_visible_after_settlement: "no",
            attribution_value: "none",
            caveat: "confirmed live 2026-07-29: dev-quote-api.dflow.net returns no transaction field at all; this is a quote-only surface, not an order/swap endpoint",
        },
        LineageRow {
            field: "route_plan (venue, marketKey)",
            present_in_quote_json: "yes",
            present_in_serialized_transaction: "not_applicable",
            present_after_decoding: "not_applicable",
            potentially_visible_after_settlement: "indirectly (marketKey may match an on-chain account)",
            attribution_value: "descriptive only from this surface",
            caveat: "route plan is DFlow's own claim in JSON; not independently verified against any transaction from this endpoint",
        },
        LineageRow {
            field: "platformFee",
            present_in_quote_json: "field exists, observed null in all live captures this session",
            present_in_serialized_transaction: "not_applicable",
            present_after_decoding: "not_applicable",
            potentially_visible_after_settlement: "unknown",
            attribution_value: "none observed",
            caveat: "unlike JTX's quote response, this dev endpoint's platformFee field was null in every live capture run this session -- distinct finding from the Sunday project's JTX-specific fee analysis",
        },
        LineageRow {
            field: "output amount / minOutAmount",
            present_in_quote_json: "yes",
            present_in_serialized_transaction: "not_applicable",
            present_after_decoding: "not_applicable",
            potentially_visible_after_settlement: "yes, if a matching transaction is separately obtained",
            attribution_value: "weak without a matched transaction",
            caveat: "this endpoint alone cannot be linked to a specific settled transaction; no request/transaction correlation mechanism observed",
        },
        LineageRow {
            field: "program IDs / accounts / ALT (general Solana transaction structure)",
            present_in_quote_json: "no",
            present_in_serialized_transaction: "yes (verified against real mainnet data)",
            present_after_decoding: "yes (verified against real mainnet data)",
            potentially_visible_after_settlement: "yes",
            attribution_value: "strong for provider/venue attribution once a real transaction is obtained",
            caveat: "decoder and ALT resolution verified this session against a real, finalized mainnet transaction (Jupiter aggregator v6), not synthetic data -- but no DFlow or JTX transaction has been run through it yet",
        },
    ];

    let mut wtr = csv::Writer::from_path(out_path)?;
    for row in rows {
        wtr.serialize(row)?;
    }
    wtr.flush()?;
    Ok(())
}
