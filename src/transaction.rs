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
}

#[derive(Debug, Serialize)]
pub struct AltReference {
    pub lookup_table_account: String,
    pub num_writable_indexes: usize,
    pub num_readonly_indexes: usize,
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
    let raw = STANDARD.decode(b64.trim()).context("invalid base64 transaction")?;
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
        let label = known
            .get(program_id.as_str())
            .copied()
            .unwrap_or("unknown");
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
        });
    }

    Ok(DecodedTransaction {
        transaction_type: if alt_refs.is_empty() { "legacy_or_v0_no_alt".to_string() } else { "v0_with_alt".to_string() },
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
