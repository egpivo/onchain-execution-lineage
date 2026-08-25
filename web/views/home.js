// Home — the first screen. Answers three questions only: what this does,
// whether there's a real example, and how to inspect your own execution.
// Everything else (capability breakdown, architecture diagram, quick start,
// current provider/chain coverage) lives under Docs, one click away.

import { el, link, note, metric, groupDigits } from "../components/format.js";
import { renderMiniFlow } from "../components/mini-flow.js";
import { data, artifact, readPath, repoUrl, state, render } from "../app.js";

// Curated subset of the case's declared metrics, for a three-figure glance.
// The values still come from the tracked artifact; this only picks which
// declared labels are worth showing on a first screen.
const HOME_METRICS = ["requests", "brackets", "transactions searched"];

function hero(root) {
  const hero = el("div", { class: "hero hero-home" });
  hero.append(el("h1", { text: "Onchain Execution Lineage" }));
  hero.append(
    el("p", {
      class: "lede",
      text: "Trace what a provider promised into what the transaction encoded and what the chain eventually executed.",
    }),
  );
  hero.append(
    el("p", {
      class: "prose",
      text: "Read-only Rust tooling for execution reconstruction and verification.",
    }),
  );
  hero.append(
    el("div", { class: "actions" }, [
      link("#/explore", "Explore a real case", { class: "btn btn-primary" }),
      link("#/inspect", "Inspect lineage JSON", { class: "btn" }),
    ]),
  );
  hero.append(
    el("p", { class: "prose muted", style: "margin-top: 12px" }, [
      el("a", { href: repoUrl(), text: "View source on GitHub ↗", rel: "noopener" }),
    ]),
  );
  root.append(hero);
}

function executionPath(root) {
  renderMiniFlow(root, state.homeStage, (id) => {
    state.homeStage = id;
    render();
  });
}

function featuredCase(root) {
  const featured = (data.useCases?.cases || [])[0];
  if (!featured) return;

  const extract = artifact(featured.metrics?.[0]?.artifact);
  root.append(el("h2", { text: "A real example" }));

  const card = el("a", { class: "case-card case-card-plain", href: `#/explore/${featured.id}` });
  card.append(el("span", { class: "case-title", text: featured.title }));

  if (extract) {
    const metrics = el("div", { class: "metrics" });
    for (const m of featured.metrics || []) {
      if (!HOME_METRICS.includes(m.label)) continue;
      const value = readPath(artifact(m.artifact), m.path);
      if (value === undefined) continue;
      metrics.append(metric(m.label, groupDigits(String(value))));
    }
    card.append(metrics);
    card.append(
      el("p", {
        class: "prose",
        text: "The quoted output amount occurs verbatim in instruction bytes in every eligible transaction searched. The threshold does not, under the tested encoding family.",
      }),
    );
  } else {
    card.append(note("Evidence artifact unavailable — figures are not shown rather than guessed."));
  }

  card.append(
    el("span", { class: "cta-text" }, [
      "Explore this case ",
      el("span", { "aria-hidden": "true", text: "→" }),
    ]),
  );
  root.append(card);
}

export function renderHome(root) {
  hero(root);
  executionPath(root);
  featuredCase(root);
}
