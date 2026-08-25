//! Offline controlled-experiment integration tests (fixture mode).

use onchain_execution_lineage::experiment::{run_experiment, ExperimentManifest};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn fee_injection_fixture_experiment_runs() {
    let manifest = root().join("tests/fixtures/experiments/fee_injection_synthetic.json");
    let report = run_experiment(&manifest, &root()).await.unwrap();
    assert_eq!(report.experiment_id, "fee_injection_synthetic");
    assert_eq!(report.runs.len(), 3);
    assert!(report
        .mechanism
        .changed
        .iter()
        .any(|c| c.contains("platformFee")));
    assert!(report.runs.iter().all(|r| {
        r.within_response_accounting_identity == Some(true)
            && r.transaction_presence_note == "quote-stage only"
    }));
    assert!(report
        .mechanism
        .unchanged
        .iter()
        .any(|u| u.contains("within-response accounting identity")));
    assert!(report
        .mechanism
        .unchanged
        .iter()
        .any(|u| u.contains("route venue/marketKey: unchanged in this run")));
    let out = root().join("artifacts/experiments/fee_injection_synthetic/experiment_report.md");
    let md = std::fs::read_to_string(out).unwrap();
    assert!(md.contains("within-response accounting identity"));
    assert!(md.contains("quote-stage only"));
    assert!(md.contains("## Per-treatment response observations"));
}

#[tokio::test]
async fn slippage_encoding_fixture_experiment_runs() {
    let manifest = root().join("tests/fixtures/experiments/slippage_encoding_synthetic.json");
    let report = run_experiment(&manifest, &root()).await.unwrap();
    assert_eq!(report.runs.len(), 3);
    assert!(report
        .mechanism
        .changed
        .iter()
        .any(|c| c.contains("otherAmountThreshold")));
    for r in &report.runs {
        assert!(r.implied_threshold_distance_bps.is_some());
        assert_eq!(r.transaction_presence_note, "quote-stage only");
    }
}

#[tokio::test]
async fn size_reroute_fixture_experiment_runs() {
    let manifest = root().join("tests/fixtures/experiments/size_reroute_synthetic.json");
    let report = run_experiment(&manifest, &root()).await.unwrap();
    assert_eq!(report.runs.len(), 3);
    assert!(report.mechanism.changed.iter().any(|c| c.contains("route")));
    assert!(!report.diffs_vs_baseline.is_empty());
}

#[test]
fn public_experiment_manifests_validate() {
    for name in [
        "fee_injection_synthetic.json",
        "slippage_encoding_synthetic.json",
        "size_reroute_synthetic.json",
    ] {
        let m =
            ExperimentManifest::load_path(&root().join("tests/fixtures/experiments").join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            m.mode,
            onchain_execution_lineage::experiment::ExperimentMode::Fixture
        );
    }
}
