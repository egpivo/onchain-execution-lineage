// Cross-stage relationships: LineageBundle.links.
//
// A link is a join Rust recorded between two stages, with an evidence level and
// a claim ceiling. The ceiling is rendered next to every relationship, always —
// a candidate byte match must never read as decoding.

import { el, definitionList, chips, stageLabel, note, table } from "./format.js";

const RELATIONSHIP = {
  same_value: { glyph: "=", label: "same value" },
  value_mismatch: { glyph: "≠", label: "value mismatch" },
  candidate_byte_match: { glyph: "◈", label: "candidate byte match" },
  not_recoverable: { glyph: "?", label: "not recoverable" },
  derived_from: { glyph: "→", label: "derived from" },
};

export function relationshipBadge(relationship) {
  const spec = RELATIONSHIP[relationship] || { glyph: "·", label: relationship };
  return el("span", { class: `rel rel-${relationship}` }, [
    el("span", { "aria-hidden": "true", text: `${spec.glyph} ` }),
    el("span", { text: spec.label }),
  ]);
}

function linkDetail(link) {
  const card = el("div", { class: "card" });
  card.append(
    el("div", { class: "card-head" }, [
      el("span", { class: "card-id", text: link.id }),
      relationshipBadge(link.relationship),
    ]),
  );
  card.append(
    el("p", { class: "lede" }, [
      el("span", { class: "mono", text: stageLabel(link.from_stage) }),
      " → ",
      el("span", { class: "mono", text: stageLabel(link.to_stage) }),
    ]),
  );
  card.append(
    definitionList([
      ["source evidence", link.subject],
      ["target evidence", link.object],
      ["relationship", link.relationship],
      ["evidence level", link.evidence_level],
      ["explanation", link.explanation],
    ]),
  );
  if ((link.evidence || []).length) {
    card.append(el("h3", { text: "evidence" }));
    card.append(chips(link.evidence));
  }
  card.append(el("p", { class: "ceiling", text: link.claim_ceiling }));

  if (link.relationship === "candidate_byte_match") {
    card.append(
      note(
        "A byte match shows the integer's encoding occurs in an instruction payload. It does not show the program reads those bytes as that quantity. No IDL, no official decoder, no protocol schema was used.",
      ),
    );
  }
  if (link.relationship === "not_recoverable") {
    card.append(
      note(
        "Non-recovery is not evidence of absence: the value may be encoded differently, derived at runtime, or carried in an account rather than a payload.",
      ),
    );
  }
  return card;
}

export function renderLinks(root, bundleSet, state, onSelect) {
  const links = bundleSet.lineage?.links || [];
  root.append(el("h2", { text: "Lineage relationships" }));
  root.append(
    el("p", { class: "lede" }, [
      "What the verifier could and could not join across stages. ",
      el("strong", { text: "Every relationship carries a claim ceiling, including the ones that failed to recover." }),
    ]),
  );

  if (!links.length) {
    root.append(note("This lineage carries no cross-stage links — the artifact has fewer than two observed stages."));
    return;
  }

  const rows = links.map((link) =>
    el(
      "tr",
      {
        class: "link-row",
        tabindex: "0",
        "aria-selected": String(state.link === link.id),
        onclick: () => onSelect(link.id),
        onkeydown: (e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect(link.id);
          }
        },
      },
      [
        el("td", {}, [relationshipBadge(link.relationship)]),
        el("td", { class: "mono", text: link.subject }),
        el("td", { class: "mono", text: link.object }),
        el("td", { class: "mono", text: link.evidence_level }),
      ],
    ),
  );
  root.append(table(["relationship", "source", "target", "evidence level"], rows));

  const selected = links.find((l) => l.id === state.link) || links[0];
  root.append(linkDetail(selected));
}
