//! Offline controlled-experiment integration tests (fixture mode).

use dflow_lineage::experiment::{run_experiment, ExperimentManifest};
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
    assert!(report
        .mechanism
        .not_observable_without_settlement
        .iter()
        .any(|s| s.contains("realized")));
    let out = root().join("artifacts/experiments/fee_injection_synthetic/experiment_report.md");
    let md = std::fs::read_to_string(out).unwrap();
    assert!(md.contains("Controlled experiments are not simulated fills"));
    assert!(md.contains("## Candidate mechanism"));
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
        assert_eq!(m.mode, dflow_lineage::experiment::ExperimentMode::Fixture);
    }
}
