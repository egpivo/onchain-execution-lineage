//! Regenerates the public evidence extract from the recorded bracket report and
//! checks the published values. This is the guard that keeps the article, the
//! figures and the lab honest: they all read the extract, and the extract is
//! only ever produced here.

use std::path::PathBuf;

use onchain_execution_lineage::evidence_extract;
use onchain_execution_lineage::route_bracket::BracketExperimentReport;

fn tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn report() -> Option<(BracketExperimentReport, PathBuf)> {
    let base = tree();
    let path = base.join(
        "artifacts/experiments/dflow_order_slippage_route_stable_live/experiment_report.json",
    );
    if !path.exists() {
        eprintln!("skipping: {} not present (regenerable run)", path.display());
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("read bracket report");
    Some((
        serde_json::from_str(&text).expect("parse bracket report"),
        base,
    ))
}

#[test]
fn extract_reproduces_the_published_values() {
    let Some((report, base)) = report() else {
        return;
    };
    let extract = evidence_extract::build(&report, &base).expect("build extract");

    let identity = &extract.threshold_identity;
    assert_eq!(identity.observations, 30, "total requests");
    assert_eq!(identity.exact_matches, 30, "ceil identity");
    assert_eq!(identity.floor_variant_matches, 0, "floor alternative");
    assert!(identity.min_out_equals_threshold_all);

    assert_eq!(extract.total_batches, 10);
    assert_eq!(extract.total_requests, 30);
    assert_eq!(extract.eligible_batches, vec![1, 2, 5, 8, 9]);

    let candidate = &extract.candidate_result;
    assert_eq!(candidate.transactions_searched, 15, "5 brackets x 3 roles");
    assert!(candidate.quote_matched_in_all, "quote found in every one");
    assert!(
        candidate.quote_site_unique_in_all,
        "one site per transaction"
    );
    assert_eq!(
        candidate.threshold_sites_total, 0,
        "threshold never matched"
    );
    assert_eq!(
        candidate.difference_sites_total, 0,
        "difference never matched"
    );
    assert_eq!(candidate.quote_sites.len(), 1, "one site across all 15");
    assert_eq!(candidate.quote_sites[0].instruction_index, 2);
    assert_eq!(candidate.quote_sites[0].byte_offset, 99);

    // Same-treatment control: anchors share a setting, the quote still moved.
    let pairs = &extract.anchor_control;
    assert_eq!(pairs.len(), 5);
    let differing = pairs.iter().filter(|p| p.quote_differs).count();
    assert_eq!(differing, 5, "quote drifted in every anchor pair");
    assert!(
        pairs
            .iter()
            .all(|p| p.candidate_carries_own_quote && p.route_same && p.topology_same),
        "with dS = 0 the site still carried each response's own quote"
    );
}
