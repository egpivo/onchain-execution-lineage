//! Resolves address-lookup-table (ALT) references into their actual
//! on-chain addresses, via a read-only public RPC call. Fully public
//! on-chain data -- no wallet, no signing, no credentials involved.

use anyhow::{Context, Result};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Fetches one lookup table account and returns its full resolved address
/// list, in on-chain storage order (index 0, 1, 2, ... as referenced by
/// writable_indexes / readonly_indexes in the compiled message).
pub async fn resolve_lookup_table(rpc_url: &str, lookup_table_account: &str) -> Result<Vec<String>> {
    let client = RpcClient::new(rpc_url.to_string());
    let pubkey = Pubkey::from_str(lookup_table_account).context("invalid lookup table pubkey")?;

    let account = client
        .get_account(&pubkey)
        .await
        .context("failed to fetch lookup table account -- it may not exist or RPC may be rate-limiting")?;

    let table = AddressLookupTable::deserialize(&account.data)
        .context("failed to deserialize account data as an AddressLookupTable")?;

    Ok(table.addresses.iter().map(|p| p.to_string()).collect())
}
