// Verification view: Rust CheckResult records, grouped by execution stage.
//
// Statuses are rendered exactly as Rust emitted them. There is no code path
// here that maps one status onto another, and CANDIDATE / UNKNOWN never share
// styling or wording with PASS or FAIL.

import {
  el,
  statusBadge,
  statusKey,
  definitionList,
  chips,
  stageLabel,
  note,
} from "./format.js";

const STAGE_ORDER = [
  "intent",
  "provider_response",
  "route",
  "transaction_construction",
  "settlement",
];

const STATUS_ORDER = ["FAIL", "CANDIDATE", "UNKNOWN", "PASS", "NOT_APPLICABLE"];

/** How each status should be read. Wording is deliberately not interchangeable. */
const MEANING = {
  PASS: "The stated relationship holds on observed evidence.",
  FAIL: "The relationship is contradicted by observed evidence.",
  CANDIDATE:
    "Consistent with the claim, but the evidence cannot separate it from coincidence. A candidate is not a weak pass.",
  UNKNOWN: "The evidence could exist but was not observed. This is not a failure.",
  NOT_APPLICABLE: "The check does not apply to this artifact at all.",
};

function primaryStage(result) {
  const stages = result.stages || [];
  for (const stage of STAGE_ORDER) if (stages.includes(stage)) return stage;
  return "provider_response";
}

function checkCard(result) {
  const key = statusKey(result.status);
  const card = el("div", { class: "card", dataset: { status: key, checkId: result.check_id } });

  card.append(
    el("div", { class: "card-head" }, [
      el("span", { class: "card-id", text: result.check_id }),
      statusBadge(key),
    ]),
  );
  card.append(el("p", { class: "lede", text: result.explanation }));
  card.append(
    definitionList([
      ["observed", result.observed],
      ["expected", result.expected],
      ["stages", (result.stages || []).map(stageLabel).join(", ")],
      ["provider", result.provider],
      ["status means", MEANING[key]],
    ]),
  );

  if ((result.evidence || []).length) {
    card.append(el("h3", { text: "evidence" }));
    card.append(chips(result.evidence));
  }
  if ((result.provenance || []).length) {
    card.append(el("h3", { text: "provenance" }));
    card.append(chips(result.provenance));
  }
  card.append(el("p", { class: "ceiling", text: result.evidence_ceiling }));
  return card;
}

export function renderChecks(root, bundleSet, state, onFilter, opts = {}) {
  const report = bundleSet.verification;
  root.append(el("h2", { text: "Checks" }));

  if (!report) {
    root.append(
      note(
        "No verification report loaded. Produce one with `onchain-execution-lineage verify --lineage <dir> --out-json verification.json`, then load it here.",
      ),
    );
    return;
  }

  root.append(
    el("p", { class: "lede" }, [
      "Check results are not booleans. ",
      el("strong", { text: "A candidate never becomes a pass, and an unknown is not a failure." }),
    ]),
  );

  const summary = report.summary || {};
  const counts = {
    PASS: summary.pass || 0,
    FAIL: summary.fail || 0,
    CANDIDATE: summary.candidate || 0,
    UNKNOWN: summary.unknown || 0,
    NOT_APPLICABLE: summary.not_applicable || 0,
  };

  if (!opts.hideSummary) {
    const row = el("div", { class: "summary-row" });
    for (const status of STATUS_ORDER) {
      row.append(
        el("span", {}, [statusBadge(status), el("span", { class: "mono", text: ` ${counts[status]}` })]),
      );
    }
    root.append(row);
  }

  const controls = el("div", { class: "controls" }, [el("span", { class: "chip", text: "filter" })]);
  for (const status of ["ALL", ...STATUS_ORDER]) {
    const active = (state.status || "ALL") === status;
    controls.append(
      el("button", {
        class: "control",
        type: "button",
        "aria-pressed": String(active),
        text: status === "ALL" ? "all" : status.replace("_", " ").toLowerCase(),
        onclick: () => onFilter(status),
      }),
    );
  }
  root.append(controls);

  const filter = state.status || "ALL";
  const shown = (report.results || []).filter(
    (r) => filter === "ALL" || statusKey(r.status) === filter,
  );

  if (!shown.length) {
    root.append(note("No checks with that status in this report."));
    return;
  }

  for (const stage of STAGE_ORDER) {
    const group = shown.filter((r) => primaryStage(r) === stage);
    if (!group.length) continue;
    root.append(el("h3", { text: stageLabel(stage) }));
    group.sort((a, b) => STATUS_ORDER.indexOf(statusKey(a.status)) - STATUS_ORDER.indexOf(statusKey(b.status)));
    for (const result of group) root.append(checkCard(result));
  }
}
