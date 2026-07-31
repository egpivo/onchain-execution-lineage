//! Corpus groups and fingerprint promotion rules (refuse n=1).

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::lineage_model::LineageBundle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusEntry {
    pub artifact_id: String,
    pub group: String,
    pub lineage_path: String,
    #[serde(default)]
    pub smoke_or_synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusManifest {
    pub schema_version: String,
    pub entries: Vec<CorpusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintReport {
    pub schema_version: String,
    pub target_group: String,
    pub sample_size: usize,
    pub control_sample_size: usize,
    pub stable_across_group: Vec<String>,
    pub variable_within_group: Vec<String>,
    pub present_in_controls: Vec<String>,
    pub absent_in_controls: Vec<String>,
    pub candidate_unique_fields: Vec<String>,
    pub insufficient_sample: bool,
    pub refusals: Vec<String>,
}

pub fn load_corpus(path: &Path) -> Result<CorpusManifest> {
    let text = fs::read_to_string(path)?;
    let m: CorpusManifest = serde_json::from_str(&text)?;
    Ok(m)
}

pub fn fingerprint_group(
    corpus: &CorpusManifest,
    base_dir: &Path,
    target_group: &str,
) -> Result<FingerprintReport> {
    let mut target = Vec::new();
    let mut controls = Vec::new();
    for e in &corpus.entries {
        if e.smoke_or_synthetic {
            continue; // never treat smoke/synthetic as evidence
        }
        let path = base_dir.join(&e.lineage_path);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read lineage {}", path.display()))?;
        let bundle: LineageBundle = serde_json::from_str(&text)?;
        let feats = feature_set(&bundle);
        if e.group == target_group {
            target.push(feats);
        } else {
            controls.push(feats);
        }
    }

    let mut refusals = Vec::new();
    let insufficient = target.len() < 2;
    if insufficient {
        refusals.push(
            "insufficient sample: no field is app-specific from n<2 in the target group".into(),
        );
    }
    refusals
        .push("DFlow program IDs are never JTX-specific; provider-generic by definition".into());
    refusals.push("wallet and fee-payer fields require wallet controls before promotion".into());

    if target.is_empty() {
        bail!("no non-synthetic entries for group {target_group}");
    }

    let stable = intersection_all(&target);
    let union = union_all(&target);
    let variable: Vec<_> = union.difference(&stable).cloned().collect();
    let control_union = union_all(&controls);
    let present_in_controls: Vec<_> = stable.intersection(&control_union).cloned().collect();
    let absent_in_controls: Vec<_> = stable.difference(&control_union).cloned().collect();

    // Candidate unique = stable in target, absent in controls — but never
    // auto-promoted when sample is insufficient.
    let candidate_unique_fields = if insufficient {
        Vec::new()
    } else {
        absent_in_controls.clone()
    };

    if !insufficient && candidate_unique_fields.is_empty() {
        refusals
            .push("no candidate unique fields after negative controls; nothing to promote".into());
    }

    Ok(FingerprintReport {
        schema_version: crate::evidence::LINEAGE_SCHEMA_VERSION.to_string(),
        target_group: target_group.into(),
        sample_size: target.len(),
        control_sample_size: controls.len(),
        stable_across_group: stable.into_iter().collect(),
        variable_within_group: variable,
        present_in_controls,
        absent_in_controls,
        candidate_unique_fields,
        insufficient_sample: insufficient,
        refusals,
    })
}

fn feature_set(b: &LineageBundle) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for p in &b.transaction_construction.program_labels {
        // Explicitly drop provider-generic DFlow label from "unique" consideration later
        // by tagging it.
        if p == "dflow_aggregator_v4" {
            s.insert("provider:dflow_aggregator_v4".into());
        } else {
            s.insert(format!("program_label:{p}"));
        }
    }
    if let Some(fp) = &b.transaction_construction.fee_payer {
        s.insert(format!("fee_payer:{fp}"));
    }
    if let Some(r) = &b.route.provider_route_label {
        s.insert(format!("route_label:{r}"));
    }
    s.insert(format!("tx_present:{}", b.transaction_construction.present));
    s
}

fn intersection_all(sets: &[BTreeSet<String>]) -> BTreeSet<String> {
    let mut iter = sets.iter();
    let mut acc = iter.next().cloned().unwrap_or_default();
    for s in iter {
        acc = acc.intersection(s).cloned().collect();
    }
    acc
}

fn union_all(sets: &[BTreeSet<String>]) -> BTreeSet<String> {
    let mut acc = BTreeSet::new();
    for s in sets {
        acc.extend(s.iter().cloned());
    }
    acc
}

/// Helper used by tests: n=1 must refuse candidates.
pub fn n1_refuses_unique_promotion(report: &FingerprintReport) -> bool {
    report.insufficient_sample && report.candidate_unique_fields.is_empty()
}

pub fn group_counts(corpus: &CorpusManifest) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for e in &corpus.entries {
        *m.entry(e.group.clone()).or_insert(0) += 1;
    }
    m
}
