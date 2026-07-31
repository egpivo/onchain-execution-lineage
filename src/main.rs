use anyhow::Result;
use clap::{Parser, Subcommand};
use dflow_lineage::{
    artifact, capture, diff, experiment, fingerprint, instruction_map, lineage, lookup_tables,
    pairs, report, rpc, trace, transaction,
};
use std::path::PathBuf;

fn join_indexes(v: &[usize]) -> String {
    v.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Parser)]
#[command(name = "dflow-lineage")]
#[command(
    about = "Read-only Solana execution mechanism and transaction-lineage experiments in Rust"
)]
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
    Decode {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Fetch a settled transaction by signature via public RPC and decode it.
    FetchAndDecode {
        #[arg(long)]
        signature: String,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
    },
    /// Decode a base64 transaction, resolve ALTs, map instructions → CSV + Markdown.
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
    /// Deprecated: writes the static DFlow-dev field-lineage CSV.
    /// Prefer `trace` for provider-normalized LineageBundle output.
    #[command(hide = false)]
    Lineage {
        #[arg(
            long,
            default_value = "artifacts/analysis/quote_to_transaction_field_lineage.csv"
        )]
        out: PathBuf,
    },
    /// Build a LineageBundle from manifest / provider JSON / tx / signature.
    Trace {
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        provider_json: Option<PathBuf>,
        #[arg(long)]
        transaction: Option<PathBuf>,
        #[arg(long)]
        signature: Option<String>,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        #[arg(long, default_value_t = false)]
        resolve_alts: bool,
        #[arg(long, default_value_t = false)]
        enrich_settlement: bool,
        #[arg(long, default_value = "artifacts/analysis/lineage_bundle.json")]
        out_json: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/lineage_report.md")]
        out_md: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/lineage_evidence.csv")]
        out_csv: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/lineage.dot")]
        out_dot: PathBuf,
    },
    /// Diff two LineageBundle JSON files.
    Diff {
        #[arg(long)]
        left: PathBuf,
        #[arg(long)]
        right: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/lineage_diff.json")]
        out_json: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/lineage_diff.md")]
        out_md: PathBuf,
    },
    /// Fingerprint a corpus group (refuses n=1 promotion).
    Fingerprint {
        #[arg(long, default_value = ".local/corpus/corpus_manifest.json")]
        corpus: PathBuf,
        #[arg(long)]
        group: String,
        #[arg(long, default_value = ".")]
        base_dir: PathBuf,
        #[arg(long, default_value = "artifacts/analysis/fingerprint_report.json")]
        out: PathBuf,
    },
    /// Run a bounded read-only provider experiment from a manifest.
    Experiment {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = ".")]
        base_dir: PathBuf,
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
        }
        Commands::Lineage { out } => {
            eprintln!(
                "warning: `lineage` is deprecated; use `trace` for LineageBundle JSON/Markdown/CSV/DOT. \
                 This command still writes the static DFlow-dev field CSV for compatibility."
            );
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            lineage::write_dev_endpoint_lineage(&out)?;
            println!("wrote {}", out.display());
        }
        Commands::Trace {
            manifest,
            provider_json,
            transaction,
            signature,
            rpc_url,
            resolve_alts,
            enrich_settlement,
            out_json,
            out_md,
            out_csv,
            out_dot,
        } => {
            let loaded_manifest = match &manifest {
                Some(p) => Some(artifact::ArtifactManifest::load_path(p)?),
                None => None,
            };
            let bundle = trace::build_trace(trace::TraceInputs {
                manifest: loaded_manifest.as_ref(),
                provider_json_path: provider_json.as_deref(),
                transaction_b64_path: transaction.as_deref(),
                signature: signature.as_deref(),
                rpc_url: &rpc_url,
                resolve_alts,
                enrich_settlement,
            })
            .await?;

            for path in [&out_json, &out_md, &out_csv, &out_dot] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out_json, bundle.to_canonical_json()?)?;
            report::write_markdown_report(&bundle, &out_md)?;
            report::write_evidence_csv(&bundle, &out_csv)?;
            report::write_dot(&bundle, &out_dot)?;
            println!("wrote {}", out_json.display());
            println!("wrote {}", out_md.display());
            println!("wrote {}", out_csv.display());
            println!("wrote {}", out_dot.display());
        }
        Commands::Diff {
            left,
            right,
            out_json,
            out_md,
        } => {
            let left_b: dflow_lineage::lineage_model::LineageBundle =
                serde_json::from_str(&std::fs::read_to_string(&left)?)?;
            let right_b: dflow_lineage::lineage_model::LineageBundle =
                serde_json::from_str(&std::fs::read_to_string(&right)?)?;
            let d = diff::diff_bundles(&left_b, &right_b);
            for path in [&out_json, &out_md] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out_json, serde_json::to_string_pretty(&d)?)?;
            report::write_diff_markdown(&d, &out_md)?;
            println!("wrote {}", out_json.display());
            println!("wrote {}", out_md.display());
        }
        Commands::Fingerprint {
            corpus,
            group,
            base_dir,
            out,
        } => {
            let c = fingerprint::load_corpus(&corpus)?;
            let report = fingerprint::fingerprint_group(&c, &base_dir, &group)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
            println!("wrote {}", out.display());
            if report.insufficient_sample {
                println!("insufficient_sample=true (n=1 promotion refused)");
            }
        }
        Commands::Experiment { manifest, base_dir } => {
            let report = experiment::run_experiment(&manifest, &base_dir).await?;
            println!(
                "experiment_id={} treatments={} baseline={}",
                report.experiment_id,
                report.runs.len(),
                report.baseline_value
            );
            for run in &report.runs {
                println!(
                    "  value={} baseline={} raw={}",
                    run.treatment_value, run.is_baseline, run.raw_path
                );
            }
            println!(
                "wrote experiment_report.json / experiment_report.md under the manifest output_path"
            );
        }
    }

    Ok(())
}
