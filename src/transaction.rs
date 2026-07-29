//! Decodes a base64-encoded Solana transaction (legacy or versioned).
//! Static analysis only -- never signs, never submits.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use solana_sdk::transaction::VersionedTransaction;

use crate::program_registry::{jito_tip_accounts, known_programs};

#[derive(Debug, Serialize)]
pub struct DecodedInstruction {
    pub index: usize,
    pub program_id: String,
    pub program_label: String,
    pub num_accounts: usize,
    pub data_len: usize,
    pub discriminator_hex: String,
    pub data_sha256: String,
    /// Raw account indexes into the *full* account vector (static keys
    /// followed by ALT-loaded addresses). Retained because a count alone
    /// cannot tell you which accounts an instruction actually references.
    pub account_indexes: Vec<u8>,
    /// Raw index of this instruction's program in the full account vector.
    pub program_id_index: u8,
}

#[derive(Debug, Serialize)]
pub struct AltReference {
    pub lookup_table_account: String,
    pub num_writable_indexes: usize,
    pub num_readonly_indexes: usize,
    /// The actual indexes into the lookup table's own address list. Only
    /// these entries are loaded by the transaction -- membership in the
    /// table is not the same as being loaded, and the difference is the
    /// whole point of resolving them.
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct DecodedTransaction {
    pub transaction_type: String,
    pub num_signature_slots: usize,
    pub fee_payer: Option<String>,
    pub recent_blockhash: String,
    pub static_account_keys: Vec<String>,
    pub address_lookup_table_references: Vec<AltReference>,
    pub instructions: Vec<DecodedInstruction>,
    pub candidate_jito_tip_transfers: Vec<usize>,
    pub unknown_program_ids: Vec<String>,
}

pub fn decode_base64_transaction(b64: &str) -> Result<DecodedTransaction> {
    let raw = STANDARD
        .decode(b64.trim())
        .context("invalid base64 transaction")?;
    let vtx: VersionedTransaction =
        bincode::deserialize(&raw).context("failed to deserialize as VersionedTransaction")?;

    let message = &vtx.message;
    let static_keys: Vec<String> = message
        .static_account_keys()
        .iter()
        .map(|k| k.to_string())
        .collect();

    let fee_payer = static_keys.first().cloned();
    let recent_blockhash = message.recent_blockhash().to_string();

    let alt_refs: Vec<AltReference> = message
        .address_table_lookups()
        .map(|luts| {
            luts.iter()
                .map(|l| AltReference {
                    lookup_table_account: l.account_key.to_string(),
                    num_writable_indexes: l.writable_indexes.len(),
                    num_readonly_indexes: l.readonly_indexes.len(),
                    writable_indexes: l.writable_indexes.clone(),
                    readonly_indexes: l.readonly_indexes.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let known = known_programs();
    let tip_accounts = jito_tip_accounts();

    let mut instructions = Vec::new();
    let mut unknown_programs = std::collections::BTreeSet::new();
    let mut jito_tip_hits = Vec::new();

    for (idx, ix) in message.instructions().iter().enumerate() {
        let program_idx = ix.program_id_index as usize;
        let program_id = static_keys
            .get(program_idx)
            .cloned()
            .unwrap_or_else(|| format!("<lookup-table-index-{}>", program_idx));
        let label = known.get(program_id.as_str()).copied().unwrap_or("unknown");
        if label == "unknown" {
            unknown_programs.insert(program_id.clone());
        }

        let mut hasher = Sha256::new();
        hasher.update(&ix.data);
        let data_hash = format!("{:x}", hasher.finalize());
        let discriminator_hex = ix
            .data
            .iter()
            .take(8)
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        if label == "system_program" {
            for &acct_idx in &ix.accounts {
                if let Some(key) = static_keys.get(acct_idx as usize) {
                    if tip_accounts.contains(&key.as_str()) {
                        jito_tip_hits.push(idx);
                    }
                }
            }
        }

        instructions.push(DecodedInstruction {
            index: idx,
            program_id,
            program_label: label.to_string(),
            num_accounts: ix.accounts.len(),
            data_len: ix.data.len(),
            discriminator_hex,
            data_sha256: data_hash,
            account_indexes: ix.accounts.clone(),
            program_id_index: ix.program_id_index,
        });
    }

    Ok(DecodedTransaction {
        transaction_type: if alt_refs.is_empty() {
            "legacy_or_v0_no_alt".to_string()
        } else {
            "v0_with_alt".to_string()
        },
        num_signature_slots: vtx.signatures.len(),
        fee_payer,
        recent_blockhash,
        static_account_keys: static_keys,
        address_lookup_table_references: alt_refs,
        instructions,
        candidate_jito_tip_transfers: jito_tip_hits,
        unknown_program_ids: unknown_programs.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use solana_sdk::{
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::{Transaction, VersionedTransaction},
    };
    use std::str::FromStr;

    #[allow(deprecated)]
    fn encode_transfer(from: &Keypair, to: &Pubkey, lamports: u64) -> String {
        use solana_sdk::system_instruction;
        let ix = system_instruction::transfer(&from.pubkey(), to, lamports);
        let message = Message::new(&[ix], Some(&from.pubkey()));
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = Hash::new_unique();
        let vtx = VersionedTransaction::from(tx);
        let raw = bincode::serialize(&vtx).expect("serialize VersionedTransaction");
        STANDARD.encode(raw)
    }

    #[test]
    fn rejects_invalid_base64() {
        let err = decode_base64_transaction("%%%not-base64%%%").unwrap_err();
        assert!(err.to_string().contains("invalid base64"));
    }

    #[test]
    fn rejects_valid_base64_that_is_not_a_transaction() {
        let junk = STANDARD.encode(b"not-a-solana-transaction");
        assert!(decode_base64_transaction(&junk).is_err());
    }

    #[test]
    fn decodes_legacy_system_transfer() {
        let from = Keypair::new();
        let to = Keypair::new();
        let b64 = encode_transfer(&from, &to.pubkey(), 42);
        let decoded = decode_base64_transaction(&b64).unwrap();

        assert_eq!(decoded.transaction_type, "legacy_or_v0_no_alt");
        assert_eq!(
            decoded.fee_payer.as_deref(),
            Some(from.pubkey().to_string()).as_deref()
        );
        assert!(decoded.address_lookup_table_references.is_empty());
        assert_eq!(decoded.instructions.len(), 1);
        assert_eq!(decoded.instructions[0].program_label, "system_program");
        assert!(decoded.unknown_program_ids.is_empty());
        assert!(decoded.candidate_jito_tip_transfers.is_empty());
    }

    #[test]
    fn flags_system_transfer_to_known_jito_tip_account() {
        let from = Keypair::new();
        let tip = Pubkey::from_str(jito_tip_accounts()[0]).unwrap();
        let b64 = encode_transfer(&from, &tip, 1000);
        let decoded = decode_base64_transaction(&b64).unwrap();

        assert_eq!(decoded.candidate_jito_tip_transfers, vec![0]);
        assert_eq!(decoded.instructions[0].program_label, "system_program");
    }
}
