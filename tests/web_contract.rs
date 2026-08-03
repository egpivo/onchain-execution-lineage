//! Data-contract and privacy guards for the web viewer.
//!
//! The viewer is a rendering layer over Rust output. These tests hold that
//! boundary from the Rust side: bundled artifacts must deserialize into the
//! real types, must carry no captured data, and the JavaScript must not
//! re-implement anything the verifier decides.
//!
//! No browser automation: a headless-browser dependency would cost more than it
//! buys for a first milestone. What is checked here is the data contract and
//! the static source, which is where the boundary can actually rot.

use std::path::PathBuf;

use onchain_execution_lineage::checks::{CheckStatus, VerificationReport};
use onchain_execution_lineage::evidence_extract::{EvidenceExtract, PUBLIC_EXTRACT_PATH};
use onchain_execution_lineage::execution_context::ExecutionContext;
use onchain_execution_lineage::lineage_model::LineageBundle;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn web() -> PathBuf {
    root().join("web")
}

fn read(relative: &str) -> String {
    let path = web().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e} — run scripts/build_web.sh", path.display()))
}

const SAMPLE_DIRS: [&str; 2] = ["samples/dflow-order", "samples/dflow-order-mismatch"];

/// Every JavaScript file the site ships, including nested view modules.
fn js_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![web()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|x| x == "js").unwrap_or(false) {
                let name = path.strip_prefix(web()).unwrap().display().to_string();
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out.sort();
    assert!(
        out.len() >= 10,
        "expected the full view tree, found {}",
        out.len()
    );
    out
}

// ---- data contract ------------------------------------------------------

#[test]
fn bundled_artifacts_deserialize_into_the_rust_types() {
    for dir in SAMPLE_DIRS {
        let context: ExecutionContext = serde_json::from_str(&read(&format!("{dir}/context.json")))
            .unwrap_or_else(|e| panic!("{dir}/context.json is not an ExecutionContext: {e}"));
        let lineage: LineageBundle = serde_json::from_str(&read(&format!("{dir}/lineage.json")))
            .unwrap_or_else(|e| panic!("{dir}/lineage.json is not a LineageBundle: {e}"));
        let report: VerificationReport =
            serde_json::from_str(&read(&format!("{dir}/verification.json")))
                .unwrap_or_else(|e| panic!("{dir}/verification.json is not a report: {e}"));

        lineage.validate_schema().unwrap();
        assert_eq!(context.schema_version, "1.0.0");
        assert_eq!(report.schema_version, "1.0.0");
        assert!(!report.results.is_empty());
        assert!(!lineage.links.is_empty(), "{dir} has no cross-stage links");
    }
}

#[test]
fn bundled_evidence_extract_is_the_tracked_artifact_verbatim() {
    let bundled = read("data/route_stable_evidence_extract.json");
    let tracked = std::fs::read_to_string(root().join(PUBLIC_EXTRACT_PATH))
        .expect("tracked publication extract");
    assert_eq!(
        bundled, tracked,
        "web/samples copy has drifted from the tracked evidence extract; run scripts/build_web.sh"
    );

    // And it is still the real thing, not a hand-edited lookalike.
    let extract: EvidenceExtract = serde_json::from_str(&bundled).unwrap();
    assert_eq!(
        extract.experiment_id,
        "dflow_order_slippage_route_stable_live"
    );
    assert_eq!(extract.total_requests, 30);
}

#[test]
fn every_verification_status_is_representable_in_the_bundle() {
    let mut seen = std::collections::BTreeSet::new();
    for dir in SAMPLE_DIRS {
        let report: VerificationReport =
            serde_json::from_str(&read(&format!("{dir}/verification.json"))).unwrap();
        for r in &report.results {
            seen.insert(r.status.as_str());
        }
    }
    for status in ["PASS", "FAIL", "CANDIDATE", "UNKNOWN", "NOT_APPLICABLE"] {
        assert!(
            seen.contains(status),
            "no bundled sample produces {status}; the viewer cannot demonstrate it"
        );
    }
}

#[test]
fn the_healthy_sample_has_no_failures_and_the_mismatch_sample_does() {
    let healthy: VerificationReport =
        serde_json::from_str(&read("samples/dflow-order/verification.json")).unwrap();
    assert!(!healthy.has_failures());
    assert!(healthy
        .results
        .iter()
        .any(|r| r.status == CheckStatus::Candidate));

    let mismatch: VerificationReport =
        serde_json::from_str(&read("samples/dflow-order-mismatch/verification.json")).unwrap();
    assert!(
        mismatch.has_failures(),
        "the failure sample must actually fail, or the viewer never shows FAIL"
    );
}

// ---- privacy ------------------------------------------------------------

/// Fee payer of the private recorded capture. If this string ever appears in a
/// bundled file, capture data has leaked into the published site.
const PRIVATE_FEE_PAYER: &str = "Hmx7mUZ2tHQewMWhEkyvWLuBDCoJy2nHtagd9XhZn7hL";

fn bundled_files() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![web()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(web()).unwrap().display().to_string();
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((name, text));
                }
            }
        }
    }
    out
}

#[test]
fn no_private_capture_data_is_bundled() {
    for (name, text) in bundled_files() {
        assert!(
            !text.contains(PRIVATE_FEE_PAYER),
            "web/{name} contains the private capture's fee-payer pubkey"
        );
        for marker in [
            "user_public_key",
            "userPublicKey",
            "request_timestamp_utc",
            "wallet_balance_at_capture_time",
            "authorization",
            "Bearer ",
        ] {
            assert!(
                !text.contains(marker),
                "web/{name} contains private capture marker `{marker}`"
            );
        }
    }
}

#[test]
fn no_transaction_payload_is_bundled() {
    for (name, text) in bundled_files() {
        if !name.ends_with(".json") {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_no_blob(&value, &name);
    }
}

/// Recursively reject long base64-looking strings. A serialized transaction is
/// hundreds of characters of base64; nothing the viewer needs is.
fn assert_no_blob(value: &serde_json::Value, file: &str) {
    match value {
        serde_json::Value::String(s) => {
            let looks_base64 = s.len() > 180
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=');
            assert!(
                !looks_base64,
                "web/{file} contains a {}-char base64 blob; transaction payloads are not published",
                s.len()
            );
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| assert_no_blob(v, file)),
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                assert!(
                    key != "transaction_b64"
                        || v.as_str().map(|s| s.starts_with('<')).unwrap_or(false),
                    "web/{file} carries an unredacted transaction_b64"
                );
                assert_no_blob(v, file);
            }
        }
        _ => {}
    }
}

// ---- boundary: the frontend must not re-implement the verifier ----------

#[test]
fn javascript_does_not_reimplement_empirical_logic() {
    // Comments explain the rules; code must not apply them.
    let forbidden = [
        (
            "atob(",
            "no base64 decoding — transactions are decoded in Rust",
        ),
        ("Buffer.from", "no byte handling in the browser"),
        ("DataView", "no binary parsing in the browser"),
        ("Uint8Array", "no binary parsing in the browser"),
        ("10000", "no threshold arithmetic — the identity is Rust's"),
        ("Math.ceil", "no rounding rules in the browser"),
        ("Math.floor", "no rounding rules in the browser"),
        (
            "BigInt",
            "amounts are displayed as strings, never recomputed",
        ),
        (
            "parseInt(",
            "amounts are displayed as strings, never recomputed",
        ),
        ("parseFloat", "base-unit amounts must never become floats"),
        ("indexOf(needle", "no byte search in the browser"),
    ];

    for (name, source) in js_sources() {
        let code: String = source
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("*") && !t.starts_with("/*")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for (token, why) in forbidden {
            assert!(!code.contains(token), "web/{name} uses `{token}`: {why}");
        }
    }
}

#[test]
fn javascript_makes_no_third_party_or_upload_requests() {
    for (name, source) in js_sources() {
        for token in [
            "http://",
            "XMLHttpRequest",
            "WebSocket",
            "navigator.sendBeacon",
            "localStorage",
            "sessionStorage",
            "method: \"POST\"",
            "FormData",
        ] {
            assert!(
                !source.contains(token),
                "web/{name} uses `{token}`; the viewer must stay local and read-only"
            );
        }
        // The only https:// allowed is the repository link.
        for line in source.lines() {
            if let Some(idx) = line.find("https://") {
                let url = &line[idx..];
                assert!(
                    url.starts_with("https://github.com/egpivo/onchain-execution-lineage"),
                    "web/{name} references an external URL: {url}"
                );
            }
        }
    }
    let html = read("index.html");
    for token in ["cdn", "unpkg", "jsdelivr", "googleapis", "http://"] {
        assert!(!html.contains(token), "index.html pulls in `{token}`");
    }
}

#[test]
fn status_rendering_keeps_candidate_and_unknown_distinct() {
    let format = read("components/format.js");
    // Each status has its own glyph and its own literal label.
    for status in ["PASS", "FAIL", "CANDIDATE", "UNKNOWN", "NOT_APPLICABLE"] {
        assert!(
            format.contains(status),
            "format.js has no vocabulary for {status}"
        );
    }
    for glyph in ["✓", "✕", "◈", "?", "–"] {
        assert!(
            format.contains(glyph),
            "format.js is missing status glyph {glyph}"
        );
    }

    // Colour is never the only signal: every status class also sets a border
    // style, and no two share one.
    let css = read("styles.css");
    for (class, border) in [
        (".status-pass", "solid"),
        (".status-fail", "double"),
        (".status-candidate", "dashed"),
        (".status-unknown", "dotted"),
    ] {
        let block = css
            .split(class)
            .nth(1)
            .unwrap_or_else(|| panic!("styles.css has no {class} rule"));
        let block = block.split('}').next().unwrap();
        assert!(
            block.contains(border),
            "{class} must be distinguishable without colour (expected `{border}` border)"
        );
    }

    // CANDIDATE must not be described with PASS wording anywhere.
    let checks = read("components/checks.js");
    let candidate_meaning = checks
        .split("CANDIDATE:")
        .nth(1)
        .expect("checks.js explains CANDIDATE")
        .split("UNKNOWN:")
        .next()
        .unwrap();
    assert!(
        candidate_meaning.contains("not a weak pass") || candidate_meaning.contains("coincidence"),
        "checks.js must state what a candidate is not"
    );
    let unknown_meaning = checks
        .split("UNKNOWN:")
        .nth(1)
        .expect("checks.js explains UNKNOWN")
        .split("NOT_APPLICABLE:")
        .next()
        .unwrap();
    assert!(
        unknown_meaning.contains("not a failure"),
        "checks.js must state that UNKNOWN is not a failure"
    );
}

/// The site's use-case views must read published values, not carry them.
#[test]
fn published_values_are_never_hard_coded_in_javascript() {
    let case_code: String = js_sources()
        .into_iter()
        .filter(|(name, _)| name.starts_with("views/") || name.starts_with("components/"))
        .map(|(_, source)| production_js(&source))
        .collect::<Vec<_>>()
        .join("\n");

    // Unambiguous published tokens only: a bare "15" would match a CSS-ish
    // number and teach nothing.
    for literal in [
        "1,2,5,8,9",
        "u64_le",
        "ix2:99",
        "30/30",
        "15/15",
        "8-byte little-endian",
        "78e349e7",
        "dea0ce94",
        "Tessera V",
        "BisonFi",
    ] {
        assert!(
            !case_code.contains(literal),
            "a view hard-codes `{literal}`; it must read the value from tracked evidence"
        );
    }

    // Field names may be read in JS or declared as metric paths in the
    // use-case index; both are data-driven, and both count.
    let declared = read("data/use-cases.json");
    let all_sources = format!("{case_code}\n{declared}");
    for field in [
        "total_requests",
        "total_batches",
        "eligible_batch_count",
        "threshold_identity",
        "candidate_result",
        "per_request_search",
        "anchor_control",
        "route_class_a1_t",
        "ineligibility_reasons",
        "fingerprint_short",
    ] {
        assert!(all_sources.contains(field), "nothing reads `{field}`");
    }
}

/// Comments explain the rules; only code is checked against them.
fn production_js(source: &str) -> String {
    source
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("*") && !t.starts_with("/*")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The use-case collection must be data-driven: adding a case is a config entry
/// plus a view module, never a navigation rewrite.
#[test]
fn use_case_index_is_data_driven() {
    let index: serde_json::Value =
        serde_json::from_str(&read("data/use-cases.json")).expect("web/data/use-cases.json parses");

    let cases = index["cases"].as_array().expect("cases array");
    assert!(
        cases.len() >= 2,
        "the template should carry more than one case"
    );

    for case in cases {
        for field in [
            "id",
            "title",
            "types",
            "providers",
            "chain",
            "question",
            "views",
        ] {
            assert!(
                !case[field].is_null(),
                "use case {:?} is missing `{field}`",
                case["id"]
            );
        }
        // Every declared metric must point at a declared artifact.
        for metric in case["metrics"].as_array().unwrap_or(&vec![]) {
            let artifact = metric["artifact"].as_str().expect("metric artifact");
            assert!(
                !index["artifacts"][artifact].is_null(),
                "metric references undeclared artifact `{artifact}`"
            );
            assert!(metric["path"].is_string(), "metric has no path");
        }
    }

    // Navigation is built from this file, not from a switch in the router.
    let app = read("app.js");
    assert!(
        app.contains("data.useCases"),
        "the router must read the use-case index rather than hard-coding cases"
    );

    // Every artifact the index declares must be bundled.
    for (_, path) in index["artifacts"].as_object().expect("artifacts map") {
        let relative = path.as_str().unwrap().trim_start_matches("./");
        assert!(
            web().join(relative).exists(),
            "declared artifact {relative} is not bundled — run scripts/build_web.sh"
        );
    }
}

/// Bundled analysis artifacts must be verbatim copies of the tracked sources.
#[test]
fn bundled_data_matches_the_tracked_artifacts() {
    for name in [
        "route_stable_evidence_extract.json",
        "route_stable_batch_evidence.json",
        "route_stable_causal_model.json",
        "fee_quote_evidence.json",
    ] {
        let bundled = read(&format!("data/{name}"));
        let tracked = std::fs::read_to_string(root().join("artifacts/analysis").join(name))
            .unwrap_or_else(|e| panic!("read tracked {name}: {e}"));
        assert_eq!(
            bundled, tracked,
            "web/data/{name} has drifted from artifacts/analysis/{name}; run scripts/build_web.sh"
        );
    }
}

/// The DAG the identification view renders must be internally consistent, and
/// it must stay number-free: it is an assumption record, not a result.
#[test]
fn causal_model_is_well_formed_and_number_free() {
    let model: serde_json::Value =
        serde_json::from_str(&read("data/route_stable_causal_model.json"))
            .expect("causal model parses");

    let nodes: std::collections::BTreeSet<&str> = model["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|n| n["id"].as_str().expect("node id"))
        .collect();
    assert!(
        nodes.contains("S") && nodes.contains("R") && nodes.contains("U") && nodes.contains("B")
    );

    for edge in model["edges"].as_array().expect("edges") {
        for end in ["from", "to"] {
            let id = edge[end].as_str().expect("edge endpoint");
            assert!(
                nodes.contains(id),
                "edge {:?} references undeclared node `{id}`",
                edge["id"]
            );
        }
        assert!(
            !edge["evidence_class"].is_null(),
            "edge {:?} has no evidence class",
            edge["id"]
        );
    }

    // Positions are authored, so the view never has to invent a layout.
    for node in model["nodes"].as_array().unwrap() {
        assert!(
            node["x"].is_number() && node["y"].is_number(),
            "node lacks a fixed position"
        );
    }

    // Modes drive the interactive states.
    let modes = model["modes"].as_array().expect("modes");
    assert!(modes.len() >= 3, "expected the interactive mode set");

    // No probabilities, no effect sizes.
    for edge in model["edges"].as_array().unwrap() {
        for key in [
            "probability",
            "coefficient",
            "effect",
            "p_value",
            "estimate",
        ] {
            assert!(
                edge[key].is_null(),
                "causal model carries a fitted quantity `{key}`"
            );
        }
    }
}

#[test]
fn deep_link_routes_are_wired() {
    let app = read("app.js");
    // Canonical routes.
    for route in ["explore", "docs", "inspect"] {
        assert!(app.contains(route), "app.js has no `{route}` route");
    }
    // Use-case sub-views are declared in the index and consumed by the shell,
    // so adding a case needs no router change.
    let index: serde_json::Value = serde_json::from_str(&read("data/use-cases.json")).unwrap();
    let slippage = index["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "dflow-slippage")
        .expect("the reference case is configured");
    let views: Vec<&str> = slippage["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    for sub in ["threshold", "route", "identification", "bytes", "reproduce"] {
        assert!(
            views.contains(&sub),
            "#/explore/dflow-slippage/{sub} is not configured"
        );
    }

    let shell = read("views/dflow-slippage/index.js");
    assert!(
        shell.contains("#/explore/"),
        "the use-case shell does not build sub-view links under the canonical #/explore/ route"
    );
    for view in ["route", "bytes"] {
        let source = read(&format!("views/dflow-slippage/{view}.js"));
        assert!(
            source.contains("batch="),
            "the {view} view must accept ?batch="
        );
    }
    assert!(
        app.contains("URLSearchParams"),
        "the router must parse query parameters for deep links"
    );
}

/// The previous navigation's hash paths must keep resolving: an external link
/// or bookmark written against #/overview, #/use-cases/…, #/architecture,
/// #/reference or #/load should not 404 into the home fallback silently — it
/// should land on the page it always pointed at.
#[test]
fn legacy_hash_paths_still_resolve() {
    let app = read("app.js");

    // Every old top-level segment must be named in the router, so it can be
    // matched and redirected rather than falling through to home.
    for legacy in [
        "\"overview\"",
        "\"use-cases\"",
        "\"architecture\"",
        "\"reference\"",
        "\"load\"",
    ] {
        assert!(
            app.contains(legacy),
            "app.js no longer recognises the legacy route token {legacy}; old links to it would silently fall back to home"
        );
    }

    // Each legacy token must be routed to a *current* destination, not just
    // present as a stray string (e.g. in a comment).
    let code = production_js(&app);
    assert!(
        code.contains("head === \"overview\"") || code.contains("head===\"overview\""),
        "app.js does not branch on the legacy `overview` path"
    );
    assert!(
        code.contains("head === \"use-cases\""),
        "app.js does not branch on the legacy `use-cases` path"
    );
    assert!(
        code.contains("head === \"architecture\""),
        "app.js does not branch on the legacy `architecture` path"
    );
    assert!(
        code.contains("head === \"reference\""),
        "app.js does not branch on the legacy `reference` path"
    );
    assert!(
        code.contains("head === \"load\""),
        "app.js does not branch on the legacy `load` path"
    );

    // Old panel names (lineage/checks/links/load) must still map onto the new
    // inspector tabs rather than being dropped.
    assert!(
        app.contains("LEGACY_INSPECT_PANEL"),
        "app.js has no mapping table for the old #/load?panel=… values"
    );
}

/// Primary navigation is exactly Home / Explore / Docs / GitHub. Architecture,
/// Reference and lineage-loading are reachable, but not as top-level items.
#[test]
fn primary_navigation_is_reduced_to_four_items() {
    let app = read("app.js");
    let nav_block = app
        .split("const NAV = [")
        .nth(1)
        .expect("app.js declares a NAV array")
        .split("];")
        .next()
        .unwrap();
    for label in ["\"Home\"", "\"Explore\"", "\"Docs\""] {
        assert!(nav_block.contains(label), "primary nav is missing {label}");
    }
    for absent in ["\"Architecture\"", "\"Reference\"", "\"Load Lineage\""] {
        assert!(
            !nav_block.contains(absent),
            "primary nav still lists {absent}; it must be demoted under Docs or into a CTA"
        );
    }

    // "Inspect lineage JSON" is a task CTA, not a nav category.
    let home = read("views/home.js");
    assert!(
        home.contains("Inspect lineage JSON"),
        "home.js has no Inspect-lineage CTA"
    );
    assert!(
        home.contains("Explore a real case"),
        "home.js has no primary explore CTA"
    );
}

#[test]
fn loader_supports_and_rejects_schema_versions_explicitly() {
    let loader = read("components/loader.js");
    assert!(
        loader.contains("SUPPORTED"),
        "loader must declare supported versions"
    );
    assert!(
        loader.contains("does not support"),
        "loader must reject unsupported schema versions with a readable message"
    );
    assert!(
        loader.contains("never leaves your browser") || loader.contains("browser-local"),
        "loader must state that files stay local"
    );
    // Schema versions the viewer claims to support must match the Rust ones.
    assert!(loader.contains("\"1.0.0\""));
}

#[test]
fn the_page_declares_the_product_boundary() {
    let html = read("index.html");
    assert!(
        html.contains("Rust decides"),
        "index.html must state the boundary"
    );
    for phrase in ["No signing", "no submission"] {
        assert!(
            html.to_lowercase().contains(&phrase.to_lowercase()),
            "index.html must carry the `{phrase}` limit"
        );
    }
}

/// Identification math is plain text / SVG. A third-party math renderer would
/// be a new dependency for two conditional expressions — refuse it.
#[test]
fn identification_view_does_not_depend_on_math_renderers() {
    let html = read("index.html").to_lowercase();
    // Script/link tags are the real dependency surface.
    for banned in [
        "mathjax",
        "katex",
        "math.js",
        "cdn.jsdelivr.net/npm/katex",
        "polyfill.io/v3/polyfill.min.js?features=es6",
    ] {
        assert!(
            !html.contains(banned),
            "index.html must not load `{banned}` for identification notation"
        );
    }
    let ident = read("views/dflow-slippage/identification.js");
    assert!(
        ident.contains("notation_steps"),
        "identification.js must read authored notation_steps"
    );
    assert!(
        ident.contains("SELECTION != INTERVENTION"),
        "identification.js must surface the selection-vs-intervention headline"
    );
    assert!(
        !ident.contains("/Users/") && !ident.contains("file://") && !ident.contains("localhost"),
        "identification.js must not embed local filesystem or localhost URLs"
    );
}

#[test]
fn bundled_causal_model_keeps_selection_distinct_from_intervention() {
    let model: serde_json::Value =
        serde_json::from_str(&read("data/route_stable_causal_model.json")).unwrap();
    let blob = serde_json::to_string(&model).unwrap();
    assert!(blob.contains("P(B | do(S), R = r)"));
    assert!(blob.contains("P(B | do(S), do(R = r))"));
    assert!(blob.contains("R = r != do(R = r)"));
    assert!(!blob.contains("R = r == do(R = r)"));
    assert_eq!(model["collider"]["structure"].as_str(), Some("S -> R <- U"));
}

/// Route and view ids arrive from the URL and are used as object keys. A plain
/// truthy lookup lets an inherited key such as `constructor` resolve to a
/// function on Object.prototype, which is then called as a renderer: the page
/// silently blanks instead of reporting an unknown id. Both dispatch tables,
/// and the declared-metric-path reader, must consult own properties only.
#[test]
fn url_controlled_lookups_reject_inherited_keys() {
    for (file, table) in [
        ("app.js", "CASE_VIEWS"),
        ("views/dflow-slippage/index.js", "VIEWS"),
    ] {
        let source = read(file);
        assert!(
            source.contains(&format!("Object.hasOwn({table},")),
            "{file} must guard the {table} lookup with Object.hasOwn"
        );
    }
    assert!(
        read("app.js").contains("Object.hasOwn(acc, key)"),
        "readPath must resolve own properties only"
    );
}
