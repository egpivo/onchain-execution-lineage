// Response invariant: S, Q, M.
//
// The published counts come from the Rust evidence extract. The interactive
// panel steps through observations the verifier already evaluated; it does not
// recompute the identity, so no published claim depends on browser arithmetic.

import {
  el,
  note,
  prose,
  table,
  groupDigits,
  badge,
  statusBadge,
  numCell,
  numHeader,
  details,
} from "../../components/format.js";
import { evidence } from "./index.js";
import { state, render } from "../../app.js";

/**
 * Interactive arithmetic illustration.
 *
 * It does not compute the identity. Recomputing it here would put a second
 * implementation of a published claim in the browser, which is the exact drift
 * this project exists to prevent. Instead it steps through the observations the
 * verifier already evaluated: pick a slippage setting that actually occurred,
 * and see the quoted amount, the threshold the provider returned, and the value
 * Rust predicted — side by side.
 */
function illustration(extract, selectedBps, onSelect) {
  const rows = extract.threshold_identity.detail;
  const settings = [...new Set(rows.map((r) => r.slippage_bps))].sort((a, b) => a - b);
  const bps = settings.includes(selectedBps) ? selectedBps : settings[0];
  const example = rows.find((r) => r.slippage_bps === bps);

  const wrap = el("div", { class: "card" });
  wrap.append(el("h4", { text: "Interactive arithmetic illustration" }));
  wrap.append(
    el("p", {
      class: "muted",
      text: "Not a source of evidence, and not a recalculation: these are observations the verifier already evaluated. Pick a slippage setting that occurred in the run.",
    }),
  );

  const controls = el("div", { class: "controls" }, [el("span", { class: "chip", text: "S" })]);
  for (const setting of settings) {
    controls.append(
      el("button", {
        class: "control",
        type: "button",
        text: `${setting} bps`,
        "aria-pressed": String(setting === bps),
        onclick: () => onSelect(setting),
      }),
    );
  }
  wrap.append(controls);

  wrap.append(
    el("pre", {
      text: [
        `S = ${example.slippage_bps} bps        (assigned on the request)`,
        `Q = ${groupDigits(example.out_amount)}   (quoted output)`,
        `M = ${groupDigits(example.other_amount_threshold)}   (threshold the provider returned)`,
        "",
        `predicted = ${groupDigits(example.predicted_threshold)}   (${extract.threshold_identity.formula})`,
        `match      = ${example.exact_match ? "exact" : "differs"}`,
      ].join("\n"),
    }),
  );
  wrap.append(
    el("p", {
      class: "muted",
      text: `bracket ${example.batch_index}, role ${example.role} — one of ${rows.length} observations`,
    }),
  );
  return wrap;
}

export function renderThreshold(root) {
  const { extract } = evidence();
  if (!extract) {
    root.append(note("The evidence extract is not available in this deployment."));
    return;
  }
  const identity = extract.threshold_identity;

  root.append(el("h2", { text: "Response invariant" }));
  root.append(
    prose(
      "Three quantities travel together on every order response. The relationship between them is exact, and it is arithmetic the provider performs — not something the transaction was checked against.",
    ),
  );

  root.append(
    table(
      ["symbol", "field", "meaning"],
      [
        ["S", "slippageBps", "the slippage tolerance assigned on the request"],
        ["Q", "outAmount", "the quoted output amount, in token base units"],
        ["M", "otherAmountThreshold", "the minimum output carried on the order"],
      ].map(([sym, field, meaning]) =>
        el("tr", {}, [
          el("td", { class: "mono", text: sym }),
          el("td", { class: "mono", text: field }),
          el("td", { text: meaning }),
        ]),
      ),
    ),
  );

  root.append(el("h3", { text: "the relationship Rust verified" }));
  root.append(el("pre", { text: `${identity.formula}\n${identity.arithmetic}` }));

  // Counts, straight from the extract. `basis` is the short token; the note
  // beside it says what that token buys you.
  root.append(
    table(
      ["result", numHeader("count"), "basis", "status"],
      [
        [
          "ceil identity holds",
          `${identity.exact_matches}/${identity.observations}`,
          "recomputed",
          "re-derived by the verifier from the published inputs",
          "PASS",
        ],
        [
          "floor variant holds",
          `${identity.floor_variant_matches}/${identity.observations}`,
          "recomputed",
          "the alternative rounding convention, evaluated the same way",
          identity.floor_variant_matches === 0 ? "PASS" : "CANDIDATE",
        ],
        [
          "minOutAmount equals threshold",
          String(identity.min_out_equals_threshold_all),
          "attested",
          "the source field is not published per request; only a local rebuild can confirm it",
          "PASS",
        ],
      ].map(([label, count, basisToken, basisNote, status]) =>
        el("tr", {}, [
          el("td", { class: "text", text: label }),
          numCell(count),
          el("td", { class: "text" }, [
            badge(basisToken),
            el("p", { class: "muted", style: "margin:5px 0 0", text: basisNote }),
          ]),
          el("td", {}, [statusBadge(status)]),
        ]),
      ),
    ),
  );
  root.append(
    note(
      "Basis matters: recomputed means the verifier re-derived the value from published inputs with its own implementation; attested means the inputs needed to re-derive it are not published, and only a local rebuild from the private captures can confirm it.",
    ),
  );

  root.append(
    illustration(extract, state.illustrationBps, (bps) => {
      state.illustrationBps = bps;
      render();
    }),
  );

  root.append(
    details(
      `every request — ${identity.detail.length} observations`,
      table(
      [
        numHeader("bracket"),
        "role",
        numHeader("S · bps"),
        numHeader("Q · quoted out"),
        numHeader("M · threshold"),
        numHeader("predicted"),
        "match",
      ],
      identity.detail.map((row) =>
        el("tr", {}, [
          numCell(row.batch_index),
          el("td", { class: "mono", text: row.role }),
          numCell(row.slippage_bps),
          numCell(groupDigits(row.out_amount)),
          numCell(groupDigits(row.other_amount_threshold)),
          numCell(groupDigits(row.predicted_threshold)),
          el("td", { class: "mono", text: row.exact_match ? "exact" : "differs" }),
        ]),
      ),
      ),
    ),
  );

  root.append(
    el("p", { class: "note warn" }, [
      el("strong", { text: "Ceiling: " }),
      "this is response-level arithmetic. It shows the response is internally consistent. It says nothing about what the transaction enforces on chain, and the threshold was never located in the transaction bytes.",
    ]),
  );
}
