// Minimal execution path — the homepage's only diagram.
//
// Four conceptual stops, not a complete architecture: QUOTE/RESPONSE, ROUTE,
// TRANSACTION, SETTLEMENT. Clicking a stop reveals one sentence. No
// observation-boundary explanation belongs here — that lives in the fuller
// stage explainer under Docs → How it Works (components/stage-flow.js).

import { el } from "./format.js";

const STOPS = [
  {
    id: "quote",
    label: "QUOTE / RESPONSE",
    sentence: "What a provider promised: quoted amount, minimum output, fees.",
  },
  {
    id: "route",
    label: "ROUTE",
    sentence: "The venues the provider says it used to fill the order.",
  },
  {
    id: "transaction",
    label: "TRANSACTION",
    sentence: "What was actually encoded for execution, decoded from the bytes.",
  },
  {
    id: "settlement",
    label: "SETTLEMENT",
    sentence: "What the chain delivered — only shown when it was observed.",
  },
];

export function renderMiniFlow(root, selectedId, onSelect) {
  const selected = STOPS.find((s) => s.id === selectedId) || STOPS[0];

  const list = el("div", { class: "mini-flow" });
  STOPS.forEach((stop, i) => {
    if (i > 0) list.append(el("span", { class: "mini-flow-arrow", "aria-hidden": "true", text: "↓" }));
    list.append(
      el(
        "button",
        {
          class: "mini-flow-stop",
          type: "button",
          "aria-pressed": String(stop.id === selected.id),
          onclick: () => onSelect(stop.id),
        },
        [stop.label],
      ),
    );
  });
  root.append(list);
  root.append(el("p", { class: "mini-flow-sentence", text: selected.sentence }));
}
