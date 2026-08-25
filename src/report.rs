//! Report writers: Markdown, CSV evidence table, Graphviz DOT.

use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::diff::LineageDiff;
use crate::evidence::EvidenceLevel;
use crate::lineage_model::{classify_bucket, LineageBundle};

pub fn write_markdown_report(bundle: &LineageBundle, path: &Path) -> Result<()> {
    let mut recovered = Vec::new();
    let mut candidate = Vec::new();
    let mut unresolved = Vec::new();
    for c in &bundle.claims {
        let line = format!(
            "- {} — {} — {} ({:?}; {})",
            c.subject, c.predicate, c.object, c.evidence_level, c.explanation
        );
        match classify_bucket(c.evidence_level) {
            "recovered" => recovered.push(line),
            "candidate" => candidate.push(line),
            _ => unresolved.push(line),
        }
    }
    for u in &bundle.unresolved {
        unresolved.push(format!("- {} — {}", u.field, u.reason));
    }

    let mut md = String::new();
    md.push_str("# Lineage trace report\n\n");
    md.push_str(&format!(
        "Artifact `{}` · provider `{}` · surface `{}` · schema `{}`\n\n",
        bundle.capture.artifact_id,
        bundle.capture.provider,
        bundle.capture.surface,
        bundle.schema_version
    ));
    md.push_str("## Recovered\n\n");
    if recovered.is_empty() {
        md.push_str("_None._\n\n");
    } else {
        md.push_str(&recovered.join("\n"));
        md.push_str("\n\n");
    }
    md.push_str("## Candidate\n\n");
    if candidate.is_empty() {
        md.push_str("_None._\n\n");
    } else {
        md.push_str(&candidate.join("\n"));
        md.push_str("\n\n");
    }
    md.push_str("## Unresolved\n\n");
    if unresolved.is_empty() {
        md.push_str("_None._\n\n");
    } else {
        md.push_str(&unresolved.join("\n"));
        md.push_str("\n\n");
    }
    md.push_str("## Not applicable\n\n");
    if !bundle.settlement.applicable {
        md.push_str(
            "- settlement — this artifact is unsigned or no signature was supplied; \
             never describe an unsigned instruction as executed.\n",
        );
    } else {
        md.push_str("_Settlement examined; see settlement section in JSON._\n");
    }
    fs::write(path, md)?;
    Ok(())
}

pub fn write_evidence_csv(bundle: &LineageBundle, path: &Path) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "subject",
        "predicate",
        "object",
        "evidence_level",
        "source_artifact_id",
        "source_field",
        "instruction_index",
        "explanation",
    ])?;
    for c in &bundle.claims {
        wtr.write_record([
            c.subject.clone(),
            c.predicate.clone(),
            c.object.clone(),
            format!("{:?}", c.evidence_level),
            c.source_artifact_id.clone(),
            c.source_field.clone().unwrap_or_default(),
            c.instruction_index
                .map(|i| i.to_string())
                .unwrap_or_default(),
            c.explanation.clone(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

pub fn write_dot(bundle: &LineageBundle, path: &Path) -> Result<()> {
    let mut f = fs::File::create(path)?;
    writeln!(f, "digraph lineage {{")?;
    writeln!(f, "  rankdir=LR;")?;
    writeln!(
        f,
        "  app [label=\"app/surface\\n{}\\n{}\"];",
        escape(&bundle.capture.provider),
        escape(&bundle.capture.surface)
    )?;
    writeln!(
        f,
        "  provider [label=\"execution provider\\n{}\"];",
        escape(&bundle.capture.provider)
    )?;
    writeln!(f, "  app -> provider [label=\"claimed_by\", style=solid];")?;

    for (i, leg) in bundle.route.legs.iter().enumerate() {
        let id = format!("venue{i}");
        writeln!(
            f,
            "  {id} [label=\"venue\\n{}\"];",
            escape(&leg.venue_or_label)
        )?;
        writeln!(
            f,
            "  provider -> {id} [label=\"returned_by\", style=dashed];"
        )?;
    }

    for (i, pid) in bundle
        .transaction_construction
        .program_labels
        .iter()
        .enumerate()
    {
        let id = format!("prog{i}");
        writeln!(f, "  {id} [label=\"program\\n{}\"];", escape(pid))?;
        let style = if pid.starts_with("candidate_") {
            "dotted"
        } else {
            "solid"
        };
        writeln!(f, "  provider -> {id} [label=\"invokes\", style={style}];")?;
    }

    if bundle.settlement.applicable {
        writeln!(f, "  settlement [label=\"settlement\\nexamined\"];")?;
        writeln!(
            f,
            "  provider -> settlement [label=\"settled_as\", style=solid];"
        )?;
    } else {
        writeln!(
            f,
            "  settlement [label=\"settlement\\nunresolved\", style=dashed];"
        )?;
        writeln!(
            f,
            "  provider -> settlement [label=\"unresolved_after\", style=dotted];"
        )?;
    }

    // Evidence-level edges from claims (subset).
    for (i, c) in bundle.claims.iter().take(12).enumerate() {
        let style = match c.evidence_level {
            EvidenceLevel::Candidate | EvidenceLevel::Unresolved => "dotted",
            EvidenceLevel::CrossArtifactInference => "dashed",
            _ => "solid",
        };
        writeln!(
            f,
            "  claim{i} [shape=note, label=\"{}\\n{}\"];",
            escape(&c.predicate),
            escape(&c.object)
        )?;
        writeln!(
            f,
            "  app -> claim{i} [label=\"{:?}\", style={style}];",
            c.evidence_level
        )?;
    }

    writeln!(f, "}}")?;
    Ok(())
}

pub fn write_diff_markdown(diff: &LineageDiff, path: &Path) -> Result<()> {
    let mut md = String::new();
    md.push_str("# Lineage diff\n\n");
    md.push_str(&format!(
        "Left `{}` vs right `{}`\n\n",
        diff.left_artifact_id, diff.right_artifact_id
    ));
    md.push_str(&format!(
        "Shared programs: {}\n\nOnly left: {}\n\nOnly right: {}\n\n",
        diff.shared_programs.join(", "),
        diff.programs_only_left.join(", "),
        diff.programs_only_right.join(", ")
    ));
    md.push_str("| field | left | right | class | note |\n|---|---|---|---|---|\n");
    for e in &diff.entries {
        md.push_str(&format!(
            "| {} | {} | {} | {:?} | {} |\n",
            e.field, e.left, e.right, e.class, e.note
        ));
    }
    fs::write(path, md)?;
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\"', "\\\"")
}
