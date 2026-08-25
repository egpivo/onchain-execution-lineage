//! There is exactly one lineage-construction path.
//!
//! `extract` (raw response) and `trace` (manifest ingestion) must resolve to
//! the same canonical `LineageBundle`. If a second lineage builder ever
//! appears, these tests are what fails.

use std::path::PathBuf;

use onchain_execution_lineage::artifact::{
    sha256_file, ArtifactManifest, EndpointType, SanitizationStatus, TransactionPresence,
};
use onchain_execution_lineage::evidence::ARTIFACT_SCHEMA_VERSION;
use onchain_execution_lineage::execution_context::Stage;
use onchain_execution_lineage::extract::{self, ExtractInputs};
use onchain_execution_lineage::lineage_model::LineageBundle;
use onchain_execution_lineage::trace::{self, TraceInputs};
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn reference_response() -> PathBuf {
    root().join("artifacts/experiments/dflow_order_slippage_route_stable_live/raw/b00_A1_50.json")
}

fn have_reference_case() -> bool {
    reference_response().exists()
}

/// A manifest describing the reference response, with a real content hash.
fn reference_manifest() -> ArtifactManifest {
    ArtifactManifest {
        schema_version: ARTIFACT_SCHEMA_VERSION.into(),
        artifact_id: "art_reference_b00_A1_50".into(),
        capture_run_id: "dflow_order_slippage_route_stable_live".into(),
        matched_set_id: None,
        provider: "dflow".into(),
        surface: "order".into(),
        endpoint_type: EndpointType::Production,
        endpoint_hostname: "quote-api.dflow.net".into(),
        authentication_mode: "none".into(),
        captured_at_utc: "2026-07-31T22:13:40Z".into(),
        pair: "USDC/SOL".into(),
        input_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
        output_mint: "So11111111111111111111111111111111111111112".into(),
        raw_input_amount: "100000000".into(),
        slippage_configuration: "50_bps".into(),
        raw_artifact_path: reference_response().display().to_string(),
        raw_artifact_sha256: sha256_file(&reference_response()).unwrap(),
        sanitized_artifact_path: String::new(),
        sanitization_status: SanitizationStatus::NotRequired,
        transaction_presence: TransactionPresence::PresentBase64,
        signature: None,
        source_notes: "canonical pipeline equivalence test".into(),
    }
}

/// Strip the identity-dependent parts so two ingestion sources can be compared
/// on the evidence they recovered, not on what they were called.
///
/// `capture` is manifest-supplied metadata by design. Artifact ids are echoed
/// into every claim and unresolved entry, so they are normalized rather than
/// dropped — a claim losing its provenance field entirely would be hidden by
/// this comparison, and that is exactly the kind of regression it must catch.
fn normalize_identity(bundle: &LineageBundle) -> Value {
    let mut v: Value = serde_json::to_value(bundle).unwrap();
    let real_id = bundle.capture.artifact_id.clone();
    v.as_object_mut().unwrap().remove("capture");
    rewrite_ids(&mut v, &real_id);
    v
}

fn rewrite_ids(v: &mut Value, artifact_id: &str) {
    match v {
        Value::String(s) if s == artifact_id => *s = "<artifact_id>".into(),
        Value::Array(items) => items.iter_mut().for_each(|i| rewrite_ids(i, artifact_id)),
        Value::Object(map) => map.values_mut().for_each(|i| rewrite_ids(i, artifact_id)),
        _ => {}
    }
}

async fn via_extract() -> extract::Extraction {
    extract::extract(ExtractInputs {
        response_path: Some(&reference_response()),
        ..Default::default()
    })
    .await
    .unwrap()
}

async fn via_trace(manifest: &ArtifactManifest) -> extract::Extraction {
    trace::build_trace_full(TraceInputs {
        manifest: Some(manifest),
        provider_json_path: Some(&reference_response()),
        transaction_b64_path: None,
        signature: None,
        // Never contacted: the reference response needs no RPC, and
        // resolve_alts is off.
        rpc_url: "http://127.0.0.1:9",
        resolve_alts: false,
        enrich_settlement: false,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn extract_and_trace_produce_the_same_canonical_bundle() {
    if !have_reference_case() {
        eprintln!("skip: reference case artifacts not present");
        return;
    }
    let manifest = reference_manifest();
    let from_extract = via_extract().await;
    let from_trace = via_trace(&manifest).await;

    assert_eq!(
        normalize_identity(&from_extract.lineage),
        normalize_identity(&from_trace.lineage),
        "extract and trace disagree — a second lineage builder has appeared"
    );
}

#[tokio::test]
async fn both_paths_carry_links_and_provenance() {
    if !have_reference_case() {
        return;
    }
    let manifest = reference_manifest();
    let from_extract = via_extract().await;
    let from_trace = via_trace(&manifest).await;

    for (label, run) in [("extract", &from_extract), ("trace", &from_trace)] {
        assert!(
            !run.lineage.links.is_empty(),
            "{label} produced no cross-stage links"
        );
        assert!(
            run.lineage
                .links
                .iter()
                .any(|l| l.relationship == "candidate_byte_match"),
            "{label} lost the candidate byte relationship"
        );
        assert!(
            run.lineage
                .links
                .iter()
                .all(|l| !l.claim_ceiling.is_empty()),
            "{label} emitted a link with no claim ceiling"
        );

        let stages = &run.context.provenance.stages;
        assert!(stages.iter().any(|p| p.stage == Stage::ProviderResponse));
        assert!(stages
            .iter()
            .any(|p| p.stage == Stage::TransactionConstruction));
        assert!(
            stages
                .iter()
                .all(|p| p.stage != Stage::ProviderResponse || p.sha256.is_some()),
            "{label} lost the response content hash"
        );
    }
}

#[tokio::test]
async fn manifest_only_supplies_identity_not_evidence() {
    if !have_reference_case() {
        return;
    }
    let manifest = reference_manifest();
    let from_trace = via_trace(&manifest).await;

    // Identity comes from the manifest...
    assert_eq!(
        from_trace.lineage.capture.artifact_id,
        "art_reference_b00_A1_50"
    );
    assert_eq!(from_trace.lineage.capture.pair, "USDC/SOL");
    assert_eq!(from_trace.lineage.capture.surface, "order");
    // ...while the evidence comes from the adapter, identically to extract.
    let from_extract = via_extract().await;
    assert_eq!(
        from_trace.lineage.quote.out_amount,
        from_extract.lineage.quote.out_amount
    );
    assert_eq!(
        from_trace
            .context
            .transaction
            .as_ref()
            .map(|t| &t.transaction_sha256),
        from_extract
            .context
            .transaction
            .as_ref()
            .map(|t| &t.transaction_sha256)
    );
}

/// Production code of a module: comments and the `#[cfg(test)]` block removed.
///
/// Test fixtures legitimately contain provider JSON — a guard that flagged
/// them would only teach people to write fixtures elsewhere.
fn production_code(relative_path: &str) -> String {
    let source = std::fs::read_to_string(root().join(relative_path)).unwrap();
    let source = match source.find("#[cfg(test)]") {
        Some(i) => source[..i].to_string(),
        None => source,
    };
    source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Architectural guard: trace is ingestion glue plus rendering. If provider
/// field names or legacy normalization reappear in it, the split is back.
#[test]
fn trace_module_owns_no_provider_normalization() {
    let code = production_code("src/trace.rs");

    for forbidden in [
        "providers::",
        "normalize_provider_json",
        "outAmount",
        "otherAmountThreshold",
        "routePlan",
        "decode_base64_transaction",
        "LineageBundle::new",
    ] {
        assert!(
            !code.contains(forbidden),
            "src/trace.rs references `{forbidden}`; lineage construction belongs to \
             adapters/ + lineage_builder"
        );
    }
}

/// The same guard for the verifier core: no provider-native field name may
/// appear outside `adapters/`.
#[test]
fn provider_field_names_stay_inside_adapters() {
    let core = [
        "src/lineage_builder.rs",
        "src/execution_context.rs",
        "src/extract.rs",
        "src/solana/mod.rs",
        "src/checks/generic.rs",
        "src/checks/solana.rs",
        "src/checks/settlement.rs",
    ];
    for file in core {
        let code = production_code(file);
        for forbidden in [
            "routePlan",
            "otherAmountThreshold",
            "minOutAmount",
            "platformFee",
        ] {
            assert!(
                !code.contains(forbidden),
                "{file} reads the provider-native field `{forbidden}`"
            );
        }
    }
}
