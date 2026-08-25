//! Generic Solana extraction.
//!
//! Provider-independent by construction: nothing in this module knows whether
//! the bytes came from DFlow, Jupiter, JTX or a file on disk. It wraps the
//! existing primitives rather than reimplementing them —
//! [`crate::transaction`] for decoding, [`crate::lookup_tables`] for ALT
//! fetches, [`crate::instruction_map`] for loaded-account ordering,
//! [`crate::program_registry`] for program labels, [`crate::rpc`] for account
//! facts and settled transactions.
//!
//! Input: unsigned transaction bytes/base64 plus an optional RPC context.
//! Output: [`TransactionObservation`] — what this chain's encoding actually
//! shows. It is deliberately named apart from
//! [`crate::lineage_model::TransactionConstruction`], which is the normalized
//! cross-layer summary that ends up in a lineage bundle: one is a Solana
//! observation, the other is a chain-agnostic stage of the lineage.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use solana_sdk::message::VersionedMessage;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::BTreeMap;

use crate::instruction_map::{self, InstructionAccountMap};
use crate::lookup_tables;
use crate::transaction::{self, DecodedTransaction};

/// The message version actually encoded in the bytes.
///
/// Distinct from [`DecodedTransaction::transaction_type`], which reports ALT
/// *presence* (`legacy_or_v0_no_alt` / `v0_with_alt`) and cannot separate a
/// legacy message from a v0 message that happens to reference no tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionVersion {
    Legacy,
    V0,
}

impl TransactionVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionVersion::Legacy => "legacy",
            TransactionVersion::V0 => "v0",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AltResolution {
    pub tables_referenced: Vec<String>,
    pub tables_resolved: Vec<String>,
    /// `(lookup_table_account, error)` for tables that could not be fetched.
    pub tables_unresolved: Vec<(String, String)>,
    /// True when every referenced table was resolved. Vacuously true when no
    /// table is referenced — check `tables_referenced` before reading this as
    /// evidence that resolution happened.
    pub complete: bool,
    /// False when no attempt was made (offline extraction).
    pub attempted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputeBudgetObservation {
    pub present: bool,
    pub instruction_indexes: Vec<usize>,
}

/// Counts and identities that describe the transaction's shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionTopology {
    pub num_instructions: usize,
    pub num_static_keys: usize,
    pub num_lookup_tables: usize,
    pub num_alt_loaded_accounts: usize,
    pub account_vector_len: usize,
    pub program_ids: Vec<String>,
    pub program_labels: Vec<String>,
    pub unknown_program_ids: Vec<String>,
    pub instruction_data_lens: Vec<usize>,
}

/// An account index that names a slot outside the loaded account vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutOfRangeIndex {
    pub instruction_index: usize,
    /// `program_id_index` or `account[n]`.
    pub position: String,
    pub index: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountIndexValidity {
    pub account_vector_len: usize,
    pub max_index_referenced: Option<u8>,
    pub all_indexes_in_range: bool,
    pub out_of_range: Vec<OutOfRangeIndex>,
}

/// Signer/payer facts that are mechanically recoverable from an unsigned
/// message. Nothing here says anyone signed: an unsigned transaction carries
/// empty signature slots and a fee payer *position*, not consent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignerObservation {
    pub fee_payer: Option<String>,
    pub signature_slots: usize,
    pub all_signature_slots_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionObservation {
    pub version: TransactionVersion,
    /// Retained verbatim so byte-level checks re-derive payloads from the same
    /// bytes the decode saw, rather than from a re-encode.
    pub transaction_b64: String,
    pub transaction_sha256: String,
    pub decoded: DecodedTransaction,
    pub topology: TransactionTopology,
    pub alt_resolution: AltResolution,
    pub account_index_validity: AccountIndexValidity,
    pub compute_budget: ComputeBudgetObservation,
    pub signers: SignerObservation,
    /// Full loaded-account map, present only when every referenced ALT resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_map: Option<InstructionAccountMap>,
}

impl TransactionObservation {
    /// Raw instruction payloads, in compiled order. Re-derived from the stored
    /// base64 so callers doing byte searches never depend on a re-encode.
    pub fn instruction_payloads(&self) -> Result<Vec<Vec<u8>>> {
        instruction_payloads(&self.transaction_b64)
    }
}

/// Read-only RPC context. Absent means offline extraction.
#[derive(Debug, Clone)]
pub struct RpcContext {
    pub rpc_url: String,
    /// Fetch referenced lookup tables so the full account vector resolves.
    pub resolve_alts: bool,
    /// Fetch owner/executable facts for loaded accounts.
    pub fetch_account_facts: bool,
}

impl RpcContext {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            resolve_alts: true,
            fetch_account_facts: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct SolanaExtractor {
    rpc: Option<RpcContext>,
}

impl SolanaExtractor {
    pub fn offline() -> Self {
        Self { rpc: None }
    }

    pub fn with_rpc(rpc: RpcContext) -> Self {
        Self { rpc: Some(rpc) }
    }

    pub fn rpc(&self) -> Option<&RpcContext> {
        self.rpc.as_ref()
    }

    pub async fn extract_base64(&self, b64: &str) -> Result<TransactionObservation> {
        let b64 = b64.trim();
        let raw = STANDARD.decode(b64).context("invalid base64 transaction")?;
        let decoded = transaction::decode_base64_transaction(b64)?;
        let version = message_version(&raw)?;

        let alt_loaded: usize = decoded
            .address_lookup_table_references
            .iter()
            .map(|a| a.writable_indexes.len() + a.readonly_indexes.len())
            .sum();
        let account_vector_len = decoded.static_account_keys.len() + alt_loaded;

        let mut program_ids: Vec<String> = decoded
            .instructions
            .iter()
            .map(|i| i.program_id.clone())
            .collect();
        program_ids.sort();
        program_ids.dedup();
        let mut program_labels: Vec<String> = decoded
            .instructions
            .iter()
            .map(|i| i.program_label.clone())
            .collect();
        program_labels.sort();
        program_labels.dedup();

        let topology = TransactionTopology {
            num_instructions: decoded.instructions.len(),
            num_static_keys: decoded.static_account_keys.len(),
            num_lookup_tables: decoded.address_lookup_table_references.len(),
            num_alt_loaded_accounts: alt_loaded,
            account_vector_len,
            program_ids,
            program_labels,
            unknown_program_ids: decoded.unknown_program_ids.clone(),
            instruction_data_lens: decoded.instructions.iter().map(|i| i.data_len).collect(),
        };

        let compute_budget = ComputeBudgetObservation {
            present: decoded
                .instructions
                .iter()
                .any(|i| i.program_label == "compute_budget"),
            instruction_indexes: decoded
                .instructions
                .iter()
                .filter(|i| i.program_label == "compute_budget")
                .map(|i| i.index)
                .collect(),
        };

        let account_index_validity = check_account_indexes(&decoded, account_vector_len);

        let signers = SignerObservation {
            fee_payer: decoded.fee_payer.clone(),
            signature_slots: decoded.num_signature_slots,
            all_signature_slots_empty: all_signature_slots_empty(&raw)?,
        };

        let mut alt_resolution = AltResolution {
            tables_referenced: decoded
                .address_lookup_table_references
                .iter()
                .map(|a| a.lookup_table_account.clone())
                .collect(),
            ..Default::default()
        };
        let mut account_map = None;

        if let Some(rpc) = &self.rpc {
            if rpc.resolve_alts && !decoded.address_lookup_table_references.is_empty() {
                alt_resolution.attempted = true;
                let mut tables: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for alt in &decoded.address_lookup_table_references {
                    match lookup_tables::resolve_lookup_table(
                        &rpc.rpc_url,
                        &alt.lookup_table_account,
                    )
                    .await
                    {
                        Ok(addresses) => {
                            alt_resolution
                                .tables_resolved
                                .push(alt.lookup_table_account.clone());
                            tables.insert(alt.lookup_table_account.clone(), addresses);
                        }
                        Err(e) => alt_resolution
                            .tables_unresolved
                            .push((alt.lookup_table_account.clone(), e.to_string())),
                    }
                }
                alt_resolution.complete = alt_resolution.tables_unresolved.is_empty();

                if alt_resolution.complete {
                    let mut map =
                        instruction_map::build_instruction_account_map(&decoded, &tables)?;
                    if rpc.fetch_account_facts {
                        let addresses: Vec<String> = map
                            .loaded_addresses
                            .iter()
                            .map(|a| a.address.clone())
                            .collect();
                        if let Ok(facts) =
                            crate::rpc::fetch_account_facts(&rpc.rpc_url, &addresses).await
                        {
                            instruction_map::annotate_with_account_facts(&mut map, &facts);
                        }
                    }
                    account_map = Some(map);
                }
            } else if decoded.address_lookup_table_references.is_empty() {
                // No tables to resolve: the static keys already are the vector.
                alt_resolution.complete = true;
                let map =
                    instruction_map::build_instruction_account_map(&decoded, &BTreeMap::new())?;
                account_map = Some(map);
            }
        } else if decoded.address_lookup_table_references.is_empty() {
            alt_resolution.complete = true;
            account_map =
                instruction_map::build_instruction_account_map(&decoded, &BTreeMap::new()).ok();
        }

        Ok(TransactionObservation {
            version,
            transaction_b64: b64.to_string(),
            transaction_sha256: crate::artifact::sha256_bytes(&raw),
            decoded,
            topology,
            alt_resolution,
            account_index_validity,
            compute_budget,
            signers,
            account_map,
        })
    }
}

/// Message version, read from the encoded message rather than inferred from
/// whether lookup tables happen to be present.
pub fn message_version(raw: &[u8]) -> Result<TransactionVersion> {
    let vtx: VersionedTransaction =
        bincode::deserialize(raw).context("failed to deserialize as VersionedTransaction")?;
    Ok(match vtx.message {
        VersionedMessage::Legacy(_) => TransactionVersion::Legacy,
        VersionedMessage::V0(_) => TransactionVersion::V0,
    })
}

/// Instruction payloads in compiled order.
pub fn instruction_payloads(b64: &str) -> Result<Vec<Vec<u8>>> {
    let raw = STANDARD
        .decode(b64.trim())
        .context("invalid base64 transaction")?;
    let vtx: VersionedTransaction =
        bincode::deserialize(&raw).context("failed to deserialize as VersionedTransaction")?;
    Ok(vtx
        .message
        .instructions()
        .iter()
        .map(|ix| ix.data.clone())
        .collect())
}

fn all_signature_slots_empty(raw: &[u8]) -> Result<bool> {
    let vtx: VersionedTransaction = bincode::deserialize(raw)?;
    Ok(vtx
        .signatures
        .iter()
        .all(|s| s.as_ref().iter().all(|b| *b == 0)))
}

fn check_account_indexes(
    decoded: &DecodedTransaction,
    account_vector_len: usize,
) -> AccountIndexValidity {
    let mut max_index: Option<u8> = None;
    let mut out_of_range = Vec::new();

    let mut note = |idx: u8, instruction_index: usize, position: String, out: &mut Vec<_>| {
        max_index = Some(max_index.map_or(idx, |m: u8| m.max(idx)));
        if usize::from(idx) >= account_vector_len {
            out.push(OutOfRangeIndex {
                instruction_index,
                position,
                index: idx,
            });
        }
    };

    for ix in &decoded.instructions {
        note(
            ix.program_id_index,
            ix.index,
            "program_id_index".into(),
            &mut out_of_range,
        );
        for (position, idx) in ix.account_indexes.iter().enumerate() {
            note(
                *idx,
                ix.index,
                format!("account[{position}]"),
                &mut out_of_range,
            );
        }
    }

    AccountIndexValidity {
        account_vector_len,
        max_index_referenced: max_index,
        all_indexes_in_range: out_of_range.is_empty(),
        out_of_range,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::{v0, Message, VersionedMessage},
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

    fn legacy_transfer_b64() -> String {
        #[allow(deprecated)]
        use solana_sdk::system_instruction;
        let from = Keypair::new();
        let to = Keypair::new();
        let ix = system_instruction::transfer(&from.pubkey(), &to.pubkey(), 10);
        let message = Message::new(&[ix], Some(&from.pubkey()));
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = Hash::new_unique();
        let vtx = VersionedTransaction::from(tx);
        STANDARD.encode(bincode::serialize(&vtx).unwrap())
    }

    /// A v0 message with one ALT reference. The table is never fetched, so this
    /// exercises the unresolved path.
    fn v0_with_alt_b64() -> (String, String) {
        let payer = Keypair::new();
        let program = Pubkey::new_unique();
        let table = Pubkey::new_unique();
        // The instruction must reference an address held by the table,
        // otherwise the compiler drops the table from the message entirely.
        let from_table = Pubkey::new_unique();
        let ix = Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(from_table, false),
            ],
            data: vec![1, 2, 3, 4],
        };
        let lookup = solana_sdk::message::AddressLookupTableAccount {
            key: table,
            addresses: vec![from_table, Pubkey::new_unique()],
        };
        let msg = v0::Message::try_compile(&payer.pubkey(), &[ix], &[lookup], Hash::new_unique())
            .unwrap();
        let vtx = VersionedTransaction {
            signatures: vec![Default::default()],
            message: VersionedMessage::V0(msg),
        };
        (
            STANDARD.encode(bincode::serialize(&vtx).unwrap()),
            table.to_string(),
        )
    }

    #[tokio::test]
    async fn extracts_legacy_transaction_offline() {
        let tc = SolanaExtractor::offline()
            .extract_base64(&legacy_transfer_b64())
            .await
            .unwrap();

        assert_eq!(tc.version, TransactionVersion::Legacy);
        assert_eq!(tc.topology.num_instructions, 1);
        assert_eq!(tc.topology.num_lookup_tables, 0);
        assert!(tc.account_index_validity.all_indexes_in_range);
        assert!(tc.signers.all_signature_slots_empty);
        assert!(tc.signers.fee_payer.is_some());
        // No tables referenced, so the account vector is complete offline.
        assert!(tc.account_map.is_some());
        assert!(!tc.compute_budget.present);
    }

    #[tokio::test]
    async fn extracts_v0_transaction_and_reports_unresolved_alt() {
        let (b64, table) = v0_with_alt_b64();
        let tc = SolanaExtractor::offline()
            .extract_base64(&b64)
            .await
            .unwrap();

        assert_eq!(tc.version, TransactionVersion::V0);
        assert_eq!(tc.alt_resolution.tables_referenced, vec![table]);
        assert!(!tc.alt_resolution.attempted);
        assert!(!tc.alt_resolution.complete);
        // Without the table contents the account map cannot be built, and the
        // extractor says so instead of guessing addresses.
        assert!(tc.account_map.is_none());
        // Index validity still works: the vector length is known from the
        // message header alone.
        assert!(tc.account_index_validity.account_vector_len >= tc.topology.num_static_keys);
        assert!(tc.account_index_validity.all_indexes_in_range);
    }

    #[tokio::test]
    async fn rejects_malformed_transaction() {
        let e = SolanaExtractor::offline()
            .extract_base64("%%%")
            .await
            .unwrap_err();
        assert!(e.to_string().contains("invalid base64"));

        let junk = STANDARD.encode(b"not-a-transaction");
        assert!(SolanaExtractor::offline()
            .extract_base64(&junk)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn maps_programs_to_labels() {
        let tc = SolanaExtractor::offline()
            .extract_base64(&legacy_transfer_b64())
            .await
            .unwrap();
        assert!(tc
            .topology
            .program_labels
            .contains(&"system_program".to_string()));
        assert!(tc.topology.unknown_program_ids.is_empty());
    }

    #[test]
    fn out_of_range_account_index_is_reported() {
        use crate::transaction::DecodedInstruction;
        let decoded = DecodedTransaction {
            transaction_type: "legacy_or_v0_no_alt".into(),
            num_signature_slots: 1,
            fee_payer: Some("A".into()),
            recent_blockhash: "H".into(),
            static_account_keys: vec!["A".into(), "B".into()],
            address_lookup_table_references: vec![],
            instructions: vec![DecodedInstruction {
                index: 0,
                program_id: "B".into(),
                program_label: "unknown".into(),
                num_accounts: 1,
                data_len: 0,
                discriminator_hex: String::new(),
                data_sha256: String::new(),
                account_indexes: vec![9],
                program_id_index: 1,
            }],
            candidate_jito_tip_transfers: vec![],
            unknown_program_ids: vec!["B".into()],
        };
        let v = check_account_indexes(&decoded, 2);
        assert!(!v.all_indexes_in_range);
        assert_eq!(v.out_of_range.len(), 1);
        assert_eq!(v.out_of_range[0].index, 9);
        assert_eq!(v.max_index_referenced, Some(9));
    }
}
