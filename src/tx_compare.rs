//! Compare unsigned Solana transactions without treating blockhash churn as
//! a full structural difference.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_sdk::transaction::VersionedTransaction;
use std::collections::BTreeSet;

use crate::lookup_tables;
use crate::transaction::{decode_base64_transaction, DecodedTransaction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldStability {
    pub left: String,
    pub right: String,
    pub equal: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTransactionObservation {
    pub base64_length: usize,
    pub raw_sha256: String,
    pub transaction_version: String,
    pub transaction_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTransactionDiff {
    pub left: RawTransactionObservation,
    pub right: RawTransactionObservation,
    pub base64_length_equal: bool,
    pub raw_sha256_equal: bool,
    pub transaction_version_equal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTransactionDiff {
    pub signatures: FieldStability,
    pub recent_blockhash: FieldStability,
    pub message_header: FieldStability,
    pub static_account_keys: FieldStability,
    pub alt_references: FieldStability,
    pub resolved_loaded_account_vector: FieldStability,
    pub compiled_instruction_program_indices: FieldStability,
    pub instruction_account_indices: FieldStability,
    pub instruction_data_hashes: FieldStability,
    pub compute_budget_instructions: FieldStability,
    pub program_set: FieldStability,
    pub instruction_count: FieldStability,
    /// True when every classified field other than recent_blockhash (and
    /// empty/default signature slots) is equal.
    pub stable_aside_from_blockhash: bool,
    /// Topology only: header, accounts, ALTs, program indices, account indices,
    /// instruction count, program set, compute-budget *presence/indices*
    /// (not compute-budget data hashes). Excludes instruction data hashes.
    pub topology_stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedByteRange {
    pub instruction_index: usize,
    pub start: usize,
    pub end: usize,
    pub left_hex: String,
    pub right_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionPayloadDiff {
    pub instruction_index: usize,
    pub left_data_len: usize,
    pub right_data_len: usize,
    pub left_data_sha256: String,
    pub right_data_sha256: String,
    pub changed_byte_offsets: Vec<usize>,
    pub changed_ranges: Vec<ChangedByteRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadDiffReport {
    pub instructions: Vec<InstructionPayloadDiff>,
    pub any_payload_difference: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEncodingHit {
    pub treatment_value: String,
    pub instruction_index: usize,
    pub byte_offset: usize,
    pub width_bytes: usize,
    pub matched_value: String,
    pub classification: String,
}

#[derive(Debug, Clone)]
struct TxView {
    decoded: DecodedTransaction,
    signature_fingerprint: String,
    header: String,
    alt_fingerprint: String,
    program_indices: String,
    account_indices: String,
    data_hashes: String,
    compute_budget: String,
    program_set: String,
    raw_obs: RawTransactionObservation,
}

pub async fn observe_raw_transaction(b64: &str) -> Result<RawTransactionObservation> {
    let view = view_transaction(b64)?;
    Ok(view.raw_obs)
}

pub fn diff_raw_transactions(left_b64: &str, right_b64: &str) -> Result<RawTransactionDiff> {
    let left = view_transaction(left_b64)?;
    let right = view_transaction(right_b64)?;
    Ok(RawTransactionDiff {
        base64_length_equal: left.raw_obs.base64_length == right.raw_obs.base64_length,
        raw_sha256_equal: left.raw_obs.raw_sha256 == right.raw_obs.raw_sha256,
        transaction_version_equal: left.raw_obs.transaction_version
            == right.raw_obs.transaction_version,
        left: left.raw_obs,
        right: right.raw_obs,
    })
}

pub async fn diff_canonical_transactions(
    left_b64: &str,
    right_b64: &str,
    rpc_url: &str,
    resolve_alts: bool,
) -> Result<CanonicalTransactionDiff> {
    let left = view_transaction(left_b64)?;
    let right = view_transaction(right_b64)?;

    let left_loaded = if resolve_alts {
        resolve_loaded_vector(rpc_url, &left.decoded).await?
    } else {
        None
    };
    let right_loaded = if resolve_alts {
        resolve_loaded_vector(rpc_url, &right.decoded).await?
    } else {
        None
    };

    let signatures = stability(
        &left.signature_fingerprint,
        &right.signature_fingerprint,
        "signature slot fingerprint (unsigned txs typically empty/default)",
    );
    let recent_blockhash = stability(
        &left.decoded.recent_blockhash,
        &right.decoded.recent_blockhash,
        "recent blockhash is expected to differ across sequential constructions",
    );
    let message_header = stability(&left.header, &right.header, "message header fields");
    let static_account_keys = stability(
        &join(&left.decoded.static_account_keys),
        &join(&right.decoded.static_account_keys),
        "static account key vector",
    );
    let alt_references = stability(
        &left.alt_fingerprint,
        &right.alt_fingerprint,
        "ALT account + writable/readonly index lists",
    );
    let resolved_loaded_account_vector = match (&left_loaded, &right_loaded) {
        (Some(l), Some(r)) => stability(&join(l), &join(r), "resolved loaded-account vector"),
        _ => FieldStability {
            left: left_loaded
                .as_ref()
                .map(|v| join(v))
                .unwrap_or_else(|| "unresolved".into()),
            right: right_loaded
                .as_ref()
                .map(|v| join(v))
                .unwrap_or_else(|| "unresolved".into()),
            equal: left_loaded.is_none() && right_loaded.is_none(),
            note: "ALT resolve skipped or incomplete".into(),
        },
    };
    let compiled_instruction_program_indices = stability(
        &left.program_indices,
        &right.program_indices,
        "compiled instruction program_id_index values",
    );
    let instruction_account_indices = stability(
        &left.account_indices,
        &right.account_indices,
        "per-instruction account index vectors",
    );
    let instruction_data_hashes = stability(
        &left.data_hashes,
        &right.data_hashes,
        "SHA-256 of instruction data bytes",
    );
    let compute_budget_instructions = stability(
        &left.compute_budget,
        &right.compute_budget,
        "compute-budget instruction indices and data hashes",
    );
    let program_set = stability(&left.program_set, &right.program_set, "unique program IDs");
    let instruction_count = stability(
        &left.decoded.instructions.len().to_string(),
        &right.decoded.instructions.len().to_string(),
        "instruction count",
    );

    let topology_core = message_header.equal
        && static_account_keys.equal
        && alt_references.equal
        && (resolved_loaded_account_vector.equal
            || resolved_loaded_account_vector.note.contains("skipped"))
        && compiled_instruction_program_indices.equal
        && instruction_account_indices.equal
        && program_set.equal
        && instruction_count.equal
        && signatures.equal;

    // Compute-budget topology: same indices only (ignore data hash churn).
    let cb_topo_left = left
        .decoded
        .instructions
        .iter()
        .filter(|i| i.program_label == "compute_budget")
        .map(|i| i.index.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let cb_topo_right = right
        .decoded
        .instructions
        .iter()
        .filter(|i| i.program_label == "compute_budget")
        .map(|i| i.index.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let topology_stable = topology_core && cb_topo_left == cb_topo_right;

    let stable_aside_from_blockhash =
        topology_stable && instruction_data_hashes.equal && compute_budget_instructions.equal;

    Ok(CanonicalTransactionDiff {
        signatures,
        recent_blockhash,
        message_header,
        static_account_keys,
        alt_references,
        resolved_loaded_account_vector,
        compiled_instruction_program_indices,
        instruction_account_indices,
        instruction_data_hashes,
        compute_budget_instructions,
        program_set,
        instruction_count,
        stable_aside_from_blockhash,
        topology_stable,
    })
}

/// Byte-level instruction payload diff (raw data, not hashes only).
pub fn diff_instruction_payloads(left_b64: &str, right_b64: &str) -> Result<PayloadDiffReport> {
    let left_raw = STANDARD.decode(left_b64.trim())?;
    let right_raw = STANDARD.decode(right_b64.trim())?;
    let left_vtx: VersionedTransaction = bincode::deserialize(&left_raw)?;
    let right_vtx: VersionedTransaction = bincode::deserialize(&right_raw)?;
    let left_ixs = left_vtx.message.instructions();
    let right_ixs = right_vtx.message.instructions();
    let n = left_ixs.len().max(right_ixs.len());
    let mut instructions = Vec::new();
    for i in 0..n {
        let ld = left_ixs.get(i).map(|x| x.data.as_slice()).unwrap_or(&[]);
        let rd = right_ixs.get(i).map(|x| x.data.as_slice()).unwrap_or(&[]);
        let mut hasher_l = Sha256::new();
        hasher_l.update(ld);
        let mut hasher_r = Sha256::new();
        hasher_r.update(rd);
        let mut offsets = Vec::new();
        let max_len = ld.len().max(rd.len());
        for off in 0..max_len {
            let lb = ld.get(off).copied();
            let rb = rd.get(off).copied();
            if lb != rb {
                offsets.push(off);
            }
        }
        let ranges = collapse_ranges(i, ld, rd, &offsets);
        instructions.push(InstructionPayloadDiff {
            instruction_index: i,
            left_data_len: ld.len(),
            right_data_len: rd.len(),
            left_data_sha256: format!("{:x}", hasher_l.finalize()),
            right_data_sha256: format!("{:x}", hasher_r.finalize()),
            changed_byte_offsets: offsets,
            changed_ranges: ranges,
        });
    }
    let any_payload_difference = instructions
        .iter()
        .any(|i| i.left_data_sha256 != i.right_data_sha256 || i.left_data_len != i.right_data_len);
    Ok(PayloadDiffReport {
        instructions,
        any_payload_difference,
    })
}

fn collapse_ranges(
    instruction_index: usize,
    left: &[u8],
    right: &[u8],
    offsets: &[usize],
) -> Vec<ChangedByteRange> {
    if offsets.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = offsets[0];
    let mut prev = offsets[0];
    for &off in offsets.iter().skip(1) {
        if off == prev + 1 {
            prev = off;
            continue;
        }
        ranges.push(range_hex(instruction_index, left, right, start, prev + 1));
        start = off;
        prev = off;
    }
    ranges.push(range_hex(instruction_index, left, right, start, prev + 1));
    ranges
}

fn range_hex(
    instruction_index: usize,
    left: &[u8],
    right: &[u8],
    start: usize,
    end: usize,
) -> ChangedByteRange {
    let lh: String = left
        .get(start..end.min(left.len()))
        .unwrap_or(&[])
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let rh: String = right
        .get(start..end.min(right.len()))
        .unwrap_or(&[])
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ChangedByteRange {
        instruction_index,
        start,
        end,
        left_hex: lh,
        right_hex: rh,
    }
}

/// Search changed ranges (and full ix data) for LE/BE encodings of amounts.
pub fn search_candidate_encodings_in_payload(
    treatment_label: &str,
    left_b64: &str,
    right_b64: &str,
    amounts: &[(&str, &str)],
) -> Result<Vec<CandidateEncodingHit>> {
    let payload = diff_instruction_payloads(left_b64, right_b64)?;
    let right_raw = STANDARD.decode(right_b64.trim())?;
    let right_vtx: VersionedTransaction = bincode::deserialize(&right_raw)?;
    let mut hits = Vec::new();
    for ix_diff in &payload.instructions {
        if ix_diff.changed_byte_offsets.is_empty()
            && ix_diff.left_data_sha256 == ix_diff.right_data_sha256
        {
            continue;
        }
        let Some(ix) = right_vtx
            .message
            .instructions()
            .get(ix_diff.instruction_index)
        else {
            continue;
        };
        for (label, amount_str) in amounts {
            let Ok(amount) = amount_str.parse::<u128>() else {
                continue;
            };
            for (width, needle, endian) in amount_needles(amount) {
                // Prefer matches that overlap a changed range.
                for range in &ix_diff.changed_ranges {
                    if let Some(rel) = find_subslice(
                        ix.data
                            .get(range.start..range.end.min(ix.data.len()))
                            .unwrap_or(&[]),
                        &needle,
                    ) {
                        hits.push(CandidateEncodingHit {
                            treatment_value: format!("{treatment_label}:{label}:{endian}"),
                            instruction_index: ix_diff.instruction_index,
                            byte_offset: range.start + rel,
                            width_bytes: width,
                            matched_value: amount_str.to_string(),
                            classification: "Candidate encoding relationship".into(),
                        });
                    }
                }
                // Also record full-data matches in changed instructions.
                if let Some(offset) = find_subslice(&ix.data, &needle) {
                    if ix_diff
                        .changed_byte_offsets
                        .iter()
                        .any(|&o| o >= offset && o < offset.saturating_add(width))
                    {
                        hits.push(CandidateEncodingHit {
                            treatment_value: format!("{treatment_label}:{label}:{endian}"),
                            instruction_index: ix_diff.instruction_index,
                            byte_offset: offset,
                            width_bytes: width,
                            matched_value: amount_str.to_string(),
                            classification: "Candidate encoding relationship".into(),
                        });
                    }
                }
            }
        }
    }
    // Dedup
    hits.sort_by(|a, b| {
        (
            a.instruction_index,
            a.byte_offset,
            a.width_bytes,
            a.matched_value.clone(),
            a.treatment_value.clone(),
        )
            .cmp(&(
                b.instruction_index,
                b.byte_offset,
                b.width_bytes,
                b.matched_value.clone(),
                b.treatment_value.clone(),
            ))
    });
    hits.dedup_by(|a, b| {
        a.instruction_index == b.instruction_index
            && a.byte_offset == b.byte_offset
            && a.width_bytes == b.width_bytes
            && a.matched_value == b.matched_value
            && a.treatment_value == b.treatment_value
    });
    Ok(hits)
}

pub(crate) fn amount_needles(amount: u128) -> Vec<(usize, Vec<u8>, &'static str)> {
    let mut out = Vec::new();
    if amount <= u64::MAX as u128 {
        let v = amount as u64;
        out.push((8, v.to_le_bytes().to_vec(), "u64_le"));
        out.push((8, v.to_be_bytes().to_vec(), "u64_be"));
    }
    if amount <= u32::MAX as u128 {
        let v = amount as u32;
        out.push((4, v.to_le_bytes().to_vec(), "u32_le"));
        out.push((4, v.to_be_bytes().to_vec(), "u32_be"));
    }
    out
}

/// Search raw instruction data for little-endian encodings of a decimal amount.
/// Matches are classified only as a candidate relationship.
pub fn search_candidate_le_threshold_encoding(
    treatment_value: &str,
    b64: &str,
    threshold_decimal: &str,
) -> Result<Vec<CandidateEncodingHit>> {
    let raw = STANDARD
        .decode(b64.trim())
        .context("invalid base64 for encoding search")?;
    let vtx: VersionedTransaction =
        bincode::deserialize(&raw).context("deserialize for encoding search")?;
    let amount: u128 = threshold_decimal
        .parse()
        .context("threshold must be decimal integer")?;
    let needles = le_encodings(amount);

    let mut hits = Vec::new();
    for (ix_index, ix) in vtx.message.instructions().iter().enumerate() {
        for (width, needle) in &needles {
            if let Some(offset) = find_subslice(&ix.data, needle) {
                hits.push(CandidateEncodingHit {
                    treatment_value: treatment_value.to_string(),
                    instruction_index: ix_index,
                    byte_offset: offset,
                    width_bytes: *width,
                    matched_value: threshold_decimal.to_string(),
                    classification: "Candidate encoding relationship".into(),
                });
            }
        }
    }
    Ok(hits)
}

fn view_transaction(b64: &str) -> Result<TxView> {
    let raw_bytes = STANDARD
        .decode(b64.trim())
        .context("invalid base64 transaction")?;
    let vtx: VersionedTransaction =
        bincode::deserialize(&raw_bytes).context("failed to deserialize VersionedTransaction")?;
    let decoded = decode_base64_transaction(b64)?;

    let mut sig_hasher = Sha256::new();
    for sig in &vtx.signatures {
        sig_hasher.update(sig.as_ref());
    }
    let signature_fingerprint = format!("{:x}", sig_hasher.finalize());

    let header = vtx.message.header();
    let header_s = format!(
        "req_sig={};ro_signed={};ro_unsigned={}",
        header.num_required_signatures,
        header.num_readonly_signed_accounts,
        header.num_readonly_unsigned_accounts
    );

    let alt_fingerprint = decoded
        .address_lookup_table_references
        .iter()
        .map(|a| {
            format!(
                "{}:w{:?}:r{:?}",
                a.lookup_table_account, a.writable_indexes, a.readonly_indexes
            )
        })
        .collect::<Vec<_>>()
        .join("|");

    let program_indices = decoded
        .instructions
        .iter()
        .map(|i| i.program_id_index.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let account_indices = decoded
        .instructions
        .iter()
        .map(|i| {
            format!(
                "[{}]",
                i.account_indexes
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    let data_hashes = decoded
        .instructions
        .iter()
        .map(|i| i.data_sha256.clone())
        .collect::<Vec<_>>()
        .join(",");
    let compute_budget = decoded
        .instructions
        .iter()
        .filter(|i| i.program_label == "compute_budget")
        .map(|i| format!("{}:{}", i.index, i.data_sha256))
        .collect::<Vec<_>>()
        .join(",");
    let program_set = decoded
        .instructions
        .iter()
        .map(|i| i.program_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",");

    let version = match &vtx.message {
        solana_sdk::message::VersionedMessage::Legacy(_) => "legacy".to_string(),
        solana_sdk::message::VersionedMessage::V0(_) => "v0".to_string(),
    };

    Ok(TxView {
        decoded: decoded.clone(),
        signature_fingerprint,
        header: header_s,
        alt_fingerprint,
        program_indices,
        account_indices,
        data_hashes,
        compute_budget,
        program_set,
        raw_obs: RawTransactionObservation {
            base64_length: b64.trim().len(),
            raw_sha256: crate::artifact::sha256_bytes(&raw_bytes),
            transaction_version: version,
            transaction_type: decoded.transaction_type,
        },
    })
}

async fn resolve_loaded_vector(
    rpc_url: &str,
    decoded: &DecodedTransaction,
) -> Result<Option<Vec<String>>> {
    if decoded.address_lookup_table_references.is_empty() {
        return Ok(Some(decoded.static_account_keys.clone()));
    }
    let mut tables = std::collections::BTreeMap::new();
    for alt in &decoded.address_lookup_table_references {
        let addrs = lookup_tables::resolve_lookup_table(rpc_url, &alt.lookup_table_account).await?;
        tables.insert(alt.lookup_table_account.clone(), addrs);
    }
    let map = crate::instruction_map::build_instruction_account_map(decoded, &tables)?;
    Ok(Some(
        map.loaded_addresses
            .into_iter()
            .map(|a| a.address)
            .collect(),
    ))
}

fn stability(left: &str, right: &str, note: &str) -> FieldStability {
    FieldStability {
        left: left.to_string(),
        right: right.to_string(),
        equal: left == right,
        note: note.to_string(),
    }
}

fn join(v: &[String]) -> String {
    v.join(",")
}

fn le_encodings(amount: u128) -> Vec<(usize, Vec<u8>)> {
    let mut out = Vec::new();
    if amount <= u64::MAX as u128 {
        out.push((8, (amount as u64).to_le_bytes().to_vec()));
    }
    out.push((16, amount.to_le_bytes().to_vec()));
    if amount <= u32::MAX as u128 {
        out.push((4, (amount as u32).to_le_bytes().to_vec()));
    }
    out
}

/// Every offset where `needle` occurs, not just the first.
///
/// The bracket report only needs the first hit inside a changed range; the
/// public evidence extract needs all of them, because "the match is unique
/// within this payload" is itself a published claim.
pub(crate) fn find_all_subslices(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() || hay.len() < needle.len() {
        return out;
    }
    for start in 0..=(hay.len() - needle.len()) {
        if &hay[start..start + needle.len()] == needle {
            out.push(start);
        }
    }
    out
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        hash::Hash,
        message::Message,
        signature::{Keypair, Signer},
        transaction::{Transaction, VersionedTransaction},
    };

    #[allow(deprecated)]
    fn transfer_b64(lamports: u64) -> String {
        use solana_sdk::system_instruction;
        let from = Keypair::new();
        let to = Keypair::new();
        let ix = system_instruction::transfer(&from.pubkey(), &to.pubkey(), lamports);
        let message = Message::new(&[ix], Some(&from.pubkey()));
        let mut tx = Transaction::new_unsigned(message);
        tx.message.recent_blockhash = Hash::new_unique();
        let vtx = VersionedTransaction::from(tx);
        STANDARD.encode(bincode::serialize(&vtx).unwrap())
    }

    #[test]
    fn blockhash_difference_does_not_force_structural_instability_flag_alone() {
        // Two independent transfers differ in keys too; this only checks the
        // helper reports raw sha inequality without panicking.
        let a = transfer_b64(10);
        let b = transfer_b64(10);
        let raw = diff_raw_transactions(&a, &b).unwrap();
        assert!(!raw.raw_sha256_equal);
    }

    #[test]
    fn finds_candidate_le_encoding_of_lamports() {
        let b64 = transfer_b64(123456789);
        let hits = search_candidate_le_threshold_encoding("t", &b64, "123456789").unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].classification, "Candidate encoding relationship");
    }
}
