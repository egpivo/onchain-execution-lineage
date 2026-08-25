//! Failure modes of the reference-case reproducibility entry point.
//!
//! The happy paths are covered by unit tests in `reference_case`; what matters
//! here is that the CLI fails *loudly and informatively* when the inputs are
//! missing, corrupt, or disagree — a reproducibility command that exits 0 on a
//! broken tree is worse than none.

use std::path::PathBuf;
use std::process::{Command, Output};

use onchain_execution_lineage::evidence_extract::PUBLIC_EXTRACT_PATH;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tracked_extract() -> PathBuf {
    root().join(PUBLIC_EXTRACT_PATH)
}

fn recorded_run_present() -> bool {
    root()
        .join("artifacts/experiments/dflow_order_slippage_route_stable_live/experiment_report.json")
        .exists()
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("article_repro_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_onchain-execution-lineage"))
        .arg("reference-case")
        .args(args)
        .current_dir(root())
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

// ---- public mode --------------------------------------------------------

#[test]
fn public_mode_succeeds_on_a_clean_tree() {
    if !tracked_extract().exists() {
        eprintln!("skip: tracked extract not present");
        return;
    }
    let out = run(&[]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let text = stdout(&out);
    assert!(text.contains("PUBLIC ARTICLE VERIFICATION"));
    assert!(!text.contains("FAIL"), "a claim failed:\n{text}");
    // The honesty statement is not optional.
    assert!(text.contains("does not rebuild the"));
    assert!(text.contains("raw captures are not published"));

    // Every audited article claim appears in the table.
    for claim in [
        "requests",
        "brackets",
        "eligible brackets",
        "eligibility rate",
        "threshold ceil identity",
        "floor identity",
        "minOut == threshold",
        "eligible tx searched",
        "threshold literal matches",
        "difference literal matches",
        "quote literal matches",
        "unique quote matches",
        "quote candidate site",
        "canonical encoding",
        "same-treatment controls",
        "settlement",
    ] {
        assert!(
            text.contains(claim),
            "claim `{claim}` missing from the table"
        );
    }
}

#[test]
fn public_mode_publishes_the_expected_values() {
    if !tracked_extract().exists() {
        return;
    }
    let text = stdout(&run(&[]));
    for (claim, value) in [
        ("requests", "30"),
        ("brackets", "10"),
        ("eligible brackets", "1,2,5,8,9"),
        ("threshold ceil identity", "30/30"),
        ("floor identity", "0/30"),
        ("eligible tx searched", "15"),
        ("quote literal matches", "15/15"),
        ("quote candidate site", "ix2:99"),
        ("same-treatment controls", "5/5"),
    ] {
        let line = text
            .lines()
            .find(|l| l.starts_with(claim))
            .unwrap_or_else(|| panic!("no row for {claim}"));
        assert!(
            line.contains(value),
            "row for {claim} does not show {value}: {line}"
        );
    }
}

#[test]
fn missing_extract_fails_clearly() {
    let out = run(&[
        "--extract",
        "/nonexistent/route_stable_evidence_extract.json",
    ]);
    assert!(!out.status.success());
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("not found"), "unhelpful failure: {text}");
}

#[test]
fn corrupt_extract_fails() {
    let dir = tmp_dir("corrupt");
    let path = dir.join("extract.json");
    std::fs::write(&path, "{ this is not json").unwrap();

    let out = run(&["--extract", path.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(format!("{}{}", stdout(&out), stderr(&out)).contains("parse"));
}

#[test]
fn extract_with_an_unsupported_schema_fails() {
    let dir = tmp_dir("schema");
    let path = dir.join("extract.json");
    std::fs::write(&path, r#"{"schema_version":"9.9.9"}"#).unwrap();

    let out = run(&["--extract", path.to_str().unwrap()]);
    assert!(!out.status.success());
}

#[test]
fn a_tampered_summary_fails_loudly() {
    if !tracked_extract().exists() {
        return;
    }
    let dir = tmp_dir("tampered");
    let path = dir.join("extract.json");

    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tracked_extract()).unwrap()).unwrap();
    // Claim a threshold byte match the per-request detail does not support.
    value["candidate_result"]["threshold_sites_total"] = serde_json::json!(1);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let out = run(&["--extract", path.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "a mismatched summary must exit non-zero"
    );
    let text = stdout(&out);
    assert!(text.contains("FAIL"));
    assert!(text.contains("threshold literal matches"));
}

// ---- local rebuild ------------------------------------------------------

#[test]
fn local_rebuild_without_the_recorded_run_explains_why() {
    let dir = tmp_dir("norun");
    let out = Command::new(env!("CARGO_BIN_EXE_onchain-execution-lineage"))
        .args(["reference-case", "--from-recorded-run", "--base-dir"])
        .arg(&dir)
        .current_dir(root())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(text.contains("recorded run not found"));
    assert!(text.contains("not published"), "must say why: {text}");
    assert!(
        text.contains("public verification"),
        "must point at the mode that does work: {text}"
    );
}

#[test]
fn local_rebuild_matches_the_published_extract() {
    if !recorded_run_present() || !tracked_extract().exists() {
        eprintln!("skip: recorded run is private and not present");
        return;
    }
    let out = run(&["--from-recorded-run"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let text = stdout(&out);
    assert!(text.contains("LOCAL FULL REBUILD"));
    assert!(
        text.contains("matches the tracked publication extract exactly"),
        "rebuild diverged:\n{text}"
    );
    // The rebuild must run the production pipeline, not article-only code.
    assert!(text.contains("dflow.slippage_threshold_arithmetic"));
    assert!(text.contains("NOT_APPLICABLE settlement.landed_status"));
}

#[test]
fn local_rebuild_reports_the_exact_differing_field() {
    if !recorded_run_present() {
        return;
    }
    let dir = tmp_dir("divergent");
    let path = dir.join("extract.json");

    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tracked_extract()).unwrap()).unwrap();
    value["total_requests"] = serde_json::json!(29);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let out = run(&["--from-recorded-run", "--extract", path.to_str().unwrap()]);
    assert!(!out.status.success(), "divergence must exit non-zero");

    let text = stdout(&out);
    assert!(text.contains("DIVERGENCE"));
    assert!(text.contains("total_requests"));
    assert!(text.contains("regenerated: 30"));
    assert!(text.contains("published  : 29"));
}

// ---- guards -------------------------------------------------------------

/// Neither mode may reach the network. The reference-case module must not
/// touch any RPC or HTTP surface.
#[test]
fn reference_case_makes_no_network_calls() {
    let source = std::fs::read_to_string(root().join("src/reference_case.rs")).unwrap();
    let code: String = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    for forbidden in [
        "reqwest",
        "RpcClient",
        "crate::rpc",
        "crate::api",
        "crate::capture",
        "lookup_tables",
        "RpcContext",
        "run_route_bracket_experiment",
    ] {
        assert!(
            !code.contains(forbidden),
            "src/reference_case.rs references `{forbidden}`; reproduction must be offline"
        );
    }
}

/// The shell script is orchestration. If empirical logic appears in it, the
/// single-source-of-truth property is gone.
#[test]
fn script_contains_no_empirical_logic() {
    let path = root().join("scripts/reproduce_slippage_article.sh");
    let script = std::fs::read_to_string(&path).unwrap();
    let code: String = script
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    for forbidden in [
        "jq", "python", "bc", "awk", "10000", "ceil", "offset", "u64_le", "grep",
    ] {
        assert!(
            !code.contains(forbidden),
            "scripts/reproduce_slippage_article.sh contains `{forbidden}`; empirical logic \
             belongs in Rust"
        );
    }
    assert!(code.contains("reference-case"), "script must invoke Rust");
}

#[test]
fn script_is_executable() {
    let path = root().join("scripts/reproduce_slippage_article.sh");
    assert!(path.exists(), "the documented entry point must exist");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "script is not executable (mode {mode:o})"
        );
    }
}

/// Derived lineage output embeds the unsigned transaction and the requester's
/// fee-payer pubkey, so it must never become tracked.
#[test]
fn derived_lineage_output_is_not_publishable() {
    let out = Command::new("git")
        .args(["check-ignore", "-q", "artifacts/lineage/x/context.json"])
        .current_dir(root())
        .status();
    match out {
        Ok(status) => assert!(
            status.success(),
            "artifacts/lineage/ is not gitignored; it contains capture-derived transaction bytes"
        ),
        Err(_) => eprintln!("skip: git unavailable"),
    }
}
