use anyhow::Result;
use clap::{Parser, Subcommand};
use dflow_lineage::{capture, instruction_map, lineage, lookup_tables, pairs, rpc, transaction};
use std::path::PathBuf;

fn join_indexes(v: &[usize]) -> String {
    v.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

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
    /// Decode a base64 transaction, resolve its lookup tables, and map every
    /// compiled instruction onto the addresses it actually loads. Writes a
    /// loaded-address CSV and an instruction/account markdown map.
    Map {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        #[arg(long, default_value = "artifacts/analysis/loaded_address_map.csv")]
        out_csv: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/instruction_account_map.md")]
        out_md: PathBuf,
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
            let (input_mint, output_mint, input_decimals) = pairs::resolve_pair(&pair)?;
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
        Commands::Map {
            file,
            rpc_url,
            out_csv,
            out_md,
        } => {
            let b64 = std::fs::read_to_string(&file)?;
            let decoded = transaction::decode_base64_transaction(&b64)?;

            let mut tables = std::collections::BTreeMap::new();
            for alt in &decoded.address_lookup_table_references {
                let addresses =
                    lookup_tables::resolve_lookup_table(&rpc_url, &alt.lookup_table_account)
                        .await?;
                tables.insert(alt.lookup_table_account.clone(), addresses);
            }

            let mut map = instruction_map::build_instruction_account_map(&decoded, &tables)?;

            let addresses: Vec<String> = map
                .loaded_addresses
                .iter()
                .map(|a| a.address.clone())
                .collect();
            let facts = rpc::fetch_account_facts(&rpc_url, &addresses).await?;
            instruction_map::annotate_with_account_facts(&mut map, &facts);

            for path in [&out_csv, &out_md] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            let mut wtr = csv::Writer::from_path(&out_csv)?;
            wtr.write_record([
                "account_vector_index",
                "address",
                "source",
                "lookup_table_account",
                "lookup_table_index",
                "writable",
                "label",
                "referenced_by_instructions",
                "is_program_for_instructions",
                "transaction_relevant",
                "exists_on_chain",
                "executable",
                "owner_program",
                "owner_label",
            ])?;
            for a in &map.loaded_addresses {
                wtr.write_record([
                    a.account_vector_index.to_string(),
                    a.address.clone(),
                    a.source.clone(),
                    a.lookup_table_account.clone().unwrap_or_default(),
                    a.lookup_table_index
                        .map(|i| i.to_string())
                        .unwrap_or_default(),
                    a.writable.to_string(),
                    a.label.clone(),
                    join_indexes(&a.referenced_by_instructions),
                    join_indexes(&a.is_program_for_instructions),
                    instruction_map::is_transaction_relevant(a).to_string(),
                    a.exists_on_chain.map(|v| v.to_string()).unwrap_or_default(),
                    a.executable.map(|v| v.to_string()).unwrap_or_default(),
                    a.owner_program.clone().unwrap_or_default(),
                    a.owner_label.clone().unwrap_or_default(),
                ])?;
            }
            wtr.flush()?;

            let mut md = String::new();
            md.push_str("# Instruction / account map\n\n");
            md.push_str(&format!(
                "Static keys: {}. Loaded from lookup tables: {}. Full account vector: {}.\n\n\
                 Addresses held by the referenced lookup tables in total: {} -- \
                 of which this transaction loads {}. Table membership is not \
                 evidence of transaction relevance.\n\n",
                map.total_static_keys,
                map.total_loaded_from_alts,
                map.total_account_vector_len,
                map.total_addresses_in_referenced_tables,
                map.total_loaded_from_alts,
            ));
            for ix in &map.instructions {
                md.push_str(&format!(
                    "## Instruction {} -- {} (`{}`)\n\n\
                     Data: {} bytes, discriminator `{}`. Accounts: {}.\n\n\
                     | # | idx | address | source | writable | label |\n\
                     |---|---|---|---|---|---|\n",
                    ix.instruction_index,
                    ix.program_label,
                    ix.program_id,
                    ix.data_len,
                    ix.discriminator_hex,
                    ix.accounts.len(),
                ));
                for a in &ix.accounts {
                    md.push_str(&format!(
                        "| {} | {} | `{}` | {} | {} | {} |\n",
                        a.position_in_instruction,
                        a.account_vector_index,
                        a.address,
                        a.source,
                        a.writable,
                        a.label,
                    ));
                }
                md.push('\n');
            }
            std::fs::write(&out_md, md)?;

            println!("wrote {}", out_csv.display());
            println!("wrote {}", out_md.display());

            let dflow_ix = map
                .instructions
                .iter()
                .filter(|i| i.program_label == "dflow_aggregator_v4")
                .count();
            // An integrator program can own an account in the vector without
            // appearing in it, so check both the address and its owner.
            let direct_markers = map
                .loaded_addresses
                .iter()
                .filter(|a| a.label.starts_with("candidate_integrator_program"))
                .count();
            let owner_markers =
                instruction_map::owner_derived_markers(&map, "candidate_integrator_program");
            let candidate_markers = direct_markers + owner_markers.len();
            let venue_candidates = map
                .loaded_addresses
                .iter()
                .filter(|a| a.label == "candidate_downstream_venue_program")
                .count();

            println!("---");
            println!("transaction_type:           {}", decoded.transaction_type);
            println!(
                "lookup_tables:              {}",
                decoded.address_lookup_table_references.len()
            );
            println!("compiled_instructions:      {}", map.instructions.len());
            println!("dflow_program_instructions: {dflow_ix}");
            println!(
                "addresses_in_tables:        {}",
                map.total_addresses_in_referenced_tables
            );
            println!("addresses_actually_loaded:  {}", map.total_loaded_from_alts);
            println!("candidate_integrator_marks: {candidate_markers}");
            println!("candidate_venue_programs:   {venue_candidates}");
            println!(
                "jito_tip_matches:           {}",
                decoded.candidate_jito_tip_transfers.len()
            );
            println!("settlement:                 not submitted");
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
