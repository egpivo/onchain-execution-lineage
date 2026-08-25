// Byte search: three response quantities against the transaction.
//
// The search was performed in Rust over every instruction payload of every
// eligible transaction. This view reads the results and lays them out. It never
// searches, decodes, or names a field.

import {
  el,
  note,
  prose,
  table,
  groupDigits,
  chips,
  statusBadge,
  numCell,
  numHeader,
  details,
} from "../../components/format.js";
import { evidence } from "./index.js";
import { state, render } from "../../app.js";

/** Display labels for the searched quantities, in the order Rust searched them. */
const QUANTITIES = [
  { key: "otherAmountThreshold", symbol: "M", caption: "minimum output the order carries" },
  { key: "outAmount", symbol: "Q", caption: "quoted output amount" },
  { key: "outAmount_minus_threshold", symbol: "Q − M", caption: "the slippage allowance itself" },
];

function sitesFor(request, quantity) {
  const entry = (request.sites || []).find(([name]) => name === quantity);
  return entry ? entry[1] : [];
}

function encodingsFor(request, quantity) {
  const entry = (request.encodings_hit || []).find(([name]) => name === quantity);
  return entry ? entry[1] : [];
}

function siteLabel(sites) {
  if (!sites.length) return "no match under tested family";
  return sites.map((s) => `instruction ${s.instruction_index} / offset ${s.byte_offset}`).join(", ");
}

/** Aggregate across the whole eligible set, for the summary map. */
function summarize(extract, quantityKey) {
  const searches = extract.per_request_search || [];
  const withHits = searches.filter((r) => sitesFor(r, quantityKey).length > 0);
  const sites = new Set();
  const encodings = new Set();
  for (const r of searches) {
    for (const s of sitesFor(r, quantityKey)) sites.add(`${s.instruction_index}:${s.byte_offset}`);
    for (const e of encodingsFor(r, quantityKey)) encodings.add(e);
  }
  return {
    searched: searches.length,
    matched: withHits.length,
    unique: searches.every((r) => sitesFor(r, quantityKey).length <= 1),
    sites: [...sites],
    encodings: [...encodings],
  };
}

function relationshipRow(extract, quantity, selected, onSelect) {
  const summary = summarize(extract, quantity.key);
  const hit = summary.matched > 0;
  const relClass = hit ? "rel-candidate" : "rel-no_match";
  const relLabel = hit ? "◈ candidate relationship" : "? literal search";

  return el(
    "button",
    {
      class: "relmap",
      type: "button",
      "aria-pressed": String(selected === quantity.key),
      onclick: () => onSelect(quantity.key),
    },
    [
      el("div", {}, [
        el("div", { class: "quantity", text: quantity.symbol }),
        el("div", { class: "quantity-sub", text: quantity.key }),
        el("div", { class: "quantity-sub", text: quantity.caption }),
      ]),
      el("div", { class: "relmap-mid" }, [
        el("span", { class: `rel ${relClass}`, text: relLabel }),
        el("div", { class: "rel-line", text: "└──────────────→" }),
        el("div", { class: "rel-line", text: `${summary.matched}/${summary.searched} transactions` }),
      ]),
      el("div", {}, [
        hit
          ? el(
              "div",
              { class: "site" },
              summary.sites.map((s) => {
                const [instruction, offset] = s.split(":");
                return el("span", { class: "site-pair" }, [
                  el("span", { class: "site-part" }, [
                    el("span", { class: "site-key", text: "instruction " }),
                    el("span", { class: "site-val", text: instruction }),
                  ]),
                  el("span", { class: "site-part" }, [
                    el("span", { class: "site-key", text: "offset " }),
                    el("span", { class: "site-val", text: offset }),
                  ]),
                ]);
              }),
            )
          : el("div", { class: "mono", text: "no match" }),
        el("div", {
          class: "quantity-sub",
          text: hit ? "same site in every transaction searched" : "under the tested encoding family",
        }),
      ]),
    ],
  );
}

function detail(extract, quantityKey) {
  const quantity = QUANTITIES.find((q) => q.key === quantityKey);
  const summary = summarize(extract, quantityKey);
  const candidate = extract.candidate_result;
  const hit = summary.matched > 0;

  const card = el("div", { class: "card" });
  card.append(
    el("div", { class: "case-head" }, [
      el("span", { class: "case-title", text: `${quantity.symbol} — ${quantity.key}` }),
      statusBadge(hit ? "CANDIDATE" : "UNKNOWN"),
    ]),
  );

  const rows = [
    ["observed scope", `${summary.searched} eligible transactions, every instruction payload`],
    ["transactions matched", `${summary.matched}/${summary.searched}`],
    ["uniqueness", hit ? (summary.unique ? "one site per transaction" : "multiple sites") : "—"],
    ["instruction index", hit ? summary.sites.map((s) => s.split(":")[0]).join(", ") : "—"],
    ["byte offset", hit ? summary.sites.map((s) => s.split(":")[1]).join(", ") : "—"],
    ["encodings hit", hit ? summary.encodings.join(", ") : "none"],
    ["canonical encoding", hit ? candidate.canonical_encoding : "—"],
    ["evidence source", "route_stable_evidence_extract.json · per_request_search"],
  ];
  const dl = el("dl", { class: "kv" });
  for (const [term, value] of rows) {
    dl.append(el("dt", { text: term }), el("dd", { class: "mono", text: value }));
  }
  card.append(dl);

  if (hit) {
    card.append(el("p", { class: "prose", text: candidate.canonical_note }));
  }
  card.append(el("p", { class: "ceiling", text: candidate.evidence_ceiling }));
  return card;
}

export function renderBytes(root) {
  const { extract } = evidence();
  if (!extract) {
    root.append(note("The evidence extract is not available in this deployment."));
    return;
  }
  const candidate = extract.candidate_result;

  root.append(el("h2", { text: "Byte search" }));
  root.append(
    prose(
      "Three response-layer quantities were searched for, verbatim, inside every instruction payload of every eligible transaction. The question was narrow on purpose: does this integer occur in these bytes, under a declared family of encodings?",
    ),
  );

  root.append(
    el("div", { class: "controls" }, [
      el("span", { class: "chip", text: "response" }),
      el("span", { class: "muted", text: "→" }),
      el("span", { class: "chip", text: "transaction" }),
    ]),
  );

  const selected = state.quantity && QUANTITIES.some((q) => q.key === state.quantity)
    ? state.quantity
    : "outAmount";

  for (const quantity of QUANTITIES) {
    root.append(
      relationshipRow(extract, quantity, selected, (key) => {
        state.quantity = key;
        render();
      }),
    );
  }

  root.append(detail(extract, selected));

  root.append(
    el("p", { class: "note warn" }, [
      el("strong", { text: "Byte equality is not semantic decoding. " }),
      "A match shows that the integer's encoding occurs at that position. It does not show that the program reads those bytes as that quantity, and the offset is not to be described as a named field. No IDL, no official decoder and no protocol schema was used.",
    ]),
  );
  root.append(
    note(
      "An empty result is equally narrow: not recovered under this search is not absent from the transaction. The quantity may be encoded differently, derived at runtime, or carried in an account rather than a payload.",
    ),
  );

  // Deep-linkable bracket filter: #/explore/dflow-slippage/bytes?batch=1
  const searches = extract.per_request_search || [];
  const brackets = [...new Set(searches.map((r) => r.batch_index))].sort((a, b) => a - b);
  const requested = Number(state.params.get("batch"));
  const filter = brackets.includes(requested) ? requested : null;
  const shown = filter === null ? searches : searches.filter((r) => r.batch_index === filter);

  const filters = el("div", { class: "controls" }, [el("span", { class: "chip", text: "bracket" })]);
  filters.append(
    el("a", {
      class: "control",
      href: "#/explore/dflow-slippage/bytes",
      text: "all",
      "aria-current": filter === null ? "page" : undefined,
    }),
  );
  for (const b of brackets) {
    filters.append(
      el("a", {
        class: "control",
        href: `#/explore/dflow-slippage/bytes?batch=${b}`,
        text: String(b),
        "aria-current": filter === b ? "page" : undefined,
      }),
    );
  }
  root.append(
    details(
      `per request — ${searches.length} searches`,
      [
        filters,
        table(
          [
            numHeader("bracket"),
            "role",
            numHeader("S"),
            numHeader("Q"),
            numHeader("M"),
            ...QUANTITIES.map((q) => `${q.symbol} sites`),
          ],
          shown.map((request) =>
            el("tr", {}, [
              numCell(request.batch_index),
              el("td", { class: "mono", text: request.role }),
              numCell(request.slippage_bps),
              numCell(groupDigits(request.out_amount)),
              numCell(groupDigits(request.other_amount_threshold)),
              ...QUANTITIES.map((q) =>
                el("td", { class: "mono", text: siteLabel(sitesFor(request, q.key)) }),
              ),
            ]),
          ),
        ),
        el("h3", { text: "encoding family searched" }),
        chips(candidate.encoding_family || []),
      ],
      // A ?batch= deep link targets this table; do not hide what it points at.
      { open: filter !== null },
    ),
  );
}
