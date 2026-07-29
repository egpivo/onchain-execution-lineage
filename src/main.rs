use anyhow::Result;
use clap::{Parser, Subcommand};
use dflow_lineage::{capture, lineage, lookup_tables, rpc, transaction};
use std::path::PathBuf;

/// Known mints, spot-checked against this project's existing verified
/// allowlist (Sunday project quote-economics work) -- not re-verified
/// here, reused.
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const JUP_MINT: &str = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";

#[derive(Parser)]
#[command(name = "dflow-lineage")]
#[command(about = "Trace DFlow quote metadata into unsigned Solana transactions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Capture a live quote from DFlow's no-key developer endpoint.
    Quote {
        #[arg(long, default_value = "USDC/SOL")]
        pair: String,
        #[arg(long, default_value_t = 1000.0)]
        amount_usd: f64,
        #[arg(long, default_value_t = 50)]
        slippage_bps: u32,
    },
    /// Decode a base64 transaction (from a file) and print the JSON result.
    /// Also resolves any referenced Address Lookup Tables via read-only RPC
    /// -- an unsigned transaction's ALTs are still real, already-created
    /// on-chain accounts, so this works even though the transaction itself
    /// was never submitted.
    Decode {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetch a real, already-settled transaction by signature via public
    /// RPC (read-only) and decode it -- used to verify the decoder against
    /// real mainnet data, since DFlow's dev-quote-api does not return a
    /// transaction to decode.
    FetchAndDecode {
        #[arg(long)]
        signature: String,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Write the field-lineage CSV for DFlow's no-key developer endpoint.
    Lineage {
        #[arg(
            long,
            default_value = "artifacts/analysis/quote_to_transaction_field_lineage.csv"
        )]
        out: PathBuf,
    },
}

fn resolve_pair(pair: &str) -> Result<(&'static str, &'static str, u8)> {
    match pair {
        "USDC/SOL" => Ok((USDC_MINT, SOL_MINT, 6)),
        "USDC/JUP" => Ok((USDC_MINT, JUP_MINT, 6)),
        _ => anyhow::bail!(
            "unknown pair '{}' -- add it to resolve_pair in main.rs",
            pair
        ),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Quote {
            pair,
            amount_usd,
            slippage_bps,
        } => {
            let (input_mint, output_mint, input_decimals) = resolve_pair(&pair)?;
            let amount_atomic = (amount_usd * 10f64.powi(input_decimals as i32)) as u64;
            let out_dir = PathBuf::from("artifacts/captures");
            let pair_label = pair.replace('/', "_");
            capture::run_capture(
                &pair_label,
                input_mint,
                output_mint,
                amount_atomic,
                amount_usd,
                slippage_bps,
                &out_dir,
            )
            .await?;
        }
        Commands::Decode { file, rpc_url } => {
            let b64 = std::fs::read_to_string(&file)?;
            let decoded = transaction::decode_base64_transaction(&b64)?;

            let mut resolved_alts = Vec::new();
            for alt_ref in &decoded.address_lookup_table_references {
                match lookup_tables::resolve_lookup_table(&rpc_url, &alt_ref.lookup_table_account)
                    .await
                {
                    Ok(addresses) => resolved_alts.push(serde_json::json!({
                        "lookup_table_account": alt_ref.lookup_table_account,
                        "resolved_address_count": addresses.len(),
                        "resolved_addresses": addresses,
                    })),
                    Err(e) => resolved_alts.push(serde_json::json!({
                        "lookup_table_account": alt_ref.lookup_table_account,
                        "resolution_error": e.to_string(),
                    })),
                }
            }

            let mut output = serde_json::to_value(&decoded)?;
            output["resolved_lookup_tables"] = serde_json::json!(resolved_alts);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Commands::FetchAndDecode { signature, rpc_url } => {
            let b64 = rpc::fetch_transaction_base64(&rpc_url, &signature).await?;
            let decoded = transaction::decode_base64_transaction(&b64)?;

            let mut resolved_alts = Vec::new();
            for alt_ref in &decoded.address_lookup_table_references {
                match lookup_tables::resolve_lookup_table(&rpc_url, &alt_ref.lookup_table_account)
                    .await
                {
                    Ok(addresses) => resolved_alts.push(serde_json::json!({
                        "lookup_table_account": alt_ref.lookup_table_account,
                        "resolved_address_count": addresses.len(),
                        "resolved_addresses": addresses,
                    })),
                    Err(e) => resolved_alts.push(serde_json::json!({
                        "lookup_table_account": alt_ref.lookup_table_account,
                        "resolution_error": e.to_string(),
                    })),
                }
            }

            let mut output = serde_json::to_value(&decoded)?;
            output["resolved_lookup_tables"] = serde_json::json!(resolved_alts);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Commands::Lineage { out } => {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            lineage::write_dev_endpoint_lineage(&out)?;
            println!("wrote {}", out.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_pair;

    #[test]
    fn resolve_pair_known_pairs() {
        let (input, output, decimals) = resolve_pair("USDC/SOL").unwrap();
        assert_eq!(input, super::USDC_MINT);
        assert_eq!(output, super::SOL_MINT);
        assert_eq!(decimals, 6);

        let (input, output, decimals) = resolve_pair("USDC/JUP").unwrap();
        assert_eq!(input, super::USDC_MINT);
        assert_eq!(output, super::JUP_MINT);
        assert_eq!(decimals, 6);
    }

    #[test]
    fn resolve_pair_rejects_unknown() {
        assert!(resolve_pair("SOL/USDC").is_err());
    }
}
