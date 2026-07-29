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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use solana_sdk::{
        hash::Hash,
        message::Message,
        signature::{Keypair, Signer},
        transaction::{Transaction, VersionedTransaction},
    };
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_transaction_base64_rejects_bad_signature() {
        let err = fetch_transaction_base64("http://127.0.0.1:9", "not-a-signature")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid transaction signature"));
    }

    #[tokio::test]
    async fn fetch_account_facts_rejects_bad_pubkey() {
        let err = fetch_account_facts("http://127.0.0.1:9", &["nope".into()])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid pubkey"));
    }

    #[tokio::test]
    async fn fetch_transaction_base64_reads_binary_encoding() {
        #[allow(deprecated)]
        let b64 = {
            use solana_sdk::system_instruction;
            let from = Keypair::new();
            let to = Keypair::new();
            let ix = system_instruction::transfer(&from.pubkey(), &to.pubkey(), 1);
            let message = Message::new(&[ix], Some(&from.pubkey()));
            let mut tx = Transaction::new_unsigned(message);
            tx.message.recent_blockhash = Hash::new_unique();
            let vtx = VersionedTransaction::from(tx);
            STANDARD.encode(bincode::serialize(&vtx).unwrap())
        };

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "slot": 1,
                "transaction": [b64, "base64"],
                "meta": null,
                "version": "legacy",
                "blockTime": null
            }
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        // Any syntactically valid signature string is fine; the mock ignores it.
        let sig = "1111111111111111111111111111111111111111111111111111111111111111";
        let got = fetch_transaction_base64(&server.uri(), sig).await.unwrap();
        assert!(!got.is_empty());
        assert!(crate::transaction::decode_base64_transaction(&got).is_ok());
    }

    #[tokio::test]
    async fn fetch_account_facts_maps_present_and_missing() {
        let server = MockServer::start().await;
        let system = "11111111111111111111111111111111";
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "context": { "slot": 1 },
                "value": [
                    {
                        "data": ["", "base64"],
                        "executable": false,
                        "lamports": 1,
                        "owner": system,
                        "rentEpoch": 0,
                        "space": 0
                    },
                    null
                ]
            }
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let a = Keypair::new().pubkey().to_string();
        let b = Keypair::new().pubkey().to_string();
        let facts = fetch_account_facts(&server.uri(), &[a, b]).await.unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].as_ref().unwrap().owner, system);
        assert!(!facts[0].as_ref().unwrap().executable);
        assert!(facts[1].is_none());
    }
}
