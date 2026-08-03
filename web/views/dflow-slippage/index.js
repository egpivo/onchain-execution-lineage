// DFlow slippage-threshold use case: shell, sub-navigation, overview.
//
// This is an interactive reference case, not a copy of any article. It renders
// tracked, Rust-originated evidence and links out to the deeper views.

import { el, link, badge, metric, note, prose, table, details } from "../../components/format.js";
import { artifact, readPath, useCase, state, repoUrl } from "../../app.js";
import { renderThreshold } from "./threshold.js";
import { renderRoute } from "./route.js";
import { renderIdentification } from "./identification.js";
import { renderBytes } from "./bytes.js";
import { renderReproduce } from "./reproduce.js";

const CASE_ID = "dflow-slippage";

/** Provenance of each bundled artifact, stated where it is used. */
export const PROVENANCE = {
  slippage_extract: "Rust — onchain-execution-lineage :: route-bracket",
  slippage_batches: "public projection of the same recorded Rust run",
  causal_model: "authored, frozen, number-free",
};

export function evidence() {
  return {
    extract: artifact("slippage_extract"),
    batches: artifact("slippage_batches"),
    causal: artifact("causal_model"),
  };
}

function header(root, sub) {
  const config = useCase(CASE_ID);
  root.append(
    el("p", { class: "eyebrow" }, [link("#/explore", "Explore"), " / DFlow"]),
  );
  root.append(el("h1", { text: config.title }));
  root.append(el("p", { class: "lede", text: config.question }));
  root.append(
    el("div", { class: "badges" }, [
      ...(config.types || []).map((t) => badge(t, "type")),
      ...(config.providers || []).map((p) => badge(p)),
      badge(config.chain),
      badge(config.surface),
      badge(config.status),
    ]),
  );

  root.append(
    el(
      "nav",
      { class: "subnav" },
      (config.views || []).map((v) =>
        el("a", {
          href: `#/explore/${CASE_ID}${v.id ? `/${v.id}` : ""}`,
          text: v.label,
          "aria-current": (sub || "") === v.id ? "page" : undefined,
        }),
      ),
    ),
  );
}

function overview(root) {
  const { extract, batches } = evidence();
  const config = useCase(CASE_ID);

  if (!extract) {
    root.append(note("The evidence extract is not available in this deployment."));
    return;
  }

  const metrics = el("div", { class: "metrics" });
  for (const m of config.metrics || []) {
    const value = readPath(artifact(m.artifact), m.path);
    if (value !== undefined) metrics.append(metric(m.label, String(value)));
  }
  root.append(metrics);

  root.append(el("h2", { text: "The design" }));
  root.append(
    prose(
      "A bounded, read-only run against DFlow's /order surface: 100 USDC into SOL, in brackets of anchor / treatment / anchor (A1 / T / A2). Brackets whose route topology held still across the whole bracket were marked eligible before any bytes were compared — only those entered the byte search.",
    ),
  );

  root.append(
    details(
      "what each stage observed",
      table(
        ["stage", "what this run observed"],
      [
        ["intent", "slippage assigned by the experiment; mints and amount fixed"],
        ["provider response", `quoted output, threshold and fee for every request`],
        ["route", "venues, leg count, allocation and a frozen route fingerprint"],
        ["transaction", "unsigned v0 transactions, decoded, instructions and programs"],
        ["settlement", "not observed — nothing was signed or submitted"],
      ].map(([stage, what]) =>
        el("tr", {}, [el("td", { class: "mono", text: stage }), el("td", { text: what })]),
        ),
      ),
    ),
  );

  root.append(el("h2", { text: "What it found" }));
  root.append(
    el("div", { class: "grid" }, [
      el("div", { class: "card" }, [
        el("h4", { text: "A response-layer invariant" }),
        el("p", {
          text: `${extract.threshold_identity.exact_matches} of ${extract.threshold_identity.observations} requests satisfy ${extract.threshold_identity.formula}, in ${extract.threshold_identity.arithmetic}.`,
        }),
        el("p", { class: "muted" }, [link(`#/explore/${CASE_ID}/threshold`, "Response invariant →")]),
      ]),
      el("div", { class: "card" }, [
        el("h4", { text: "Route stability decides eligibility" }),
        el("p", {
          text: `${extract.eligible_batch_count} of ${extract.total_batches} brackets kept an exact-stable route across A1, T and A2.`,
        }),
        el("p", { class: "muted" }, [link(`#/explore/${CASE_ID}/route`, "Route stability →")]),
      ]),
      el("div", { class: "card" }, [
        el("h4", { text: "One recurring candidate" }),
        el("p", {
          text: `The quoted output amount occurs verbatim in instruction bytes in every eligible transaction searched. The threshold does not, under the tested encoding family.`,
        }),
        el("p", { class: "muted" }, [link(`#/explore/${CASE_ID}/bytes`, "Byte search →")]),
      ]),
      el("div", { class: "card" }, [
        el("h4", { text: "What it cannot support" }),
        el("p", {
          text: "Route was selected, not intervened on. Eligibility conditions on a variable downstream of both the assigned parameter and the market state.",
        }),
        el("p", { class: "muted" }, [
          link(`#/explore/${CASE_ID}/identification`, "Identification model →"),
        ]),
      ]),
    ]),
  );

  root.append(el("h2", { text: "Limits" }));
  root.append(
    el("ul", { class: "prose" }, [
      el("li", {
        text: "No settlement observation exists. Nothing was signed and nothing was submitted, so no claim about realized output, delivery path or fills is available.",
      }),
      el("li", {
        text: "Byte equality is not semantic decoding. No IDL, no official decoder and no protocol schema was used.",
      }),
      el("li", {
        text: "A quantity not found is not a quantity absent: it may be encoded differently, derived at runtime, or carried in an account rather than a payload.",
      }),
      el("li", {
        text: "The raw provider responses are not published — they carry the requester's wallet pubkey — so public reproduction verifies the evidence snapshot rather than rebuilding it from captures.",
      }),
    ]),
  );

  root.append(
    details(
      "evidence sources",
      [
      table(
      ["artifact", "provenance", "used by"],
      [
        [
          "route_stable_evidence_extract.json",
          PROVENANCE.slippage_extract,
          "overview, response invariant, byte search",
        ],
        [
          "route_stable_batch_evidence.json",
          PROVENANCE.slippage_batches,
          "route stability, bracket detail",
        ],
        ["route_stable_causal_model.json", PROVENANCE.causal_model, "identification model"],
      ].map(([name, prov, used]) =>
        el("tr", {}, [
          el("td", {}, [
            el("a", {
              href: repoUrl(`artifacts/analysis/${name}`),
              text: name,
              class: "mono",
              rel: "noopener",
            }),
          ]),
          el("td", { text: prov }),
          el("td", { class: "muted", text: used }),
        ]),
      ),
      ),
      note(
        `The batch projection ${batches ? "agrees with" : "would be cross-checked against"} the Rust extract on every field they share; a test in the repository enforces that agreement.`,
      ),
      ],
    ),
  );
}

const VIEWS = {
  "": overview,
  threshold: renderThreshold,
  route: renderRoute,
  identification: renderIdentification,
  bytes: renderBytes,
  reproduce: renderReproduce,
};

export function renderDflowSlippage(root) {
  const sub = state.sub || "";
  header(root, sub);
  const view = VIEWS[sub];
  if (!view) {
    root.append(note(`No view "${sub}" in this use case.`));
    return;
  }
  view(root);
}
