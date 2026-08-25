// Docs — landing page for everything demoted out of primary navigation.
//
// Structure: Quick Start, How it Works (both inline here), then Architecture
// and Reference (CLI, Rust types, reproducibility) as their existing, unedited
// pages. Nothing here is new content — it is content relocated out of the
// first-time path so a first-time visitor is not asked to read it.

import { el, link, prose, ascii, statusBadge, note } from "../../components/format.js";
import { renderGenericFlow } from "../../components/stage-flow.js";
import { state, render, repoUrl } from "../../app.js";

const CAPABILITIES = [
  [
    "Provider layer",
    ["provider-specific extraction", "normalized intent and response", "route observation"],
  ],
  [
    "Transaction layer",
    ["Solana legacy and v0 decoding", "ALT resolution", "exact loaded-account ordering", "program attribution"],
  ],
  ["Lineage layer", ["cross-stage relationships", "provenance for every observation", "an explicit ceiling on every claim"]],
  [
    "Verification layer",
    ["PASS / FAIL / CANDIDATE / UNKNOWN / NOT_APPLICABLE", "a candidate never silently becomes PASS"],
  ],
];

const ARCHITECTURE = `                 Execution Verifier Core
                         │
          ┌──────────────┼──────────────┐
          │              │              │
        DFlow          Jupiter        Generic
       adapter         adapter        adapter
          │
          ▼
 provider-specific extraction
          │
          ▼
 Normalized ExecutionContext
          │
          ▼
 generic Solana extraction
          │
          ▼
 LineageBundle
          │
          ▼
       verify`;

function quickStart(root) {
  root.append(el("h2", { text: "Quick start" }));
  root.append(
    el("pre", {
      text: [
        "cargo run -- extract --provider dflow --response capture.json --out-dir ./lineage",
        "cargo run -- trace --lineage ./lineage",
        "cargo run -- verify --lineage ./lineage --out-json ./lineage/verification.json",
      ].join("\n"),
    }),
  );
  root.append(
    el("p", { class: "prose" }, [
      "Then drop the resulting JSON into ",
      link("#/inspect", "Inspect lineage JSON"),
      " — read in your browser, never uploaded.",
    ]),
  );
}

function howItWorks(root) {
  root.append(el("h2", { text: "How it works" }));
  root.append(
    prose(
      "One order crosses up to five stages. Any of them can be missing — a missing stage means not observed, never observed-empty.",
    ),
  );
  renderGenericFlow(root, state.docsStage, (id) => {
    state.docsStage = id;
    render();
  });

  root.append(el("h3", { text: "capability layers" }));
  const grid = el("div", { class: "doc-grid" });
  for (const [title, items] of CAPABILITIES) {
    grid.append(
      el("div", { class: "doc-card" }, [
        el("h4", { text: title }),
        el(
          "ul",
          {},
          items.map((i) => el("li", { text: i })),
        ),
      ]),
    );
  }
  root.append(grid);

  const statuses = el("div", { class: "controls" });
  for (const s of ["PASS", "FAIL", "CANDIDATE", "UNKNOWN", "NOT_APPLICABLE"]) statuses.append(statusBadge(s));
  root.append(statuses);

  root.append(el("h3", { text: "architecture, in outline" }));
  root.append(
    prose(
      "Provider adapters are interchangeable; the core is not provider-specific. DFlow is the first complete integration, not the boundary of the tool.",
    ),
  );
  root.append(ascii(ARCHITECTURE));
  root.append(
    el("p", { class: "prose" }, [link("#/docs/architecture", "Read the full architecture →")]),
  );
}

function referenceLinks(root) {
  root.append(el("h2", { text: "Reference" }));
  const grid = el("div", { class: "doc-grid" });
  grid.append(
    el("a", { class: "doc-card doc-card-link", href: "#/docs/architecture" }, [
      el("h4", { text: "Architecture" }),
      el("p", { text: "Provider boundary, ExecutionContext, Solana extraction, verification, evidence model." }),
    ]),
  );
  grid.append(
    el("a", { class: "doc-card doc-card-link", href: "#/docs/reference" }, [
      el("h4", { text: "CLI" }),
      el("p", { text: "extract, trace, verify and every other subcommand, with principal flags." }),
    ]),
  );
  grid.append(
    el("a", { class: "doc-card doc-card-link", href: "#/docs/reference" }, [
      el("h4", { text: "Rust reference" }),
      el("p", { text: "ProviderAdapter, ExecutionContext, LineageBundle, CheckResult and status semantics." }),
    ]),
  );
  grid.append(
    el("a", { class: "doc-card doc-card-link", href: "#/docs/reference" }, [
      el("h4", { text: "Reproducibility" }),
      el("p", { text: "Public verification vs. local rebuild, and the gate commands." }),
    ]),
  );
  root.append(grid);
  root.append(
    note(
      "Architecture and Reference are unedited: everything documented there before is still there.",
    ),
  );
}

export function renderDocsIndex(root) {
  root.append(el("p", { class: "eyebrow", text: "Docs" }));
  root.append(el("h1", { text: "Docs" }));
  root.append(
    el("p", {
      class: "lede",
      text: "Quick start, how the verifier is put together, and the CLI/Rust reference.",
    }),
  );

  quickStart(root);
  howItWorks(root);
  referenceLinks(root);

  root.append(
    el("p", { class: "prose" }, [
      "Source: ",
      el("a", { href: repoUrl(), text: "github.com/egpivo/onchain-execution-lineage", rel: "noopener" }),
    ]),
  );
}
