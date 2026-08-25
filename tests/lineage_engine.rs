//! Integration tests for the lineage engine (offline).
//!
//! Public fixtures live under `tests/fixtures/`.
//! Private research corpus lives under `.local/corpus/` (gitignored).

use onchain_execution_lineage::artifact::ArtifactManifest;
use onchain_execution_lineage::diff::diff_bundles;
use onchain_execution_lineage::evidence::{AttributionClaim, EvidenceLevel};
use onchain_execution_lineage::fingerprint::{
    fingerprint_group, n1_refuses_unique_promotion, CorpusEntry, CorpusManifest,
};
use onchain_execution_lineage::lineage_model::{CaptureMetadata, LineageBundle};
use onchain_execution_lineage::providers;
use onchain_execution_lineage::transaction::decode_base64_transaction;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn local_corpus() -> PathBuf {
    root().join(".local/corpus")
}

#[test]
fn manifest_valid_public_fixture_loads_and_hashes() {
    let m =
        ArtifactManifest::load_path(&root().join("tests/fixtures/manifests/valid_dflow_dev.json"))
            .unwrap();
    m.verify_raw_hash(&root()).unwrap();
}

#[test]
fn manifest_rejects_unsupported_schema_file() {
    let err = ArtifactManifest::load_path(
        &root().join("tests/fixtures/manifests/invalid_schema_version.json"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unsupported"));
}

#[test]
fn dflow_adapter_preserves_missing_transaction() {
    let raw = std::fs::read_to_string(root().join("tests/fixtures/dev_quote_usdc_sol_no_tx.json"))
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut bundle = LineageBundle::new(CaptureMetadata {
        artifact_id: "t".into(),
        provider: String::new(),
        surface: "dev_quote".into(),
        captured_at_utc: "t".into(),
        pair: "USDC/SOL".into(),
    });
    let name = providers::normalize_provider_json(&value, &mut bundle).unwrap();
    assert_eq!(name, "dflow");
    assert!(!bundle.transaction_construction.present);
    assert!(bundle.unresolved.iter().any(|u| u.field == "transaction"));
}

#[test]
fn jtx_adapter_reads_sanitized_capture() {
    let path = local_corpus()
        .join("fixtures/jtx_dflow/jtx_unsigned_tx_capture_sanitized_20260729T132014Z.json");
    if !path.exists() {
        eprintln!("skip: private corpus missing at {}", path.display());
        return;
    }
    let raw = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut bundle = LineageBundle::new(CaptureMetadata {
        artifact_id: "jtx".into(),
        ..Default::default()
    });
    let name = providers::normalize_provider_json(&value, &mut bundle).unwrap();
    assert_eq!(name, "jtx");
    assert_eq!(
        bundle.route.provider_route_label.as_deref(),
        Some("DFlow JIT Router")
    );
}

#[test]
fn jtx_unsigned_tx_decodes_with_dflow_program() {
    let path =
        local_corpus().join("fixtures/jtx_dflow/jtx_unsigned_tx_base64_20260729T132014Z.txt");
    if !path.exists() {
        eprintln!("skip: private corpus missing at {}", path.display());
        return;
    }
    let b64 = std::fs::read_to_string(&path).unwrap();
    let decoded = decode_base64_transaction(&b64).unwrap();
    assert_eq!(decoded.transaction_type, "v0_with_alt");
    let dflow = decoded
        .instructions
        .iter()
        .filter(|i| i.program_label == "dflow_aggregator_v4")
        .count();
    assert_eq!(dflow, 2);
}

#[test]
fn unsigned_bundle_cannot_claim_settlement() {
    let mut b = LineageBundle::new(CaptureMetadata {
        artifact_id: "u".into(),
        ..Default::default()
    });
    b.settlement.applicable = false;
    b.push_claim(AttributionClaim::new(
        "settlement",
        "settled_as",
        "success",
        EvidenceLevel::ResolvedFromRpc,
        "u",
        "illegal",
    ));
    assert!(b.assert_unsigned_has_no_settlement_claims().is_err());
}

#[test]
fn fingerprint_refuses_n1_promotion() {
    let dir = std::env::temp_dir().join(format!("fp_n1_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut b = LineageBundle::new(CaptureMetadata {
        artifact_id: "only".into(),
        provider: "jtx".into(),
        ..Default::default()
    });
    b.transaction_construction.program_labels = vec!["unique_label_x".into()];
    let path = dir.join("only.json");
    std::fs::write(&path, b.to_canonical_json().unwrap()).unwrap();
    let corpus = CorpusManifest {
        schema_version: "1.0.0".into(),
        entries: vec![CorpusEntry {
            artifact_id: "only".into(),
            group: "jtx_dflow".into(),
            lineage_path: "only.json".into(),
            smoke_or_synthetic: false,
        }],
    };
    let report = fingerprint_group(&corpus, &dir, "jtx_dflow").unwrap();
    assert!(n1_refuses_unique_promotion(&report));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_marks_unique_program_as_candidate_not_fingerprint() {
    let mut left = LineageBundle::new(CaptureMetadata {
        artifact_id: "L".into(),
        ..Default::default()
    });
    left.transaction_construction.program_ids = vec!["AAA".into()];
    let mut right = LineageBundle::new(CaptureMetadata {
        artifact_id: "R".into(),
        ..Default::default()
    });
    right.transaction_construction.program_ids = vec!["BBB".into()];
    let d = diff_bundles(&left, &right);
    assert!(d.entries.iter().any(|e| {
        matches!(
            e.class,
            onchain_execution_lineage::diff::DiffClass::AppCandidate
        ) && e.note.contains("candidate")
    }));
}

#[test]
fn deterministic_lineage_json() {
    let b = LineageBundle::new(CaptureMetadata {
        artifact_id: "d".into(),
        provider: "dflow".into(),
        surface: "dev".into(),
        captured_at_utc: "t".into(),
        pair: "USDC/SOL".into(),
    });
    assert_eq!(
        b.to_canonical_json().unwrap(),
        b.to_canonical_json().unwrap()
    );
}

#[test]
fn private_corpus_manifest_loads_when_present() {
    let manifest = local_corpus().join("corpus_manifest.json");
    if !Path::new(&manifest).exists() {
        eprintln!("skip: .local/corpus not present");
        return;
    }
    let c = onchain_execution_lineage::fingerprint::load_corpus(&manifest).unwrap();
    assert!(!c.entries.is_empty());
}
