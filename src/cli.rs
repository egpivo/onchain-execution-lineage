//! CLI surface, shared by the `onchain-execution-lineage` binary and the
//! deprecated `dflow-lineage` compatibility alias.
//!
//! Command roles map one-to-one onto the canonical pipeline:
//! `extract` constructs lineage, `trace` explains it, `verify` checks it.

use crate::adapters::ProviderId;
use crate::execution_context::ExecutionContext;
use crate::lineage_model::LineageBundle;
use crate::solana::RpcContext;
use crate::{
    artifact, capture, checks, diff, evidence_extract, experiment, extract, fingerprint,
    instruction_map, lineage, lookup_tables, pairs, reference_case, report, route_bracket, rpc,
    trace, transaction,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

fn join_indexes(v: &[usize]) -> String {
    v.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Print the public claim table. Returns true when any claim failed.
///
/// Display only: every value and every verdict is computed in
/// [`reference_case`], so this function has no empirical logic to get wrong.
fn run_reference_case_public(
    base_dir: &Path,
    extract_path: Option<&Path>,
    json: bool,
) -> Result<bool> {
    let result = reference_case::run_public(base_dir, extract_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(result.failed());
    }

    println!("PUBLIC ARTICLE VERIFICATION");
    println!("experiment : {}", result.experiment_id);
    println!("extract    : {}", result.extract_path);
    println!();
    for c in &result.claims {
        println!(
            "{:<28} {:<12} {:<5} {}",
            c.claim,
            c.value,
            match c.status {
                reference_case::ClaimStatus::Pass => "PASS",
                reference_case::ClaimStatus::Fail => "FAIL",
            },
            c.basis.as_str()
        );
        if let Some(detail) = &c.detail {
            println!("{:<28} └─ {}", "", detail);
        }
    }

    let counts = reference_case::basis_counts(&result.claims);
    println!();
    println!(
        "{}/{} claims verified ({})",
        result.passed_count(),
        result.claims.len(),
        counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!(
        "Public mode verifies the published Rust evidence extract. It does not rebuild the\n\
         original 30 recorded provider responses because raw captures are not published:\n\
         they carry the requester's wallet pubkey. Claims marked `recomputed` are re-derived\n\
         by the verifier from published inputs; `cross-checked` claims are re-aggregated from\n\
         the extract's own per-request detail; `attested` claims can only be confirmed by\n\
         `--from-recorded-run` on a machine holding the original captures."
    );
    Ok(result.failed())
}

/// Rebuild the snapshot from the recorded run and diff it. Returns true on
/// any divergence.
async fn run_reference_case_rebuild(
    base_dir: &Path,
    extract_path: Option<&Path>,
    json: bool,
) -> Result<bool> {
    let result = reference_case::run_local_rebuild(base_dir, extract_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(!result.matches());
    }

    println!("LOCAL FULL REBUILD");
    println!("recorded run : {}", result.recorded_run);
    println!("published    : {}", result.published_extract);
    println!();
    if result.matches() {
        println!("regenerated extract matches the tracked publication extract exactly");
    } else {
        println!("DIVERGENCE — {} field(s) differ:", result.differences.len());
        for d in &result.differences {
            println!("  {}", d.path);
            println!("    regenerated: {}", d.regenerated);
            println!("    published  : {}", d.published);
        }
    }

    // The same production path a normal user runs, on one recorded response.
    if let Some(response) = reference_case::first_recorded_response(base_dir) {
        let report = reference_case::verify_reference_artifact(base_dir, &response).await?;
        let s = &report.summary;
        println!();
        println!(
            "canonical pipeline on {}:",
            response.file_name().unwrap_or_default().to_string_lossy()
        );
        for r in &report.results {
            println!("  {:<14} {}", r.status.as_str(), r.check_id);
        }
        println!(
            "  PASS={} FAIL={} CANDIDATE={} UNKNOWN={} NOT_APPLICABLE={}",
            s.pass, s.fail, s.candidate, s.unknown, s.not_applicable
        );
        if report.has_failures() {
            return Ok(true);
        }
    }

    println!();
    println!(
        "Chain proved: recorded raw artifacts → Rust analysis → evidence extract →\n\
         tracked publication extract. No Python, no frontend, no network request."
    );
    Ok(!result.matches())
}

fn parse_provider(name: Option<&str>) -> Result<Option<ProviderId>> {
    match name {
        None => Ok(None),
        Some(n) => ProviderId::parse(n).map(Some).with_context(|| {
            format!("unknown provider '{n}' (expected dflow, jupiter or generic)")
        }),
    }
}

/// Accepts either a lineage directory written by `extract` or a lineage.json
/// path. The sibling context.json is required: checks read the normalized
/// context, and reconstructing one from a bundle would guess at stages.
fn load_lineage_dir(path: &Path) -> Result<(ExecutionContext, LineageBundle)> {
    let (context_path, lineage_path) = if path.is_dir() {
        (path.join("context.json"), path.join("lineage.json"))
    } else {
        let parent = path.parent().unwrap_or(Path::new("."));
        (parent.join("context.json"), path.to_path_buf())
    };

    let ctx: ExecutionContext = serde_json::from_str(
        &std::fs::read_to_string(&context_path)
            .with_context(|| format!("read {}", context_path.display()))?,
    )
    .with_context(|| format!("parse {}", context_path.display()))?;
    let bundle: LineageBundle = serde_json::from_str(
        &std::fs::read_to_string(&lineage_path)
            .with_context(|| format!("read {}", lineage_path.display()))?,
    )
    .with_context(|| format!("parse {}", lineage_path.display()))?;

    Ok((ctx, bundle))
}

fn print_trace(ctx: &ExecutionContext, bundle: &LineageBundle) {
    println!("artifact_id : {}", ctx.provenance.artifact_id);
    println!("provider    : {}", ctx.provider);
    println!(
        "stages      : {}",
        ctx.stages_present()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" → ")
    );

    println!("\nprovenance");
    for p in &ctx.provenance.stages {
        println!(
            "  {:<26} {} {}",
            p.stage.as_str(),
            p.source,
            p.source_path.clone().unwrap_or_default()
        );
    }

    println!("\nlineage links");
    for l in &bundle.links {
        println!("  [{}] {}", l.relationship, l.id);
        println!("      {} → {}", l.subject, l.object);
        println!(
            "      evidence_level={:?} ceiling: {}",
            l.evidence_level, l.claim_ceiling
        );
        if !l.evidence.is_empty() {
            println!("      evidence: {}", l.evidence.join(", "));
        }
    }

    if !bundle.unresolved.is_empty() {
        println!("\nunresolved");
        for u in &bundle.unresolved {
            println!("  {:<34} {}", u.field, u.reason);
        }
    }
}

#[derive(Parser)]
#[command(name = "onchain-execution-lineage")]
#[command(
    about = "Read-only toolkit for reconstructing and verifying execution lineage: \
             extract → trace → verify"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Raw provider evidence → normalized ExecutionContext + LineageBundle.
    ///
    /// Offline unless --rpc-url is given. Output paths are derived from a
    /// content hash, so re-running on the same bytes rewrites the same files.
    Extract {
        /// dflow | jupiter | generic. Omit to detect from the response shape.
        #[arg(long)]
        provider: Option<String>,
        /// Raw provider response JSON (e.g. a recorded /order response).
        #[arg(long)]
        response: Option<PathBuf>,
        /// Base64 transaction file; overrides any payload inline in the response.
        #[arg(long)]
        transaction: Option<PathBuf>,
        /// Optional manifest, used only for artifact identity and provenance.
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        signature: Option<String>,
        /// Enables ALT resolution and, with --enrich-settlement, RPC lookups.
        #[arg(long)]
        rpc_url: Option<String>,
        #[arg(long, default_value_t = false)]
        enrich_settlement: bool,
        /// Defaults to artifacts/lineage/<artifact-id>/.
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Print the lineage bundle instead of writing files.
        #[arg(long, default_value_t = false)]
        stdout: bool,
    },
    /// Reproducibility entry point for the DFlow slippage reference case.
    ///
    /// Public mode verifies the tracked evidence snapshot. With
    /// --from-recorded-run it regenerates that snapshot from the original
    /// recorded captures and compares the two. Neither mode makes a network
    /// request.
    ReferenceCase {
        /// Rebuild the snapshot from the private recorded run and diff it.
        #[arg(long, default_value_t = false)]
        from_recorded_run: bool,
        /// Publication extract. Defaults to the tracked path.
        #[arg(long)]
        extract: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        base_dir: PathBuf,
        /// Emit the machine-readable result instead of the table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Run cross-layer checks against a lineage bundle or a raw response.
    Verify {
        /// Lineage directory or lineage.json produced by `extract`.
        #[arg(long)]
        lineage: Option<PathBuf>,
        /// Extract first, then verify.
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        response: Option<PathBuf>,
        #[arg(long)]
        transaction: Option<PathBuf>,
        #[arg(long)]
        signature: Option<String>,
        #[arg(long)]
        rpc_url: Option<String>,
        #[arg(long, default_value_t = false)]
        enrich_settlement: bool,
        /// Write the verification report here as well as printing it.
        #[arg(long)]
        out_json: Option<PathBuf>,
    },
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
    /// Explain provenance and cross-stage relationships inside a lineage
    /// bundle (`--lineage`), or build one from manifest / provider JSON / tx /
    /// signature (every other flag; the original behaviour).
    Trace {
        /// Lineage directory or lineage.json to explain.
        #[arg(long)]
        lineage: Option<PathBuf>,
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
        /// Normalized ExecutionContext for the same run; `verify` reads this.
        #[arg(long, default_value = "artifacts/analysis/context.json")]
        out_context: PathBuf,
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
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        #[arg(long, default_value_t = true)]
        resolve_alts: bool,
    },
    /// Bracketed /order route-stability experiment (A1/T/A2).
    RouteBracket {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = ".")]
        base_dir: PathBuf,
        #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
        rpc_url: String,
        #[arg(long, default_value_t = true)]
        resolve_alts: bool,
        /// Rebuild the public evidence extract from an already-recorded run and
        /// exit. Makes no network requests: reads the stored report, responses
        /// and transactions only.
        #[arg(long, default_value_t = false)]
        rebuild_extract_only: bool,
    },
}

/// Parse argv and run the CLI. Both binaries are thin shims over this.
pub async fn run() -> Result<()> {
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
        Commands::Extract {
            provider,
            response,
            transaction,
            manifest,
            signature,
            rpc_url,
            enrich_settlement,
            out_dir,
            stdout,
        } => {
            let loaded_manifest = match &manifest {
                Some(p) => Some(artifact::ArtifactManifest::load_path(p)?),
                None => None,
            };
            let result = extract::extract(extract::ExtractInputs {
                provider: parse_provider(provider.as_deref())?,
                response_path: response.as_deref(),
                transaction_b64_path: transaction.as_deref(),
                manifest: loaded_manifest.as_ref(),
                signature: signature.as_deref(),
                rpc: rpc_url.map(RpcContext::new),
                enrich_settlement,
            })
            .await?;

            if stdout {
                println!("{}", result.lineage.to_canonical_json()?);
            } else {
                let dir = out_dir.unwrap_or_else(|| result.default_out_dir());
                let written = result.write_to_dir(&dir)?;
                println!("artifact_id={}", result.context.provenance.artifact_id);
                println!(
                    "stages={}",
                    result
                        .context
                        .stages_present()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                println!("wrote {}", written.context.display());
                println!("wrote {}", written.lineage.display());
            }
        }
        Commands::ReferenceCase {
            from_recorded_run,
            extract: extract_path,
            base_dir,
            json,
        } => {
            let failed = if from_recorded_run {
                run_reference_case_rebuild(&base_dir, extract_path.as_deref(), json).await?
            } else {
                run_reference_case_public(&base_dir, extract_path.as_deref(), json)?
            };
            if failed {
                std::process::exit(1);
            }
        }
        Commands::Verify {
            lineage,
            provider,
            response,
            transaction,
            signature,
            rpc_url,
            enrich_settlement,
            out_json,
        } => {
            let (ctx, bundle) = match lineage {
                Some(path) => load_lineage_dir(&path)?,
                None => {
                    let result = extract::extract(extract::ExtractInputs {
                        provider: parse_provider(provider.as_deref())?,
                        response_path: response.as_deref(),
                        transaction_b64_path: transaction.as_deref(),
                        manifest: None,
                        signature: signature.as_deref(),
                        rpc: rpc_url.map(RpcContext::new),
                        enrich_settlement,
                    })
                    .await?;
                    (result.context, result.lineage)
                }
            };

            let report = checks::verify(&ctx, &bundle);
            for r in &report.results {
                println!(
                    "{:<14} {:<40} {}",
                    r.status.as_str(),
                    r.check_id,
                    r.explanation
                );
            }
            let s = &report.summary;
            println!(
                "\nPASS={} FAIL={} CANDIDATE={} UNKNOWN={} NOT_APPLICABLE={}",
                s.pass, s.fail, s.candidate, s.unknown, s.not_applicable
            );
            if let Some(path) = out_json {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, report.to_canonical_json()?)?;
                println!("wrote {}", path.display());
            }
        }
        Commands::Trace {
            lineage: Some(path),
            ..
        } => {
            let (ctx, bundle) = load_lineage_dir(&path)?;
            print_trace(&ctx, &bundle);
        }
        Commands::Trace {
            lineage: None,
            manifest,
            provider_json,
            transaction,
            signature,
            rpc_url,
            resolve_alts,
            enrich_settlement,
            out_json,
            out_context,
            out_md,
            out_csv,
            out_dot,
        } => {
            let loaded_manifest = match &manifest {
                Some(p) => Some(artifact::ArtifactManifest::load_path(p)?),
                None => None,
            };
            let result = trace::build_trace_full(trace::TraceInputs {
                manifest: loaded_manifest.as_ref(),
                provider_json_path: provider_json.as_deref(),
                transaction_b64_path: transaction.as_deref(),
                signature: signature.as_deref(),
                rpc_url: &rpc_url,
                resolve_alts,
                enrich_settlement,
            })
            .await?;
            let bundle = &result.lineage;

            for path in [&out_json, &out_md, &out_csv, &out_dot, &out_context] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out_json, bundle.to_canonical_json()?)?;
            // The context is what `verify` reads; writing it makes a traced
            // bundle checkable without re-extracting.
            std::fs::write(&out_context, result.context.to_canonical_json()?)?;
            report::write_markdown_report(bundle, &out_md)?;
            report::write_evidence_csv(bundle, &out_csv)?;
            report::write_dot(bundle, &out_dot)?;
            println!("wrote {}", out_json.display());
            println!("wrote {}", out_context.display());
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
            let left_b: crate::lineage_model::LineageBundle =
                serde_json::from_str(&std::fs::read_to_string(&left)?)?;
            let right_b: crate::lineage_model::LineageBundle =
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
        Commands::Experiment {
            manifest,
            base_dir,
            rpc_url,
            resolve_alts,
        } => {
            let report =
                experiment::run_experiment_with_rpc(&manifest, &base_dir, &rpc_url, resolve_alts)
                    .await?;
            println!(
                "experiment_id={} endpoint=/{} treatments={} baseline={}",
                report.experiment_id,
                report.endpoint_path,
                report.runs.len(),
                report.baseline_value
            );
            for run in &report.runs {
                println!(
                    "  value={} http={:?} construction={} tx={} venue={:?} raw={}",
                    run.treatment_value,
                    run.http_status,
                    run.construction_status,
                    run.transaction_present,
                    run.route_venue,
                    run.raw_path
                );
            }
            if !report.canonical_tx_diffs_vs_baseline.is_empty() {
                println!("canonical_tx_diffs_vs_baseline:");
                for (k, d) in &report.canonical_tx_diffs_vs_baseline {
                    println!(
                        "  vs {k}: stable_aside_from_blockhash={} data_hashes_equal={}",
                        d.stable_aside_from_blockhash, d.instruction_data_hashes.equal
                    );
                }
            }
            println!(
                "wrote experiment_report.json / experiment_report.md under the manifest output_path"
            );
        }
        Commands::RouteBracket {
            manifest,
            base_dir,
            rpc_url,
            resolve_alts,
            rebuild_extract_only,
        } => {
            if rebuild_extract_only {
                let loaded = route_bracket::BracketManifest::load_path(&manifest)?;
                let out_dir = base_dir.join(&loaded.output_path);
                let recorded = std::fs::read_to_string(out_dir.join("experiment_report.json"))?;
                let report: route_bracket::BracketExperimentReport =
                    serde_json::from_str(&recorded)?;
                evidence_extract::write(&report, &base_dir, &out_dir)?;
                println!(
                    "rebuilt {}",
                    out_dir.join("evidence_extract.json").display()
                );
                return Ok(());
            }
            let report = route_bracket::run_route_bracket_experiment(
                &manifest,
                &base_dir,
                &rpc_url,
                resolve_alts,
            )
            .await?;
            println!(
                "experiment_id={} batches={} requests={} exact_route_stable={} eligible={}",
                report.experiment_id,
                report.attempted_batches,
                report.total_requests,
                report.exact_route_stable_batches,
                report.eligible_instruction_diff_batches
            );
            for b in &report.batches {
                println!(
                    "  batch{} T={} complete={} exact={} topo={:?}/{:?} eligible={} reasons={}",
                    b.batch_index,
                    b.treatment_slippage_bps,
                    b.complete,
                    b.exact_route_stable,
                    b.topology_stable_a1_t,
                    b.topology_stable_t_a2,
                    b.eligible_for_instruction_diff,
                    b.ineligibility_reasons.join("; ")
                );
            }
            println!("wrote artifacts under manifest output_path");
        }
    }

    Ok(())
}
