//! Structural validation for the published causal model.
//!
//! `artifacts/analysis/route_stable_causal_model.json` is authored, not derived:
//! no library code produces it, and the identification model is deliberately not
//! a runtime feature. But the file ships from this repository and is rendered
//! directly to readers by the article figures and the evidence lab, so it needs
//! the same guard the manifests get.
//!
//! Two failure modes are covered:
//!
//! 1. Referential drift — an edge pointing at a node that no longer exists, an
//!    evidence class with no reader-facing mapping, a claim with no ceiling.
//! 2. Empirical drift — the reason this test exists. The model previously
//!    carried hand-written counts ("5 of 10 brackets", "u32 and u64
//!    little-endian"). When the byte search was widened to all fifteen
//!    eligible-bracket transactions and the encoding width was collapsed to one
//!    canonical form, those strings silently became wrong and were still being
//!    shown in the lab. Counts belong to the generated evidence extracts; this
//!    test keeps them out of the authored file.

use std::path::PathBuf;

use serde_json::Value;

fn model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("artifacts/analysis/route_stable_causal_model.json")
}

fn load() -> Option<Value> {
    let path = model_path();
    if !path.exists() {
        // The experiment tree is regenerable and the extract may be absent on a
        // clean checkout; skip rather than fail the whole suite.
        eprintln!("skipping: {} not present", path.display());
        return None;
    }
    Some(
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read causal model"))
            .expect("causal model is valid JSON"),
    )
}

/// Prose fields that would carry an empirical count if someone re-added one.
fn prose_fields(model: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut push = |label: String, value: Option<&Value>| {
        if let Some(text) = value.and_then(Value::as_str) {
            out.push((label, text.to_string()));
        }
    };
    push("note".into(), model.get("note"));
    for (index, node) in model["nodes"].as_array().unwrap().iter().enumerate() {
        push(format!("nodes[{index}].evidence"), node.get("evidence"));
        push(format!("nodes[{index}].ceiling"), node.get("ceiling"));
        push(format!("nodes[{index}].definition"), node.get("definition"));
    }
    for (index, edge) in model["edges"].as_array().unwrap().iter().enumerate() {
        push(format!("edges[{index}].note"), edge.get("note"));
        push(format!("edges[{index}].ceiling"), edge.get("ceiling"));
    }
    out
}

/// "5 of 10", "30/30" — the shape a hand-copied count takes.
///
/// Tokenises on whitespace so the two separators can be checked uniformly:
/// a bare `of` between two numeric tokens, or a single token like `30/30`.
fn contains_count(text: &str) -> bool {
    let numeric = |token: &str| {
        let trimmed = token.trim_matches(|c: char| !c.is_ascii_digit());
        !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit())
    };

    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let Some((left, right)) = token.split_once('/') {
            if numeric(left) && numeric(right) {
                return true;
            }
        }
        if *token == "of"
            && index > 0
            && numeric(tokens[index - 1])
            && tokens.get(index + 1).is_some_and(|t| numeric(t))
        {
            return true;
        }
    }
    false
}

#[test]
fn model_carries_no_hardcoded_empirical_counts() {
    let Some(model) = load() else { return };
    let mut offenders = Vec::new();
    for (label, text) in prose_fields(&model) {
        if contains_count(&text) {
            offenders.push(format!("{label}: {text}"));
        }
        for banned in ["u32_le", "u64_le", "u32 and u64", "offset 99"] {
            if text.contains(banned) {
                offenders.push(format!("{label} mentions `{banned}`: {text}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the causal model must not restate empirical results — they belong to the \
         generated evidence extracts, which the lab and figures already load:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn every_edge_connects_declared_nodes() {
    let Some(model) = load() else { return };
    let ids: Vec<&str> = model["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    for edge in model["edges"].as_array().unwrap() {
        for end in ["from", "to"] {
            let node = edge[end].as_str().unwrap();
            assert!(
                ids.contains(&node),
                "edge {} references undeclared node {node}",
                edge["id"]
            );
        }
    }
}

#[test]
fn every_evidence_class_is_defined_and_mapped() {
    let Some(model) = load() else { return };
    let defined = model["evidence_classes"].as_object().unwrap();
    let readers = model["reader_classes"].as_object().unwrap();

    for edge in model["edges"].as_array().unwrap() {
        let class = edge["evidence_class"].as_str().unwrap();
        assert!(
            defined.contains_key(class),
            "edge {} uses undefined evidence class {class}",
            edge["id"]
        );
    }

    // Every internal class must surface under exactly one reader-facing label,
    // otherwise the lab silently falls back to a default.
    for class in defined.keys() {
        let mapped: Vec<&String> = readers
            .iter()
            .filter(|(_, meta)| {
                meta["maps_from"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_str() == Some(class))
            })
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            mapped.len(),
            1,
            "evidence class {class} maps to {mapped:?}, expected exactly one reader class"
        );
    }
}

#[test]
fn every_claim_states_a_ceiling() {
    let Some(model) = load() else { return };
    for node in model["nodes"].as_array().unwrap() {
        let ceiling = node["ceiling"].as_str().unwrap_or_default();
        assert!(
            !ceiling.trim().is_empty(),
            "node {} has no ceiling",
            node["id"]
        );
    }
    for edge in model["edges"].as_array().unwrap() {
        let ceiling = edge["ceiling"].as_str().unwrap_or_default();
        assert!(
            !ceiling.trim().is_empty(),
            "edge {} has no ceiling",
            edge["id"]
        );
    }
}

#[test]
fn rejected_terminology_stays_rejected() {
    let Some(model) = load() else { return };
    let serialized = serde_json::to_string(&model).unwrap().to_lowercase();
    for term in ["controlled direct effect", "cde"] {
        // The term may appear only where the model explicitly rejects it.
        let rejected = model["collider"]["rejected_terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().to_lowercase().contains(term));
        assert!(
            rejected || !serialized.contains(term),
            "{term} appears outside the rejected-terms list"
        );
    }
}

/// Selection on an observed route must stay formally distinct from intervening
/// on route. The published notation steps are the guardrail the identification
/// view renders; they must stay ASCII-safe and must not equate R = r with do(R).
#[test]
fn notation_steps_separate_selection_from_intervention() {
    let Some(model) = load() else { return };
    let collider = &model["collider"];
    let structure = collider["structure"].as_str().unwrap_or_default();
    assert_eq!(
        structure, "S -> R <- U",
        "collider structure must stay ASCII-safe for web/PDF portability"
    );

    let steps = collider["notation_steps"]
        .as_array()
        .expect("notation_steps must be authored on the collider");
    assert!(
        steps.len() >= 4,
        "expected progressive-disclosure steps for the identification view"
    );

    let mut exprs = Vec::new();
    for step in steps {
        for block in step["formulas"].as_array().expect("step formulas") {
            let expr = block["expr"].as_str().expect("formula expr");
            let plain = block["plain"].as_str().unwrap_or_default();
            assert!(
                !expr.contains('→')
                    && !expr.contains('←')
                    && !expr.contains('≠')
                    && !expr.contains('⟺')
                    && !expr.contains('⇏'),
                "formula must stay ASCII-safe: {expr}"
            );
            assert!(
                !plain.is_empty(),
                "every formal expression needs an adjacent plain-English sentence"
            );
            exprs.push(expr.to_string());
        }
    }

    let joined = exprs.join("\n");
    assert!(
        joined.contains("P(B | do(S), R = r)"),
        "missing conditioned contrast"
    );
    assert!(
        joined.contains("P(B | do(S), do(R = r))"),
        "missing intervened-route contrast"
    );
    assert!(
        joined.contains("R = r != do(R = r)"),
        "missing selection-vs-intervention takeaway"
    );
    assert!(
        !joined.contains("R = r = do(R = r)") && !joined.contains("R = r == do(R = r)"),
        "R = r must never be treated as equivalent to do(R = r)"
    );
}
