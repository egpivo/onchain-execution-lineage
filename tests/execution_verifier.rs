//! End-to-end tests for the extract → lineage → verify path.
//!
//! The DFlow reference case is the recorded route-stable bracket run. No
//! network requests are made: every input is a file already in the repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use onchain_execution_lineage::adapters::{ProviderAdapter, ProviderId, RawProviderArtifact};
use onchain_execution_lineage::checks::{self, CheckStatus};
use onchain_execution_lineage::execution_context::{ExecutionContext, Stage};
use onchain_execution_lineage::extract::{self, ExtractInputs};
use onchain_execution_lineage::lineage_builder::build_lineage;
use onchain_execution_lineage::lineage_model::SettlementObservation;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A recorded DFlow `/order` response with an inline unsigned transaction.
fn reference_response() -> PathBuf {
    root().join("artifacts/experiments/dflow_order_slippage_route_stable_live/raw/b00_A1_50.json")
}

fn have_reference_case() -> bool {
    reference_response().exists()
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_onchain-execution-lineage"))
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oel_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn reference_extraction() -> extract::Extraction {
    extract::extract(ExtractInputs {
        provider: Some(ProviderId::Dflow),
        response_path: Some(&reference_response()),
        ..Default::default()
    })
    .await
    .unwrap()
}

fn status_of(report: &checks::VerificationReport, id: &str) -> CheckStatus {
    report
        .results
        .iter()
        .find(|r| r.check_id == id)
        .unwrap_or_else(|| panic!("check {id} missing from report"))
        .status
}

// ---- lineage ------------------------------------------------------------

#[tokio::test]
async fn response_plus_transaction_lineage_covers_four_stages() {
    if !have_reference_case() {
        eprintln!("skip: reference case artifacts not present");
        return;
    }
    let out = reference_extraction().await;

    assert_eq!(
        out.context.stages_present(),
        vec![
            Stage::Intent,
            Stage::ProviderResponse,
            Stage::Route,
            Stage::TransactionConstruction
        ]
    );
    assert!(out.lineage.transaction_construction.present);
    // Settlement is the one stage a recorded unsigned order cannot reach.
    assert!(!out.lineage.settlement.applicable);
}

#[tokio::test]
async fn provenance_survives_into_the_context() {
    if !have_reference_case() {
        return;
    }
    let out = reference_extraction().await;
    let stages = &out.context.provenance.stages;

    let response = stages
        .iter()
        .find(|p| p.stage == Stage::ProviderResponse)
        .expect("response provenance");
    assert_eq!(response.source, "provider_response");
    assert!(response.sha256.is_some(), "raw bytes must be hashed");
    assert!(response
        .source_path
        .as_ref()
        .unwrap()
        .ends_with("b00_A1_50.json"));

    let tx = stages
        .iter()
        .find(|p| p.stage == Stage::TransactionConstruction)
        .expect("transaction provenance");
    assert_eq!(tx.source, "provider_response_inline");
}

#[tokio::test]
async fn candidate_relationship_is_recorded_with_a_ceiling() {
    if !have_reference_case() {
        return;
    }
    let out = reference_extraction().await;

    let hit = out
        .lineage
        .links
        .iter()
        .find(|l| l.id == "response_to_transaction_bytes:out_amount")
        .expect("out_amount byte link");
    assert_eq!(hit.relationship, "candidate_byte_match");
    assert!(hit.claim_ceiling.contains("byte presence only"));
    // Reproduces the published extract: one site, instruction 2, offset 99.
    assert!(hit.evidence.iter().any(|e| e == "instruction[2]+99"));

    // The threshold is not recoverable from the payload bytes, and the link
    // says non-recovery is not absence.
    let miss = out
        .lineage
        .links
        .iter()
        .find(|l| l.id == "response_to_transaction_bytes:other_amount_threshold")
        .expect("threshold byte link");
    assert_eq!(miss.relationship, "not_recoverable");
    assert!(miss.claim_ceiling.contains("not evidence of absence"));
}

#[tokio::test]
async fn missing_stages_produce_no_links_into_those_stages() {
    let dir = tmp_dir("missing_stages");
    let path = dir.join("quote_only.json");
    std::fs::write(
        &path,
        r#"{"inputMint":"A","inAmount":"1","outputMint":"B","outAmount":"2",
            "otherAmountThreshold":"1","minOutAmount":"1","slippageBps":50,
            "routePlan":[],"requestId":"r"}"#,
    )
    .unwrap();

    let out = extract::extract(ExtractInputs {
        provider: Some(ProviderId::Dflow),
        response_path: Some(&path),
        ..Default::default()
    })
    .await
    .unwrap();

    assert!(out.context.transaction.is_none());
    assert!(!out
        .lineage
        .links
        .iter()
        .any(|l| l.to_stage == Stage::TransactionConstruction));
    assert!(!out
        .lineage
        .links
        .iter()
        .any(|l| l.to_stage == Stage::Settlement));
}

// ---- verification -------------------------------------------------------

#[tokio::test]
async fn reference_case_verification_statuses() {
    if !have_reference_case() {
        return;
    }
    let out = reference_extraction().await;
    let report = checks::verify(&out.context, &out.lineage);

    // PASS: response-level arithmetic, reproduced independently of the
    // publication extract.
    assert_eq!(
        status_of(&report, "dflow.slippage_threshold_arithmetic"),
        CheckStatus::Pass
    );
    assert_eq!(
        status_of(&report, "generic.transaction_decode"),
        CheckStatus::Pass
    );
    // CANDIDATE: the outAmount byte relationship, and nothing stronger.
    assert_eq!(
        status_of(&report, "solana.candidate_byte_search"),
        CheckStatus::Candidate
    );
    // UNKNOWN: lookup tables were referenced but not resolved offline.
    assert_eq!(
        status_of(&report, "solana.alt_resolution"),
        CheckStatus::Unknown
    );
    // NOT_APPLICABLE: no settlement input.
    for id in [
        "settlement.landed_status",
        "settlement.runtime_program_invocation",
        "settlement.realized_output_vs_threshold",
        "settlement.fees_and_compute_units",
    ] {
        assert_eq!(status_of(&report, id), CheckStatus::NotApplicable);
    }

    assert!(!report.has_failures());
}

#[tokio::test]
async fn contradicted_evidence_fails() {
    let dir = tmp_dir("fail_case");
    let path = dir.join("bad_threshold.json");
    // Threshold is not ceil(out * (10000 - bps) / 10000) under either
    // rounding convention, and the route names a different input mint.
    std::fs::write(
        &path,
        r#"{"inputMint":"MintA","inAmount":"100","outputMint":"MintB","outAmount":"1000",
            "otherAmountThreshold":"999999","minOutAmount":"999999","slippageBps":50,
            "routePlan":[{"venue":"V","inputMint":"MintZ","outputMint":"MintB"}],
            "requestId":"r"}"#,
    )
    .unwrap();

    let out = extract::extract(ExtractInputs {
        provider: Some(ProviderId::Dflow),
        response_path: Some(&path),
        ..Default::default()
    })
    .await
    .unwrap();
    let report = checks::verify(&out.context, &out.lineage);

    assert_eq!(
        status_of(&report, "dflow.slippage_threshold_arithmetic"),
        CheckStatus::Fail
    );
    assert_eq!(
        status_of(&report, "generic.input_mint_consistency"),
        CheckStatus::Fail
    );
    assert!(report.has_failures());
}

#[tokio::test]
async fn a_candidate_never_reports_as_pass() {
    if !have_reference_case() {
        return;
    }
    let out = reference_extraction().await;
    let report = checks::verify(&out.context, &out.lineage);

    let candidate = report
        .results
        .iter()
        .find(|r| r.check_id == "solana.candidate_byte_search")
        .unwrap();
    assert_ne!(candidate.status, CheckStatus::Pass);
    assert!(candidate.evidence_ceiling.contains("byte presence only"));

    // And no PASS anywhere cites a byte site as its evidence: byte-level
    // matches may only ever back a CANDIDATE.
    assert!(out
        .lineage
        .links
        .iter()
        .any(|l| l.relationship == "candidate_byte_match"));
    for r in report
        .results
        .iter()
        .filter(|r| r.status == CheckStatus::Pass)
    {
        assert!(
            !r.evidence
                .iter()
                .any(|e| e.starts_with("instruction[") && e.contains('+')),
            "check {} passed on byte-site evidence",
            r.check_id
        );
    }
}

#[tokio::test]
async fn unknown_is_distinct_from_not_applicable() {
    if !have_reference_case() {
        return;
    }
    let out = reference_extraction().await;
    let report = checks::verify(&out.context, &out.lineage);

    // Referenced-but-unresolved tables are UNKNOWN...
    assert_eq!(
        status_of(&report, "solana.alt_resolution"),
        CheckStatus::Unknown
    );

    // ...whereas a transaction with no tables at all is NOT_APPLICABLE.
    let dir = tmp_dir("no_alt");
    let path = dir.join("quote.json");
    std::fs::write(&path, r#"{"routePlan":[],"requestId":"r"}"#).unwrap();
    let no_tx = extract::extract(ExtractInputs {
        provider: Some(ProviderId::Dflow),
        response_path: Some(&path),
        ..Default::default()
    })
    .await
    .unwrap();
    let report = checks::verify(&no_tx.context, &no_tx.lineage);
    assert_eq!(
        status_of(&report, "solana.alt_resolution"),
        CheckStatus::NotApplicable
    );
}

#[test]
fn a_signature_alone_unlocks_no_settlement_claim() {
    let mut ctx = ExecutionContext::new(ProviderId::Dflow, "sig_only");
    ctx = ctx.with_settlement(SettlementObservation {
        applicable: false,
        signature: Some("5xYsignature".into()),
        ..Default::default()
    });
    let lineage = build_lineage(&ctx).unwrap();
    let report = checks::verify(&ctx, &lineage);

    for r in report
        .results
        .iter()
        .filter(|r| r.check_id.starts_with("settlement."))
    {
        assert_eq!(
            r.status,
            CheckStatus::NotApplicable,
            "{} claimed something without settlement evidence",
            r.check_id
        );
    }
    lineage.assert_unsigned_has_no_settlement_claims().unwrap();
}

// ---- provider support levels -------------------------------------------

#[test]
fn jupiter_reports_its_unsupported_fields_rather_than_faking_parity() {
    let raw = RawProviderArtifact::from_value(serde_json::json!({
        "inputMint": "A", "outputMint": "B", "inAmount": "1", "outAmount": "2",
        "otherAmountThreshold": "1", "swapMode": "ExactIn", "slippageBps": 50,
        "routePlan": [],
    }));
    let e = onchain_execution_lineage::adapters::jupiter::JupiterAdapter
        .extract(&raw)
        .unwrap();
    assert!(e.response.min_out_amount.is_none());
    assert!(e.unsupported.iter().any(|u| u.field == "minOutAmount"));

    let ctx = ExecutionContext::new(ProviderId::Jupiter, "jup").with_extraction(e);
    let lineage = build_lineage(&ctx).unwrap();
    let report = checks::verify(&ctx, &lineage);

    // No DFlow check runs for a Jupiter artifact.
    assert!(!report
        .results
        .iter()
        .any(|r| r.check_id.starts_with("dflow.")));
    // And the unsupported field is visible in the lineage, not swallowed.
    assert!(lineage.unresolved.iter().any(|u| u.field == "minOutAmount"));
}

// ---- CLI ----------------------------------------------------------------

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn cli_extract_is_deterministic() {
    if !have_reference_case() {
        return;
    }
    let dir = tmp_dir("cli_extract");
    let run = |out: &Path| {
        let status = bin()
            .args([
                "extract",
                "--provider",
                "dflow",
                "--response",
                reference_response().to_str().unwrap(),
                "--out-dir",
                out.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
    };

    let a = dir.join("a");
    let b = dir.join("b");
    run(&a);
    run(&b);

    assert_eq!(read(&a.join("lineage.json")), read(&b.join("lineage.json")));
    assert_eq!(read(&a.join("context.json")), read(&b.join("context.json")));
}

#[test]
fn cli_trace_and_verify_read_an_extracted_lineage() {
    if !have_reference_case() {
        return;
    }
    let dir = tmp_dir("cli_trace_verify");
    assert!(bin()
        .args([
            "extract",
            "--provider",
            "dflow",
            "--response",
            reference_response().to_str().unwrap(),
            "--out-dir",
            dir.to_str().unwrap(),
        ])
        .status()
        .unwrap()
        .success());

    let trace = bin()
        .args(["trace", "--lineage", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(trace.status.success());
    let trace_out = String::from_utf8_lossy(&trace.stdout);
    assert!(trace_out.contains("candidate_byte_match"));
    assert!(trace_out.contains("provider    : dflow"));

    let verify = bin()
        .args(["verify", "--lineage", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(verify.status.success());
    let verify_out = String::from_utf8_lossy(&verify.stdout);
    assert!(verify_out.contains("dflow.slippage_threshold_arithmetic"));
    assert!(verify_out.contains("CANDIDATE"));
    assert!(verify_out.contains("NOT_APPLICABLE"));
}

#[test]
fn cli_verify_works_directly_from_a_raw_provider_response() {
    if !have_reference_case() {
        return;
    }
    let out = bin()
        .args([
            "verify",
            "--provider",
            "dflow",
            "--response",
            reference_response().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FAIL=0"));
    assert!(text.contains("PASS "));
}
