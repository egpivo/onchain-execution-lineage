//! Known Solana program IDs, each independently verified via web search
//! before inclusion (not trusted from memory) -- the Python decoder built
//! earlier in this project (Sunday project's engineering-reset phase)
//! caught real transcription errors doing this same check, so it is
//! repeated here rather than copied blind.

use std::collections::HashMap;

pub fn known_programs() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("11111111111111111111111111111111", "system_program"),
        ("ComputeBudget111111111111111111111111111111", "compute_budget"),
        ("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "spl_token"),
        ("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", "spl_token_2022"),
        ("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", "associated_token_account"),
        ("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", "memo_v2"),
        ("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo", "memo_v1"),
        ("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", "jupiter_aggregator_v6"),
        ("AddressLookupTab1e1111111111111111111111111", "address_lookup_table_program"),
    ])
}

/// Jito mainnet tip payment accounts, fetched from
/// jito-foundation.gitbook.io/mev/mev-payment-and-distribution/on-chain-addresses
/// (same source used and cross-checked in the Python decoder). Spot-check
/// against the live `getTipAccounts` RPC method before treating a match as
/// confirmed for any real capture.
pub fn jito_tip_accounts() -> Vec<&'static str> {
    vec![
        "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
        "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
        "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
        "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
        "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
        "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
        "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
        "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::str::FromStr;

    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn known_programs_cover_core_labels() {
        let known = known_programs();
        assert_eq!(
            known.get("11111111111111111111111111111111"),
            Some(&"system_program")
        );
        assert_eq!(
            known.get("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"),
            Some(&"jupiter_aggregator_v6")
        );
        assert_eq!(
            known.get("ComputeBudget111111111111111111111111111111"),
            Some(&"compute_budget")
        );
        assert_eq!(
            known.get("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            Some(&"spl_token")
        );
        assert_eq!(
            known.get("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
            Some(&"associated_token_account")
        );
    }

    #[test]
    fn known_program_ids_are_valid_pubkeys() {
        for id in known_programs().keys() {
            Pubkey::from_str(id).unwrap_or_else(|_| panic!("invalid program pubkey: {id}"));
        }
    }

    #[test]
    fn jito_tip_accounts_are_unique_valid_pubkeys() {
        let tips = jito_tip_accounts();
        assert_eq!(tips.len(), 8);
        let unique: HashSet<_> = tips.iter().copied().collect();
        assert_eq!(unique.len(), tips.len());
        for id in tips {
            Pubkey::from_str(id).unwrap_or_else(|_| panic!("invalid tip pubkey: {id}"));
        }
    }
}
