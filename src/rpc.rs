//! Read-only Solana RPC helpers. No wallet, no signing, no submission.

use anyhow::{Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::signature::Signature;
use solana_transaction_status_client_types::UiTransactionEncoding;
use std::str::FromStr;

/// On-chain facts about one account, as reported by a read-only RPC call.
/// `None` for a whole entry means the account does not exist on chain --
/// normal for an address a transaction is about to create.
#[derive(Debug, Clone)]
pub struct AccountFacts {
    pub owner: String,
    pub executable: bool,
}

/// Fetch owner/executable for a batch of addresses.
///
/// This is what surfaces an integrator marker that a plain address scan
/// misses: a program can be absent from the account vector while still
/// *owning* an account inside it. Attribution has to follow ownership, not
/// just the addresses the transaction names directly.
pub async fn fetch_account_facts(
    rpc_url: &str,
    addresses: &[String],
) -> Result<Vec<Option<AccountFacts>>> {
    let client = RpcClient::new(rpc_url.to_string());
    let mut out = Vec::with_capacity(addresses.len());

    for chunk in addresses.chunks(100) {
        let keys: Vec<solana_sdk::pubkey::Pubkey> = chunk
            .iter()
            .map(|a| solana_sdk::pubkey::Pubkey::from_str(a))
            .collect::<std::result::Result<_, _>>()
            .context("invalid pubkey in account batch")?;

        let accounts = client
            .get_multiple_accounts(&keys)
            .await
            .context("getMultipleAccounts RPC failed")?;

        for acct in accounts {
            out.push(acct.map(|a| AccountFacts {
                owner: a.owner.to_string(),
                executable: a.executable,
            }));
        }
    }

    Ok(out)
}

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
