//! Settlement enrichment via read-only RPC. Never infers JTX origin from
//! generic Jito infrastructure, and never treats priority fee as delivery.

use anyhow::{Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::signature::Signature;
use solana_transaction_status_client_types::UiTransactionEncoding;
use std::collections::BTreeSet;
use std::str::FromStr;

use crate::evidence::{AttributionClaim, EvidenceLevel};
use crate::lineage_model::LineageBundle;
use crate::program_registry::jito_tip_accounts;

pub async fn enrich_settlement(
    bundle: &mut LineageBundle,
    rpc_url: &str,
    signature: &str,
) -> Result<()> {
    let client = RpcClient::new(rpc_url.to_string());
    let sig = Signature::from_str(signature).context("invalid signature")?;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Json),
        max_supported_transaction_version: Some(0),
        commitment: None,
    };
    let tx = client
        .get_transaction_with_config(&sig, config)
        .await
        .context("getTransaction for settlement enrichment failed")?;

    bundle.settlement.applicable = true;
    bundle.settlement.signature = Some(signature.to_string());
    bundle.settlement.slot = Some(tx.slot);
    bundle.settlement.block_time = tx.block_time;

    if let Some(meta) = tx.transaction.meta {
        let err = meta.err;
        bundle.settlement.status = Some(if err.is_none() {
            "success".into()
        } else {
            format!("failed:{err:?}")
        });
        bundle.settlement.compute_units_consumed = meta.compute_units_consumed.into();

        let mut runtime = BTreeSet::new();
        let logs: Option<Vec<String>> = meta.log_messages.into();
        if let Some(logs) = logs {
            bundle
                .raw_extensions
                .insert("settlement_logs_len".into(), serde_json::json!(logs.len()));
            for line in &logs {
                if let Some(rest) = line.strip_prefix("Program ") {
                    if let Some(pid) = rest.split_whitespace().next() {
                        runtime.insert(pid.to_string());
                    }
                }
            }
        }
        bundle.settlement.runtime_program_set = runtime.into_iter().collect();

        // Tip-account match against static keys if we have a decoded view.
        let tips = jito_tip_accounts();
        if let Some(dec) = &bundle.decoded_transaction {
            for key in &dec.static_account_keys {
                if tips.contains(&key.as_str()) {
                    bundle.settlement.notes.push(format!(
                        "static key {key} matches a known Jito tip account — infrastructure evidence only"
                    ));
                }
            }
        }

        bundle.push_claim(
            AttributionClaim::new(
                "settlement",
                "has_status",
                bundle.settlement.status.clone().unwrap_or_default(),
                EvidenceLevel::ResolvedFromRpc,
                &bundle.capture.artifact_id,
                "transaction status from getTransaction meta",
            )
            .with_field("meta.err"),
        );
    } else {
        bundle
            .settlement
            .notes
            .push("transaction meta missing from RPC response".into());
    }

    Ok(())
}
