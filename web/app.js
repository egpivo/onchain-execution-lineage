// Onchain Execution Lineage — project site.
//
// Router, data loading, navigation. The only network activity is same-origin
// reads of tracked artifacts under ./data/ and ./samples/. No backend, no
// third-party script, no telemetry, no storage.
//
// Primary navigation is exactly Home / Explore / Docs / GitHub — everything
// else (Architecture, Reference, Load Lineage) was demoted under Docs or into
// a task-shaped CTA ("Inspect lineage JSON"), not deleted. Old hash paths
// (#/overview, #/use-cases/..., #/architecture, #/reference, #/load) still
// resolve: parseHash() aliases every one of them to its new destination, so
// links written against the previous navigation keep working.

import { el, errorBox } from "./components/format.js";
import { renderHome } from "./views/home.js";
import { renderExplore } from "./views/explore.js";
import { renderDocsIndex } from "./views/docs/index.js";
import { renderArchitecture } from "./views/architecture.js";
import { renderReference } from "./views/reference.js";
import { renderInspect } from "./views/inspect.js";
import { renderDflowSlippage } from "./views/dflow-slippage/index.js";
import { renderDflowFee } from "./views/dflow-fee.js";

const REPO = "https://github.com/egpivo/onchain-execution-lineage";

/** Everything the site can render, loaded once. */
export const data = {
  useCases: null,
  artifacts: {},
  /** Generic lineage inspector state — bundled sample or user-loaded. */
  lineage: { context: null, lineage: null, verification: null, source: "bundled" },
};

export const state = {
  route: "home",
  caseId: null,
  sub: "",
  docSection: "",
  params: new URLSearchParams(),
  // Per-view UI state, none of it empirical.
  stage: "provider_response",
  homeStage: "quote",
  docsStage: "provider_response",
  status: "ALL",
  link: null,
  batch: null,
  mode: "structural",
  quantity: null,
  sample: "dflow-order",
  // Inspect: "picker" (no result shown yet) or "result" (title/checklist/tabs).
  inspectMode: "picker",
  inspectPanel: "execution",
};

async function loadJson(path) {
  const response = await fetch(path, { cache: "no-cache" });
  if (!response.ok) throw new Error(`${path} (HTTP ${response.status})`);
  return response.json();
}

/** Read a declared metric path out of a loaded artifact. Display-only. */
export function readPath(object, path) {
  return path.split(".").reduce((acc, key) => {
    if (acc === undefined || acc === null) return undefined;
    if (key === "length") return Array.isArray(acc) ? acc.length : undefined;
    // Own properties only: a declared path addresses data in the artifact,
    // never an inherited member of the object that happens to carry it.
    return Object.hasOwn(acc, key) ? acc[key] : undefined;
  }, object);
}

export function artifact(name) {
  return data.artifacts[name] || null;
}

export function useCase(id) {
  return (data.useCases?.cases || []).find((c) => c.id === id) || null;
}

export function repoUrl(path = "") {
  return path ? `${REPO}/blob/main/${path}` : REPO;
}

// ---- routing ---------------------------------------------------------

/** Old "load" panel names → new inspect tab ids. "load" itself means "show the picker". */
const LEGACY_INSPECT_PANEL = { lineage: "execution", checks: "checks", links: "execution", load: "load" };

function parseHash() {
  const raw = location.hash.replace(/^#\/?/, "");
  const [path, query] = raw.split("?");
  const parts = path.split("/").filter(Boolean);
  state.params = new URLSearchParams(query || "");

  state.caseId = null;
  state.sub = "";
  state.docSection = "";

  const head = parts[0] || "";

  // Canonical routes.
  if (head === "explore") {
    state.route = "explore";
    state.caseId = parts[1] || null;
    state.sub = parts[2] || "";
    return;
  }
  if (head === "docs") {
    state.route = "docs";
    state.docSection = parts[1] || "";
    return;
  }
  if (head === "inspect") {
    state.route = "inspect";
    const panel = state.params.get("panel");
    if (panel === "load") state.inspectMode = "picker";
    else if (panel) {
      state.inspectMode = "result";
      state.inspectPanel = panel;
    }
    return;
  }
  if (head === "" || head === "home") {
    state.route = "home";
    return;
  }

  // Legacy aliases — the previous navigation's hash paths, kept resolving.
  if (head === "overview") {
    state.route = "home";
    return;
  }
  if (head === "use-cases") {
    state.route = "explore";
    state.caseId = parts[1] || null;
    state.sub = parts[2] || "";
    return;
  }
  if (head === "architecture") {
    state.route = "docs";
    state.docSection = "architecture";
    return;
  }
  if (head === "reference") {
    state.route = "docs";
    state.docSection = "reference";
    return;
  }
  if (head === "load") {
    state.route = "inspect";
    const mapped = LEGACY_INSPECT_PANEL[state.params.get("panel")];
    if (mapped === "load" || !mapped) state.inspectMode = "picker";
    else {
      state.inspectMode = "result";
      state.inspectPanel = mapped;
    }
    return;
  }

  state.route = "home";
}

const NAV = [
  ["Home", "#/", "home"],
  ["Explore", "#/explore", "explore"],
  ["Docs", "#/docs", "docs"],
];

function renderNav() {
  const nav = document.getElementById("nav");
  nav.replaceChildren();
  for (const [label, href, route] of NAV) {
    nav.append(
      el("a", { href, text: label, "aria-current": state.route === route ? "page" : undefined }),
    );
  }
  nav.append(el("a", { href: REPO, text: "GitHub ↗", rel: "noopener" }));
}

const CASE_VIEWS = {
  "dflow-slippage": renderDflowSlippage,
  "dflow-platform-fee": renderDflowFee,
};

export function render() {
  renderNav();
  const view = document.getElementById("view");
  view.replaceChildren();
  document.title =
    state.route === "home" ? "Onchain Execution Lineage" : `${state.route} — Onchain Execution Lineage`;

  switch (state.route) {
    case "explore":
      if (state.caseId) {
        // hasOwn, not a truthy lookup: the id comes from the URL, and
        // "constructor" would otherwise resolve to an inherited function and
        // be called as a renderer instead of reporting an unknown case.
        const renderer = Object.hasOwn(CASE_VIEWS, state.caseId) ? CASE_VIEWS[state.caseId] : null;
        if (renderer) renderer(view);
        else {
          view.append(errorBox(`No use case with id "${state.caseId}".`));
          view.append(el("p", {}, [el("a", { href: "#/explore", text: "← All use cases" })]));
        }
      } else {
        renderExplore(view);
      }
      break;
    case "docs":
      if (state.docSection === "architecture") renderArchitecture(view);
      else if (state.docSection === "reference") renderReference(view);
      else renderDocsIndex(view);
      break;
    case "inspect":
      renderInspect(view);
      break;
    default:
      renderHome(view);
  }
  view.focus({ preventScroll: true });
  if (!location.hash) window.scrollTo(0, 0);
}

async function boot() {
  parseHash();
  const view = document.getElementById("view");

  try {
    data.useCases = await loadJson("./data/use-cases.json");
  } catch (e) {
    view.replaceChildren(
      errorBox(`Could not load the use-case index: ${e.message}. Run scripts/build_web.sh.`),
    );
    return;
  }

  const entries = Object.entries(data.useCases.artifacts || {});
  const loaded = await Promise.allSettled(entries.map(([, path]) => loadJson(path)));
  entries.forEach(([name], i) => {
    const result = loaded[i];
    // A missing artifact disables the views that need it; it never turns into a
    // silently wrong number, because nothing here has a fallback value.
    data.artifacts[name] = result.status === "fulfilled" ? result.value : null;
  });

  // The generic inspector's bundled sample. Optional: the site works without it.
  try {
    const [context, lineage, verification] = await Promise.all([
      loadJson("./samples/dflow-order/context.json"),
      loadJson("./samples/dflow-order/lineage.json"),
      loadJson("./samples/dflow-order/verification.json"),
    ]);
    Object.assign(data.lineage, { context, lineage, verification, source: "bundled" });
  } catch {
    /* inspector falls back to its empty state */
  }

  window.addEventListener("hashchange", () => {
    parseHash();
    render();
    window.scrollTo({ top: 0, behavior: "instant" });
  });
  render();
}

boot();
