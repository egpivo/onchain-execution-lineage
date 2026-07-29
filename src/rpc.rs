//! Read-only Solana RPC helpers. No wallet, no signing, no submission.

use anyhow::{Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::signature::Signature;
use solana_transaction_status_client_types::UiTransactionEncoding;
use std::str::FromStr;

/// Fetch a settled transaction's base64 payload via public RPC.
pub async fn fetch_transaction_base64(rpc_url: &str, signature_str: &str) -> Result<String> {
    let client = RpcClient::new(rpc_url.to_string());
    let sig = Signature::from_str(signature_str).context("invalid transaction signature")?;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        max_supported_transaction_version: Some(0),
        commitment: None,
    };
    let tx = client
        .get_transaction_with_config(&sig, config)
        .await
        .context("getTransaction RPC failed")?;
    let encoded = tx.transaction.transaction;
    match encoded {
        solana_transaction_status_client_types::EncodedTransaction::Binary(data, _) => Ok(data),
        _ => anyhow::bail!("expected base64-encoded transaction"),
    }
}
