//! Resolves a decoded v0 transaction's *loaded* addresses and maps every
//! compiled instruction onto them.
//!
//! The distinction this module exists to enforce: an address lookup table
//! may hold 251 addresses, but a transaction loads only the handful its
//! `writable_indexes` / `readonly_indexes` actually name. Membership in a
//! table is not evidence that a transaction touches an account. Only an
//! address that is both loaded *and* referenced by a compiled instruction
//! is transaction-relevant.
//!
//! Account-vector ordering follows Solana's own rule for v0 messages:
//!   [static account keys] ++ [all ALT writable] ++ [all ALT readonly]
//! with ALT entries concatenated across tables in message order.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::program_registry::known_programs;
use crate::transaction::DecodedTransaction;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoadedAddress {
    /// Position in the final account vector -- the index instructions use.
    pub account_vector_index: usize,
    pub address: String,
    /// "static", "alt_writable", or "alt_readonly".
    pub source: String,
    pub lookup_table_account: Option<String>,
    /// Index within the lookup table's own address list, when applicable.
    pub lookup_table_index: Option<u8>,
    pub writable: bool,
    pub label: String,
    /// Compiled instructions that reference this address as an account.
    pub referenced_by_instructions: Vec<usize>,
    /// Compiled instructions that invoke this address as their program.
    pub is_program_for_instructions: Vec<usize>,
    /// On-chain owner program, when the account exists. An account's owner
    /// can carry attribution the account's own address does not.
    pub owner_program: Option<String>,
    pub owner_label: Option<String>,
    pub executable: Option<bool>,
    pub exists_on_chain: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionAccountRef {
    pub position_in_instruction: usize,
    pub account_vector_index: usize,
    pub address: String,
    pub source: String,
    pub writable: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappedInstruction {
    pub instruction_index: usize,
    pub program_id: String,
    pub program_label: String,
    pub data_len: usize,
    pub discriminator_hex: String,
    pub accounts: Vec<InstructionAccountRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionAccountMap {
    pub total_static_keys: usize,
    pub total_loaded_from_alts: usize,
    pub total_account_vector_len: usize,
    /// Sum of every referenced table's full address count -- reported only
    /// to contrast with `total_loaded_from_alts`.
    pub total_addresses_in_referenced_tables: usize,
    pub loaded_addresses: Vec<LoadedAddress>,
    pub instructions: Vec<MappedInstruction>,
}

/// Builds the full account vector and per-instruction account map.
///
/// `resolved_tables` maps a lookup-table account to its complete on-chain
/// address list, in storage order (as returned by
/// `lookup_tables::resolve_lookup_table`).
pub fn build_instruction_account_map(
    decoded: &DecodedTransaction,
    resolved_tables: &BTreeMap<String, Vec<String>>,
) -> Result<InstructionAccountMap> {
    let known = known_programs();
    let label_for = |addr: &str| -> String {
        known
            .get(addr)
            .copied()
            .unwrap_or("unclassified")
            .to_string()
    };

    let mut vector: Vec<LoadedAddress> = Vec::new();

    for (i, key) in decoded.static_account_keys.iter().enumerate() {
        vector.push(LoadedAddress {
            account_vector_index: i,
            address: key.clone(),
            source: "static".to_string(),
            lookup_table_account: None,
            lookup_table_index: None,
            // Writability of static keys depends on the message header,
            // which this decode does not retain; reported as unknown-false
            // rather than guessed.
            writable: false,
            label: label_for(key),
            referenced_by_instructions: Vec::new(),
            is_program_for_instructions: Vec::new(),
            owner_program: None,
            owner_label: None,
            executable: None,
            exists_on_chain: None,
        });
    }

    let mut total_addresses_in_referenced_tables = 0usize;

    // Solana loads all writable ALT addresses first (across every table in
    // message order), then all readonly ones. Building them in any other
    // order silently misattributes every instruction account index.
    for writable_pass in [true, false] {
        for alt in &decoded.address_lookup_table_references {
            let table = resolved_tables
                .get(&alt.lookup_table_account)
                .with_context(|| {
                    format!(
                        "no resolved address list for lookup table {}",
                        alt.lookup_table_account
                    )
                })?;

            if writable_pass {
                total_addresses_in_referenced_tables += table.len();
            }

            let indexes = if writable_pass {
                &alt.writable_indexes
            } else {
                &alt.readonly_indexes
            };

            for &table_index in indexes {
                let address = table.get(table_index as usize).with_context(|| {
                    format!(
                        "lookup table {} has {} addresses but index {} was requested",
                        alt.lookup_table_account,
                        table.len(),
                        table_index
                    )
                })?;

                vector.push(LoadedAddress {
                    account_vector_index: vector.len(),
                    address: address.clone(),
                    source: if writable_pass {
                        "alt_writable".to_string()
                    } else {
                        "alt_readonly".to_string()
                    },
                    lookup_table_account: Some(alt.lookup_table_account.clone()),
                    lookup_table_index: Some(table_index),
                    writable: writable_pass,
                    label: label_for(address),
                    referenced_by_instructions: Vec::new(),
                    is_program_for_instructions: Vec::new(),
                    owner_program: None,
                    owner_label: None,
                    executable: None,
                    exists_on_chain: None,
                });
            }
        }
    }

    let total_static_keys = decoded.static_account_keys.len();
    let total_loaded_from_alts = vector.len() - total_static_keys;

    let mut instructions = Vec::new();

    for ix in &decoded.instructions {
        let program_slot = vector.get(ix.program_id_index as usize).with_context(|| {
            format!(
                "instruction {} names program index {} but the account vector has {} entries",
                ix.index,
                ix.program_id_index,
                vector.len()
            )
        })?;
        let program_id = program_slot.address.clone();
        let program_label = program_slot.label.clone();

        let mut accounts = Vec::new();
        for (pos, &acct_index) in ix.account_indexes.iter().enumerate() {
            let slot = vector.get(acct_index as usize).with_context(|| {
                format!(
                    "instruction {} references account index {} but the account vector has {} entries",
                    ix.index,
                    acct_index,
                    vector.len()
                )
            })?;
            accounts.push(InstructionAccountRef {
                position_in_instruction: pos,
                account_vector_index: acct_index as usize,
                address: slot.address.clone(),
                source: slot.source.clone(),
                writable: slot.writable,
                label: slot.label.clone(),
            });
        }

        vector[ix.program_id_index as usize]
            .is_program_for_instructions
            .push(ix.index);
        for &acct_index in &ix.account_indexes {
            let refs = &mut vector[acct_index as usize].referenced_by_instructions;
            if !refs.contains(&ix.index) {
                refs.push(ix.index);
            }
        }

        instructions.push(MappedInstruction {
            instruction_index: ix.index,
            program_id,
            program_label,
            data_len: ix.data_len,
            discriminator_hex: ix.discriminator_hex.clone(),
            accounts,
        });
    }

    Ok(InstructionAccountMap {
        total_static_keys,
        total_loaded_from_alts,
        total_account_vector_len: vector.len(),
        total_addresses_in_referenced_tables,
        loaded_addresses: vector,
        instructions,
    })
}

/// Annotates each loaded address with its on-chain owner and executability.
///
/// `facts` must be positionally aligned with `map.loaded_addresses`.
pub fn annotate_with_account_facts(
    map: &mut InstructionAccountMap,
    facts: &[Option<crate::rpc::AccountFacts>],
) {
    let known = known_programs();
    for (addr, fact) in map.loaded_addresses.iter_mut().zip(facts) {
        match fact {
            Some(f) => {
                addr.owner_label = Some(
                    known
                        .get(f.owner.as_str())
                        .copied()
                        .unwrap_or("unclassified")
                        .to_string(),
                );
                addr.owner_program = Some(f.owner.clone());
                addr.executable = Some(f.executable);
                addr.exists_on_chain = Some(true);
            }
            None => {
                addr.exists_on_chain = Some(false);
            }
        }
    }
}

/// Addresses whose *owner* carries a label the address itself does not.
/// This is the attribution path that finds an integrator program which
/// never appears in the account vector directly.
pub fn owner_derived_markers(map: &InstructionAccountMap, prefix: &str) -> Vec<(String, String)> {
    map.loaded_addresses
        .iter()
        .filter(|a| {
            a.owner_label
                .as_deref()
                .is_some_and(|l| l.starts_with(prefix))
        })
        .map(|a| {
            (
                a.address.clone(),
                a.owner_program.clone().unwrap_or_default(),
            )
        })
        .collect()
}

/// True only when an address is loaded *and* referenced by at least one
/// compiled instruction. Mere presence in the account vector is not enough,
/// and mere membership in a lookup table is not even that.
pub fn is_transaction_relevant(addr: &LoadedAddress) -> bool {
    !addr.referenced_by_instructions.is_empty() || !addr.is_program_for_instructions.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{AltReference, DecodedInstruction, DecodedTransaction};

    fn table(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i:03}")).collect()
    }

    fn fixture() -> (DecodedTransaction, BTreeMap<String, Vec<String>>) {
        let decoded = DecodedTransaction {
            transaction_type: "v0_with_alt".to_string(),
            num_signature_slots: 1,
            fee_payer: Some("PAYER".to_string()),
            recent_blockhash: "BLOCKHASH".to_string(),
            static_account_keys: vec!["PAYER".to_string(), "PROGRAM".to_string()],
            address_lookup_table_references: vec![
                AltReference {
                    lookup_table_account: "TABLE_A".to_string(),
                    num_writable_indexes: 1,
                    num_readonly_indexes: 1,
                    writable_indexes: vec![5],
                    readonly_indexes: vec![9],
                },
                AltReference {
                    lookup_table_account: "TABLE_B".to_string(),
                    num_writable_indexes: 1,
                    num_readonly_indexes: 0,
                    writable_indexes: vec![2],
                    readonly_indexes: vec![],
                },
            ],
            instructions: vec![DecodedInstruction {
                index: 0,
                program_id: "PROGRAM".to_string(),
                program_label: "unclassified".to_string(),
                num_accounts: 2,
                data_len: 4,
                discriminator_hex: "deadbeef".to_string(),
                data_sha256: "x".to_string(),
                account_indexes: vec![2, 4],
                program_id_index: 1,
            }],
            candidate_jito_tip_transfers: vec![],
            unknown_program_ids: vec![],
        };

        let mut tables = BTreeMap::new();
        tables.insert("TABLE_A".to_string(), table("A", 251));
        tables.insert("TABLE_B".to_string(), table("B", 10));
        (decoded, tables)
    }

    #[test]
    fn orders_writable_before_readonly_across_tables() {
        let (decoded, tables) = fixture();
        let map = build_instruction_account_map(&decoded, &tables).unwrap();

        // static(2) ++ writable(A005, B002) ++ readonly(A009)
        let addrs: Vec<&str> = map
            .loaded_addresses
            .iter()
            .map(|a| a.address.as_str())
            .collect();
        assert_eq!(addrs, vec!["PAYER", "PROGRAM", "A005", "B002", "A009"]);
        assert_eq!(map.total_static_keys, 2);
        assert_eq!(map.total_loaded_from_alts, 3);
        assert_eq!(map.total_account_vector_len, 5);
    }

    #[test]
    fn loaded_count_is_far_smaller_than_table_membership() {
        let (decoded, tables) = fixture();
        let map = build_instruction_account_map(&decoded, &tables).unwrap();
        assert_eq!(map.total_addresses_in_referenced_tables, 261);
        assert_eq!(map.total_loaded_from_alts, 3);
    }

    #[test]
    fn maps_instruction_accounts_to_resolved_addresses() {
        let (decoded, tables) = fixture();
        let map = build_instruction_account_map(&decoded, &tables).unwrap();
        let ix = &map.instructions[0];
        assert_eq!(ix.program_id, "PROGRAM");
        assert_eq!(ix.accounts[0].address, "A005");
        assert!(ix.accounts[0].writable);
        assert_eq!(ix.accounts[1].address, "A009");
        assert!(!ix.accounts[1].writable);
    }

    #[test]
    fn unreferenced_loaded_address_is_not_transaction_relevant() {
        let (decoded, tables) = fixture();
        let map = build_instruction_account_map(&decoded, &tables).unwrap();
        // B002 is loaded but no instruction references it.
        let b002 = map
            .loaded_addresses
            .iter()
            .find(|a| a.address == "B002")
            .unwrap();
        assert!(b002.referenced_by_instructions.is_empty());
        assert!(!is_transaction_relevant(b002));

        let a005 = map
            .loaded_addresses
            .iter()
            .find(|a| a.address == "A005")
            .unwrap();
        assert!(is_transaction_relevant(a005));
    }

    #[test]
    fn rejects_index_beyond_table_length() {
        let (mut decoded, tables) = fixture();
        decoded.address_lookup_table_references[1].writable_indexes = vec![99];
        assert!(build_instruction_account_map(&decoded, &tables).is_err());
    }

    #[test]
    fn rejects_missing_resolved_table() {
        let (decoded, mut tables) = fixture();
        tables.remove("TABLE_B");
        assert!(build_instruction_account_map(&decoded, &tables).is_err());
    }

    #[test]
    fn annotate_with_account_facts_sets_owner_and_missing() {
        let (decoded, tables) = fixture();
        let mut map = build_instruction_account_map(&decoded, &tables).unwrap();
        let system = "11111111111111111111111111111111".to_string();
        let facts = vec![
            Some(crate::rpc::AccountFacts {
                owner: system.clone(),
                executable: false,
            }),
            None,
        ];
        // facts shorter than vector is fine for zip -- only first two annotated
        annotate_with_account_facts(&mut map, &facts);
        assert_eq!(
            map.loaded_addresses[0].owner_program.as_deref(),
            Some(system.as_str())
        );
        assert_eq!(
            map.loaded_addresses[0].owner_label.as_deref(),
            Some("system_program")
        );
        assert_eq!(map.loaded_addresses[0].exists_on_chain, Some(true));
        assert_eq!(map.loaded_addresses[1].exists_on_chain, Some(false));
    }

    #[test]
    fn owner_derived_markers_match_prefix() {
        let (decoded, tables) = fixture();
        let mut map = build_instruction_account_map(&decoded, &tables).unwrap();
        map.loaded_addresses[0].owner_label = Some("candidate_integrator_program_x".into());
        map.loaded_addresses[0].owner_program = Some("OWNER_X".into());
        map.loaded_addresses[1].owner_label = Some("system_program".into());

        let hits = owner_derived_markers(&map, "candidate_integrator_program");
        assert_eq!(hits, vec![("PAYER".into(), "OWNER_X".into())]);
    }
}
