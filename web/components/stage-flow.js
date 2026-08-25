// The five execution stages, rendered provider-neutrally.
//
// This is the explanatory version used on the homepage and the architecture
// page: it describes what each stage *is*, with no artifact loaded. The
// lineage-driven version, which reports what was actually observed, lives in
// `components/stages.js`.

import { el, stageLabel } from "./format.js";

export const STAGES = [
  {
    id: "intent",
    short: "what the caller requested",
    detail:
      "The order as it was asked for: input and output mint, amount, slippage tolerance, and the requesting account where recoverable. Most provider responses echo these back — that echo is the provider's account of the request, not an independent record of it, and the verifier says so.",
    observable: [
      "requested mints and amount",
      "requested slippage tolerance",
      "how the intent was recovered",
    ],
  },
  {
    id: "provider_response",
    short: "quote/order fields and execution promises",
    detail:
      "What the provider returned: quoted output, minimum-output threshold, price impact, fee declaration, execution mode. These are promises and declarations, normalized into provider-neutral names so the core never reads a provider's own field names.",
    observable: [
      "quoted and minimum amounts",
      "declared fees",
      "request/quote identifiers",
      "whether a transaction was returned at all",
    ],
  },
  {
    id: "route",
    short: "provider-selected execution path",
    detail:
      "The venues and legs the provider says it chose. A route is the provider's own account of where the order went — it is not on-chain proof that any of it executed. Where a leg names a market account, that account can be looked for in the transaction.",
    observable: ["venues and leg order", "per-leg amounts", "market keys, when named"],
  },
  {
    id: "transaction_construction",
    short: "what was actually encoded",
    detail:
      "The unsigned transaction, decoded: message version, instructions, program IDs, the exact loaded-account vector including lookup-table entries, compute budget, and index validity. This layer knows nothing about which provider produced the bytes.",
    observable: [
      "message version and topology",
      "programs invoked in the message",
      "loaded accounts in exact order",
      "account-index validity",
    ],
  },
  {
    id: "settlement",
    short: "what landed, when observed",
    detail:
      "Only available when a signature exists and its metadata was fetched: landed status, slot, runtime program set, compute units. A signature on its own is a pointer, not an observation — no settlement input means no settlement claim.",
    observable: [
      "landed status and slot",
      "runtime programs invoked",
      "compute units consumed",
      "token balance deltas — not yet recovered",
    ],
  },
];

export function renderGenericFlow(root, selectedId, onSelect) {
  const selected = STAGES.find((s) => s.id === selectedId) || STAGES[1];

  const flow = el("div", { class: "flow" });
  for (const stage of STAGES) {
    // Everything to the right of this line needs a signature and a fetch.
    // Nothing before it does. That boundary is the shape of the whole tool.
    if (stage.id === "settlement") {
      flow.append(
        el("div", { class: "flow-boundary", "aria-hidden": "true" }, [
          el("span", { class: "flow-boundary-label", text: "observation boundary" }),
        ]),
      );
    }
    flow.append(
      el(
        "button",
        {
          class: "flow-stage stage-observed",
          type: "button",
          "aria-pressed": String(stage.id === selected.id),
          onclick: () => onSelect(stage.id),
        },
        [
          el("span", { class: "flow-name", text: stageLabel(stage.id) }),
          el("span", { class: "flow-headline", text: stage.short }),
        ],
      ),
    );
  }
  root.append(flow);

  root.append(
    el("p", {
      class: "muted",
      style: "font-size:11.5px;margin:2px 0 0",
      text: "Stages left of the boundary are recoverable from a captured response and its transaction bytes. Settlement needs a signature and a fetched result; without both, the verifier reports no settlement claim.",
    }),
  );

  root.append(
    el("div", { class: "card" }, [
      el("h4", { text: stageLabel(selected.id) }),
      el("p", { text: selected.detail }),
      el("h3", { text: "typically observable" }),
      el(
        "ul",
        {},
        selected.observable.map((o) => el("li", { text: o })),
      ),
    ]),
  );
}
