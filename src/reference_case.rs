//! Reference-case reproducibility: the DFlow slippage article.
//!
//! Publication tooling, deliberately outside the verifier core. It owns every
//! assertion behind `scripts/reproduce_slippage_article.sh`, so the shell layer
//! contains no empirical logic at all.
//!
//! Two modes, and the difference between them is not cosmetic:
//!
//! - **Public verification** works on a clean clone. It reads the tracked
//!   evidence snapshot, re-derives the threshold arithmetic from the published
//!   inputs using the verifier's own implementation, and re-aggregates every
//!   summary claim from the snapshot's per-request detail. It does **not**
//!   rebuild the experiment: the 30 raw provider responses are not published,
//!   because they carry the requester's wallet pubkey.
//! - **Local rebuild** requires the private recorded run. It regenerates the
//!   snapshot through the Rust pipeline and compares it field by field against
//!   the tracked one.
//!
//! Neither mode makes a network request.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::checks::dflow::{ceil_threshold, floor_threshold};
use crate::evidence_extract::{
    self, EvidenceExtract, MatchSite, EVIDENCE_EXTRACT_SCHEMA, PUBLIC_EXTRACT_PATH,
};
use crate::route_bracket::BracketExperimentReport;

/// The recorded run the article is built from. Private; gitignored.
pub const RECORDED_RUN_DIR: &str = "artifacts/experiments/dflow_order_slippage_route_stable_live";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClaimStatus {
    Pass,
    Fail,
}

/// How much a public-mode result is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimBasis {
    /// Rust re-derived the value from published inputs. A wrong number fails.
    Recomputed,
    /// The published summary was re-aggregated from the snapshot's own
    /// per-request detail. Catches summary/detail divergence, not a bad run.
    CrossChecked,
    /// The snapshot states it and the inputs needed to re-derive it are not
    /// published. Only local rebuild can confirm this one.
    Attested,
}

impl ClaimBasis {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimBasis::Recomputed => "recomputed",
            ClaimBasis::CrossChecked => "cross-checked",
            ClaimBasis::Attested => "attested",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimCheck {
    pub claim: String,
    /// The published value, rendered for display.
    pub value: String,
    pub status: ClaimStatus,
    pub basis: ClaimBasis,
    /// Populated on failure with what actually differed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ClaimCheck {
    fn new(
        claim: &str,
        value: impl Into<String>,
        ok: bool,
        basis: ClaimBasis,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            claim: claim.to_string(),
            value: value.into(),
            status: if ok {
                ClaimStatus::Pass
            } else {
                ClaimStatus::Fail
            },
            basis,
            detail: if ok { None } else { Some(detail.into()) },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicVerification {
    pub extract_path: String,
    pub experiment_id: String,
    pub schema_version: String,
    pub claims: Vec<ClaimCheck>,
}

impl PublicVerification {
    pub fn failed(&self) -> bool {
        self.claims.iter().any(|c| c.status == ClaimStatus::Fail)
    }

    pub fn passed_count(&self) -> usize {
        self.claims
            .iter()
            .filter(|c| c.status == ClaimStatus::Pass)
            .count()
    }
}

/// Load and schema-check the tracked publication extract.
pub fn load_published_extract(path: &Path) -> Result<EvidenceExtract> {
    if !path.exists() {
        bail!(
            "publication extract not found at {}\n\
             This file is tracked in the repository; a clean clone should have it. \
             If it was deleted, restore it with `git checkout -- {}`.",
            path.display(),
            path.display()
        );
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read publication extract {}", path.display()))?;
    let extract: EvidenceExtract = serde_json::from_str(&text).with_context(|| {
        format!(
            "parse publication extract {} — the file is corrupt or was written by an \
             incompatible version",
            path.display()
        )
    })?;
    if extract.schema_version != EVIDENCE_EXTRACT_SCHEMA {
        bail!(
            "publication extract schema_version '{}' is not supported (expected '{}')",
            extract.schema_version,
            EVIDENCE_EXTRACT_SCHEMA
        );
    }
    if extract.threshold_identity.detail.is_empty() {
        bail!("publication extract carries no threshold observations; nothing to verify");
    }
    if extract.per_request_search.is_empty() {
        bail!("publication extract carries no per-request byte searches; nothing to verify");
    }
    Ok(extract)
}

/// Sites recorded for one searched quantity, in one request.
fn sites_for<'a>(
    request: &'a evidence_extract::RequestSearch,
    quantity: &str,
) -> Option<&'a Vec<MatchSite>> {
    request
        .sites
        .iter()
        .find(|(name, _)| name == quantity)
        .map(|(_, sites)| sites)
}

fn encodings_for<'a>(
    request: &'a evidence_extract::RequestSearch,
    quantity: &str,
) -> Option<&'a Vec<String>> {
    request
        .encodings_hit
        .iter()
        .find(|(name, _)| name == quantity)
        .map(|(_, labels)| labels)
}

/// Verify every article claim that published data can support.
///
/// Nothing here trusts a summary field on its own: each summary is either
/// recomputed from published inputs or re-aggregated from published detail.
pub fn verify_public(extract: &EvidenceExtract) -> Vec<ClaimCheck> {
    let mut claims = Vec::new();
    let identity = &extract.threshold_identity;
    let candidate = &extract.candidate_result;

    // --- run shape -------------------------------------------------------
    let detail_rows = identity.detail.len();
    claims.push(ClaimCheck::new(
        "requests",
        extract.total_requests.to_string(),
        detail_rows == extract.total_requests && identity.observations == extract.total_requests,
        ClaimBasis::CrossChecked,
        format!(
            "total_requests={} but {detail_rows} observation rows / {} observations",
            extract.total_requests, identity.observations
        ),
    ));

    let batches: BTreeSet<usize> = identity.detail.iter().map(|d| d.batch_index).collect();
    claims.push(ClaimCheck::new(
        "brackets",
        extract.total_batches.to_string(),
        batches.len() == extract.total_batches,
        ClaimBasis::CrossChecked,
        format!(
            "total_batches={} but detail spans {} distinct batches",
            extract.total_batches,
            batches.len()
        ),
    ));

    let searched_batches: BTreeSet<usize> = extract
        .per_request_search
        .iter()
        .map(|r| r.batch_index)
        .collect();
    let published_eligible: BTreeSet<usize> = extract.eligible_batches.iter().copied().collect();
    claims.push(ClaimCheck::new(
        "eligible brackets",
        extract
            .eligible_batches
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(","),
        searched_batches == published_eligible,
        ClaimBasis::CrossChecked,
        format!("eligible_batches={published_eligible:?} but searches cover {searched_batches:?}"),
    ));

    claims.push(ClaimCheck::new(
        "eligibility rate",
        format!("{}/{}", extract.eligible_batch_count, extract.total_batches),
        extract.eligible_batch_count == extract.eligible_batches.len()
            && extract.eligible_batch_count <= extract.total_batches,
        ClaimBasis::CrossChecked,
        format!(
            "eligible_batch_count={} but {} eligible batches listed",
            extract.eligible_batch_count,
            extract.eligible_batches.len()
        ),
    ));

    // --- threshold arithmetic, recomputed from published inputs ----------
    let mut ceil_matches = 0usize;
    let mut floor_matches = 0usize;
    let mut arithmetic_errors = Vec::new();
    for row in &identity.detail {
        let (Ok(q), Ok(m)) = (
            row.out_amount.parse::<u128>(),
            row.other_amount_threshold.parse::<u128>(),
        ) else {
            arithmetic_errors.push(format!(
                "batch {} {} carries a non-integer amount",
                row.batch_index, row.role
            ));
            continue;
        };
        if ceil_threshold(q, row.slippage_bps) == Some(m) {
            ceil_matches += 1;
        }
        if floor_threshold(q, row.slippage_bps) == Some(m) {
            floor_matches += 1;
        }
    }
    claims.push(ClaimCheck::new(
        "threshold ceil identity",
        format!("{ceil_matches}/{detail_rows}"),
        arithmetic_errors.is_empty()
            && ceil_matches == detail_rows
            && identity.exact_matches == ceil_matches
            && identity.holds_for_all,
        ClaimBasis::Recomputed,
        format!(
            "recomputed {ceil_matches}/{detail_rows}; extract claims {} {}",
            identity.exact_matches,
            arithmetic_errors.join("; ")
        ),
    ));
    claims.push(ClaimCheck::new(
        "floor identity",
        format!("{floor_matches}/{detail_rows}"),
        floor_matches == identity.floor_variant_matches,
        ClaimBasis::Recomputed,
        format!(
            "recomputed {floor_matches}; extract claims {}",
            identity.floor_variant_matches
        ),
    ));

    // minOutAmount is not published per request — only the comparison result.
    claims.push(ClaimCheck::new(
        "minOut == threshold",
        if identity.min_out_equals_threshold_all {
            "all"
        } else {
            "not all"
        },
        identity.min_out_equals_threshold_all
            && identity.detail.iter().all(|d| d.min_out_equals_threshold),
        ClaimBasis::Attested,
        "summary and per-request flags disagree".to_string(),
    ));

    // --- byte search, re-aggregated from published detail ----------------
    let searched = extract.per_request_search.len();
    claims.push(ClaimCheck::new(
        "eligible tx searched",
        searched.to_string(),
        searched == candidate.transactions_searched && searched == extract.eligible_batch_count * 3,
        ClaimBasis::CrossChecked,
        format!(
            "{searched} searches, extract claims {}, {} eligible batches x 3 roles",
            candidate.transactions_searched, extract.eligible_batch_count
        ),
    ));

    let threshold_sites: usize = extract
        .per_request_search
        .iter()
        .filter_map(|r| sites_for(r, "otherAmountThreshold"))
        .map(|s| s.len())
        .sum();
    claims.push(ClaimCheck::new(
        "threshold literal matches",
        threshold_sites.to_string(),
        threshold_sites == candidate.threshold_sites_total,
        ClaimBasis::CrossChecked,
        format!(
            "detail sums to {threshold_sites}; extract claims {}",
            candidate.threshold_sites_total
        ),
    ));

    let difference_sites: usize = extract
        .per_request_search
        .iter()
        .filter_map(|r| sites_for(r, "outAmount_minus_threshold"))
        .map(|s| s.len())
        .sum();
    claims.push(ClaimCheck::new(
        "difference literal matches",
        difference_sites.to_string(),
        difference_sites == candidate.difference_sites_total,
        ClaimBasis::CrossChecked,
        format!(
            "detail sums to {difference_sites}; extract claims {}",
            candidate.difference_sites_total
        ),
    ));

    let quote_hits = extract
        .per_request_search
        .iter()
        .filter(|r| {
            sites_for(r, "outAmount")
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        })
        .count();
    claims.push(ClaimCheck::new(
        "quote literal matches",
        format!("{quote_hits}/{searched}"),
        quote_hits == searched && candidate.quote_matched_in_all,
        ClaimBasis::CrossChecked,
        format!("{quote_hits}/{searched} requests matched the quote"),
    ));

    let unique_hits = extract
        .per_request_search
        .iter()
        .filter(|r| {
            sites_for(r, "outAmount")
                .map(|s| s.len() == 1)
                .unwrap_or(false)
        })
        .count();
    claims.push(ClaimCheck::new(
        "unique quote matches",
        format!("{unique_hits}/{searched}"),
        unique_hits == searched && candidate.quote_site_unique_in_all,
        ClaimBasis::CrossChecked,
        format!("{unique_hits}/{searched} requests had exactly one site"),
    ));

    let distinct_sites: BTreeSet<(usize, usize)> = extract
        .per_request_search
        .iter()
        .filter_map(|r| sites_for(r, "outAmount"))
        .flatten()
        .map(|s| (s.instruction_index, s.byte_offset))
        .collect();
    let published_sites: BTreeSet<(usize, usize)> = candidate
        .quote_sites
        .iter()
        .map(|s| (s.instruction_index, s.byte_offset))
        .collect();
    let site_label = published_sites
        .iter()
        .map(|(i, o)| format!("ix{i}:{o}"))
        .collect::<Vec<_>>()
        .join(",");
    claims.push(ClaimCheck::new(
        "quote candidate site",
        if site_label.is_empty() {
            "none".into()
        } else {
            site_label
        },
        distinct_sites == published_sites && distinct_sites.len() == 1,
        ClaimBasis::CrossChecked,
        format!("detail sites {distinct_sites:?}; extract claims {published_sites:?}"),
    ));

    // Every hit is an 8-byte little-endian read. u32_le co-hits on the same
    // offset because the high four bytes are zero — the extract says so, and
    // the detail has to agree.
    let canonical_ok = extract
        .per_request_search
        .iter()
        .filter(|r| {
            sites_for(r, "outAmount")
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        })
        .all(|r| {
            encodings_for(r, "outAmount")
                .map(|labels| labels.iter().any(|l| l == "u64_le"))
                .unwrap_or(false)
        });
    claims.push(ClaimCheck::new(
        "canonical encoding",
        "u64_le",
        canonical_ok
            && candidate.canonical_encoding.contains("8-byte")
            && candidate.encoding_family.iter().any(|e| e == "u64_le"),
        ClaimBasis::CrossChecked,
        format!(
            "canonical_encoding='{}', per-request labels disagree",
            candidate.canonical_encoding
        ),
    ));

    // --- same-treatment control -----------------------------------------
    let controls = extract.anchor_control.len();
    let clean_controls = extract
        .anchor_control
        .iter()
        .filter(|p| {
            p.quote_differs && p.candidate_carries_own_quote && p.route_same && p.topology_same
        })
        .count();
    claims.push(ClaimCheck::new(
        "same-treatment controls",
        format!("{clean_controls}/{controls}"),
        controls == extract.eligible_batch_count && clean_controls == controls && controls > 0,
        ClaimBasis::CrossChecked,
        format!(
            "{clean_controls}/{controls} anchor pairs held, {} eligible batches",
            extract.eligible_batch_count
        ),
    ));

    // --- settlement -------------------------------------------------------
    let unsigned = extract.design.get("signed") == Some(&serde_json::json!(false));
    let unsubmitted = extract.design.get("submitted") == Some(&serde_json::json!(false));
    claims.push(ClaimCheck::new(
        "settlement",
        "unavailable",
        unsigned && unsubmitted,
        ClaimBasis::Attested,
        format!(
            "design.signed / design.submitted are not both false: {}",
            extract.design
        ),
    ));

    claims
}

/// Run public verification against the tracked extract.
pub fn run_public(base_dir: &Path, extract_path: Option<&Path>) -> Result<PublicVerification> {
    let path = match extract_path {
        Some(p) => p.to_path_buf(),
        None => base_dir.join(PUBLIC_EXTRACT_PATH),
    };
    let extract = load_published_extract(&path)?;
    Ok(PublicVerification {
        extract_path: path.display().to_string(),
        experiment_id: extract.experiment_id.clone(),
        schema_version: extract.schema_version.clone(),
        claims: verify_public(&extract),
    })
}

// ---- local rebuild -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDifference {
    pub path: String,
    pub regenerated: String,
    pub published: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildResult {
    pub recorded_run: String,
    pub published_extract: String,
    pub differences: Vec<FieldDifference>,
}

impl RebuildResult {
    pub fn matches(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Regenerate the extract from the recorded run and compare it to the tracked
/// publication snapshot. Reads files only — no network, no re-request.
pub fn run_local_rebuild(base_dir: &Path, extract_path: Option<&Path>) -> Result<RebuildResult> {
    let run_dir = base_dir.join(RECORDED_RUN_DIR);
    let report_path = run_dir.join("experiment_report.json");
    if !report_path.exists() {
        bail!(
            "recorded run not found at {}\n\
             Local rebuild needs the original captured run, which is not published: the raw \
             responses and unsigned transactions carry the requester's wallet pubkey.\n\
             Run without --from-recorded-run for public verification of the tracked evidence \
             snapshot.",
            run_dir.display()
        );
    }

    let text = std::fs::read_to_string(&report_path)
        .with_context(|| format!("read bracket report {}", report_path.display()))?;
    let report: BracketExperimentReport = serde_json::from_str(&text)
        .with_context(|| format!("parse bracket report {}", report_path.display()))?;

    let regenerated = evidence_extract::build(&report, base_dir)
        .context("rebuild evidence extract from the recorded run")?;

    let published_path = match extract_path {
        Some(p) => p.to_path_buf(),
        None => base_dir.join(PUBLIC_EXTRACT_PATH),
    };
    let published = load_published_extract(&published_path)?;

    Ok(RebuildResult {
        recorded_run: run_dir.display().to_string(),
        published_extract: published_path.display().to_string(),
        differences: diff_extracts(&regenerated, &published)?,
    })
}

/// Field-level comparison of two extracts.
///
/// `generated_by` is excluded: it names the tool that wrote the file, not an
/// empirical value. Everything else must match exactly.
pub fn diff_extracts(
    regenerated: &EvidenceExtract,
    published: &EvidenceExtract,
) -> Result<Vec<FieldDifference>> {
    let mut a = serde_json::to_value(regenerated)?;
    let mut b = serde_json::to_value(published)?;
    for v in [&mut a, &mut b] {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("generated_by");
        }
    }
    let mut out = Vec::new();
    diff_value("", &a, &b, &mut out);
    out.sort_by(|x, y| x.path.cmp(&y.path));
    Ok(out)
}

fn render(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 120 {
        format!("{}…", &s[..120])
    } else {
        s
    }
}

fn diff_value(
    path: &str,
    a: &serde_json::Value,
    b: &serde_json::Value,
    out: &mut Vec<FieldDifference>,
) {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                let child = if path.is_empty() {
                    k.to_string()
                } else {
                    format!("{path}.{k}")
                };
                match (x.get(k), y.get(k)) {
                    (Some(xv), Some(yv)) => diff_value(&child, xv, yv, out),
                    (Some(xv), None) => out.push(FieldDifference {
                        path: child,
                        regenerated: render(xv),
                        published: "<absent>".into(),
                    }),
                    (None, Some(yv)) => out.push(FieldDifference {
                        path: child,
                        regenerated: "<absent>".into(),
                        published: render(yv),
                    }),
                    (None, None) => unreachable!("key came from one of the two maps"),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(FieldDifference {
                    path: format!("{path}.len()"),
                    regenerated: x.len().to_string(),
                    published: y.len().to_string(),
                });
            }
            for (i, (xv, yv)) in x.iter().zip(y.iter()).enumerate() {
                diff_value(&format!("{path}[{i}]"), xv, yv, out);
            }
        }
        _ if a != b => out.push(FieldDifference {
            path: path.to_string(),
            regenerated: render(a),
            published: render(b),
        }),
        _ => {}
    }
}

/// One recorded raw response, run through the canonical production pipeline
/// (adapter → ExecutionContext → Solana extraction → LineageBuilder → verify).
///
/// Local mode only: raw responses are not published.
pub async fn verify_reference_artifact(
    base_dir: &Path,
    response: &Path,
) -> Result<crate::checks::VerificationReport> {
    let extraction = crate::extract::extract(crate::extract::ExtractInputs {
        provider: Some(crate::adapters::ProviderId::Dflow),
        response_path: Some(response),
        ..Default::default()
    })
    .await?;
    let _ = base_dir;
    Ok(crate::checks::verify(
        &extraction.context,
        &extraction.lineage,
    ))
}

/// First recorded response of the run, by sorted filename.
pub fn first_recorded_response(base_dir: &Path) -> Option<PathBuf> {
    let raw_dir = base_dir.join(RECORDED_RUN_DIR).join("raw");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(raw_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// Claim counts by basis, for the report footer.
pub fn basis_counts(claims: &[ClaimCheck]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for c in claims {
        *counts.entry(c.basis.as_str()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn published() -> Option<EvidenceExtract> {
        let path = tree().join(PUBLIC_EXTRACT_PATH);
        if !path.exists() {
            eprintln!("skip: {} not present", path.display());
            return None;
        }
        Some(load_published_extract(&path).unwrap())
    }

    #[test]
    fn public_verification_passes_on_the_tracked_extract() {
        let Some(extract) = published() else { return };
        let claims = verify_public(&extract);
        let failures: Vec<&ClaimCheck> = claims
            .iter()
            .filter(|c| c.status == ClaimStatus::Fail)
            .collect();
        assert!(failures.is_empty(), "failing claims: {failures:#?}");
        assert!(claims.len() >= 15, "expected the full claim table");
    }

    #[test]
    fn tampering_with_a_summary_is_caught() {
        let Some(mut extract) = published() else {
            return;
        };
        // Claim one threshold byte-match that the per-request detail does not
        // support — the exact way a published summary could drift from its
        // evidence.
        extract.candidate_result.threshold_sites_total = 1;
        let claims = verify_public(&extract);
        let claim = claims
            .iter()
            .find(|c| c.claim == "threshold literal matches")
            .unwrap();
        assert_eq!(claim.status, ClaimStatus::Fail);
        assert!(claim.detail.as_ref().unwrap().contains("detail sums to 0"));
    }

    #[test]
    fn tampering_with_a_published_amount_breaks_the_recomputed_identity() {
        let Some(mut extract) = published() else {
            return;
        };
        extract.threshold_identity.detail[0].other_amount_threshold = "1".into();
        let claims = verify_public(&extract);
        let claim = claims
            .iter()
            .find(|c| c.claim == "threshold ceil identity")
            .unwrap();
        assert_eq!(
            claim.status,
            ClaimStatus::Fail,
            "the identity must be recomputed, not read from the summary"
        );
    }

    #[test]
    fn missing_extract_fails_with_a_useful_message() {
        let err = load_published_extract(Path::new("/nonexistent/extract.json")).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn corrupt_extract_fails() {
        let dir = std::env::temp_dir().join(format!("refcase_corrupt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("extract.json");

        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_published_extract(&path)
            .unwrap_err()
            .to_string()
            .contains("parse"));

        std::fs::write(&path, r#"{"schema_version":"9.9.9"}"#).unwrap();
        assert!(load_published_extract(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_rebuild_without_the_recorded_run_explains_itself() {
        let dir = std::env::temp_dir().join(format!("refcase_norun_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = run_local_rebuild(&dir, None).unwrap_err().to_string();
        assert!(err.contains("recorded run not found"));
        assert!(err.contains("not published"));
        assert!(err.contains("--from-recorded-run"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_reports_the_exact_differing_field() {
        let Some(published) = published() else { return };
        let mut changed = published.clone();
        changed.total_requests = 29;
        changed.threshold_identity.exact_matches = 29;

        let diffs = diff_extracts(&changed, &published).unwrap();
        let paths: Vec<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"total_requests"));
        assert!(paths.contains(&"threshold_identity.exact_matches"));
        assert_eq!(diffs.len(), 2, "unrelated fields must not be reported");
    }

    #[test]
    fn identical_extracts_diff_clean() {
        let Some(published) = published() else { return };
        assert!(diff_extracts(&published, &published).unwrap().is_empty());
    }
}
