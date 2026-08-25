// Explore — the use-case collection. Cards, and little else.
//
// Entirely data-driven from web/data/use-cases.json: adding a case is a config
// entry plus a view module, never a navigation rewrite. Every figure on a card
// is declared as a path into a tracked evidence artifact and read at render
// time — no count is written into this file.

import { el, link, badge, metric, note, groupDigits } from "../components/format.js";
import { data, artifact, readPath } from "../app.js";

function caseCard(useCase) {
  const card = el("a", { class: "case-card", href: `#/explore/${useCase.id}` });

  card.append(
    el("div", { class: "case-head" }, [
      el("span", { class: "case-title", text: useCase.title }),
      el(
        "span",
        { class: "badges" },
        (useCase.types || []).map((t) => badge(t, "type")),
      ),
    ]),
  );

  card.append(el("p", { class: "case-question", text: useCase.question }));

  card.append(
    el("div", { class: "badges" }, [
      ...(useCase.providers || []).map((p) => badge(p)),
      badge(useCase.chain),
      useCase.surface ? badge(useCase.surface) : null,
    ]),
  );

  const metrics = el("div", { class: "metrics" });
  let shown = 0;
  for (const m of useCase.metrics || []) {
    const value = readPath(artifact(m.artifact), m.path);
    if (value === undefined) continue;
    metrics.append(metric(m.label, groupDigits(String(value))));
    shown += 1;
    if (shown === 4) break;
  }
  // Figures appear only when their artifact loaded. There is no fallback.
  card.append(
    shown ? metrics : note("Evidence artifact unavailable — figures omitted rather than guessed."),
  );

  card.append(
    el("span", { class: "cta-text" }, ["Explore ", el("span", { "aria-hidden": "true", text: "→" })]),
  );

  return card;
}

export function renderExplore(root) {
  root.append(el("p", { class: "eyebrow", text: "Explore" }));
  root.append(el("h1", { text: "Explore" }));

  const cases = data.useCases?.cases || [];
  if (!cases.length) {
    root.append(note("No use cases are configured."));
    return;
  }

  const grid = el("div", { class: "case-grid" });
  for (const useCase of cases) grid.append(caseCard(useCase));
  root.append(grid);

  const planned = data.useCases?.planned || [];
  if (planned.length) {
    root.append(el("h2", { text: "Planned" }));
    root.append(
      el(
        "div",
        { class: "badges" },
        planned.map((p) => badge(p)),
      ),
    );
  }

  root.append(
    el("p", { class: "note" }, [
      "Have your own execution to check? ",
      link("#/inspect", "Inspect lineage JSON"),
      " produced by the CLI.",
    ]),
  );
}
