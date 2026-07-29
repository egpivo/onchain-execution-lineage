//! Compile-checks the three Rust snippets published in the 2026-08-18
//! engineering article.
//!
//! The article abridges these (it omits some field assignments and error
//! plumbing for readability), but an abridged snippet should still be valid
//! Rust against this crate's real types -- otherwise the article is showing
//! pseudocode while claiming to show production code. If a type changes and
//! a published snippet stops compiling, this test fails and the article
//! needs updating.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use dflow_lineage::instruction_map::{InstructionAccountRef, LoadedAddress, MappedInstruction};
use dflow_lineage::transaction::{AltReference, DecodedInstruction, DecodedTransaction};
use solana_sdk::transaction::VersionedTransaction;

/// Snippet 1 -- decode the unsigned transaction.
#[test]
fn snippet_1_decode_compiles_and_runs() {
    use anyhow::Context;

    // A real, valid, unsigned legacy transaction to feed the snippet.
    let b64 = build_sample_transaction_b64();
    let alt_refs: Vec<AltReference> = Vec::new();

    let decode = || -> anyhow::Result<(Option<String>, &'static str)> {
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
        let transaction_type = if alt_refs.is_empty() {
            "legacy_or_v0_no_alt"
        } else {
            "v0_with_alt"
        };

        Ok((fee_payer, transaction_type))
    };

    let (fee_payer, transaction_type) = decode().expect("snippet 1 should decode");
    assert!(fee_payer.is_some());
    assert_eq!(transaction_type, "legacy_or_v0_no_alt");
}

/// Snippet 2 -- resolve loaded addresses.
#[test]
fn snippet_2_resolve_loaded_addresses_compiles_and_runs() {
    let decoded = sample_decoded_v0();
    let mut resolved_tables: BTreeMap<String, Vec<String>> = BTreeMap::new();
    resolved_tables.insert(
        "TABLE_A".to_string(),
        (0..251).map(|i| format!("A{i:03}")).collect(),
    );

    let mut vector: Vec<LoadedAddress> = Vec::new();

    for writable_pass in [true, false] {
        for alt in &decoded.address_lookup_table_references {
            let table = resolved_tables.get(&alt.lookup_table_account).unwrap();
            let indexes = if writable_pass {
                &alt.writable_indexes
            } else {
                &alt.readonly_indexes
            };
            for &table_index in indexes {
                let address = table.get(table_index as usize).unwrap();
                vector.push(LoadedAddress {
                    account_vector_index: vector.len(),
                    address: address.clone(),
                    writable: writable_pass,
                    lookup_table_index: Some(table_index),
                    ..Default::default()
                });
            }
        }
    }

    // writable first, then readonly -- the ordering the article claims.
    assert_eq!(vector.len(), 2);
    assert_eq!(vector[0].address, "A005");
    assert!(vector[0].writable);
    assert_eq!(vector[1].address, "A009");
    assert!(!vector[1].writable);
}

/// Snippet 3 -- map a compiled instruction.
#[test]
fn snippet_3_map_instruction_compiles_and_runs() {
    let decoded = sample_decoded_v0();
    let vector: Vec<LoadedAddress> = vec![
        LoadedAddress {
            account_vector_index: 0,
            address: "PAYER".to_string(),
            ..Default::default()
        },
        LoadedAddress {
            account_vector_index: 1,
            address: "PROGRAM".to_string(),
            label: "dflow_aggregator_v4".to_string(),
            ..Default::default()
        },
    ];

    let mut instructions: Vec<MappedInstruction> = Vec::new();

    for ix in &decoded.instructions {
        let program_slot = vector.get(ix.program_id_index as usize).unwrap();
        let mut accounts = Vec::new();

        for (pos, &acct_index) in ix.account_indexes.iter().enumerate() {
            let slot = vector.get(acct_index as usize).unwrap();
            accounts.push(InstructionAccountRef {
                position_in_instruction: pos,
                account_vector_index: acct_index as usize,
                address: slot.address.clone(),
                source: slot.source.clone(),
                writable: slot.writable,
                label: slot.label.clone(),
            });
        }

        instructions.push(MappedInstruction {
            instruction_index: ix.index,
            program_id: program_slot.address.clone(),
            program_label: program_slot.label.clone(),
            data_len: ix.data_len,
            discriminator_hex: ix.discriminator_hex.clone(),
            accounts,
        });
    }

    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0].program_label, "dflow_aggregator_v4");
    assert_eq!(instructions[0].accounts[0].address, "PAYER");
}

// ---- fixtures -------------------------------------------------------

fn build_sample_transaction_b64() -> String {
    use solana_sdk::{
        hash::Hash,
        message::Message,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

    let from = Keypair::new();
    let to = Keypair::new();
    #[allow(deprecated)]
    let ix = solana_sdk::system_instruction::transfer(&from.pubkey(), &to.pubkey(), 1);
    let message = Message::new(&[ix], Some(&from.pubkey()));
    let mut tx = Transaction::new_unsigned(message);
    tx.message.recent_blockhash = Hash::new_unique();
    let vtx = VersionedTransaction::from(tx);
    STANDARD.encode(bincode::serialize(&vtx).unwrap())
}

fn sample_decoded_v0() -> DecodedTransaction {
    DecodedTransaction {
        transaction_type: "v0_with_alt".to_string(),
        num_signature_slots: 1,
        fee_payer: Some("PAYER".to_string()),
        recent_blockhash: "BLOCKHASH".to_string(),
        static_account_keys: vec!["PAYER".to_string(), "PROGRAM".to_string()],
        address_lookup_table_references: vec![AltReference {
            lookup_table_account: "TABLE_A".to_string(),
            num_writable_indexes: 1,
            num_readonly_indexes: 1,
            writable_indexes: vec![5],
            readonly_indexes: vec![9],
        }],
        instructions: vec![DecodedInstruction {
            index: 0,
            program_id: "PROGRAM".to_string(),
            program_label: "dflow_aggregator_v4".to_string(),
            num_accounts: 1,
            data_len: 134,
            discriminator_hex: "f8c69e91e17587c8".to_string(),
            data_sha256: "x".to_string(),
            account_indexes: vec![0],
            program_id_index: 1,
        }],
        candidate_jito_tip_transfers: vec![],
        unknown_program_ids: vec![],
    }
}
