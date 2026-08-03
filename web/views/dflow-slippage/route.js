// Route stability and eligibility, per bracket.
//
// Route classes, fingerprints, eligibility and the ineligibility reasons are
// all read from the tracked batch evidence. Nothing here compares routes.

import {
  el,
  note,
  prose,
  table,
  groupDigits,
  badge,
  venue,
  fingerprint,
  numCell,
  numHeader,
  details,
} from "../../components/format.js";
import { evidence } from "./index.js";
import { state, render } from "../../app.js";

/**
 * Presentation of each route class the evidence can carry. Glyphs accompany the
 * wording so the classes are distinguishable without colour.
 */
const CLASS_DISPLAY = {
  exact_route_stable: { glyph: "=", label: "exact stable" },
  allocation_drift: { glyph: "≈", label: "allocation drift" },
  same_venue_different_market: { glyph: "≃", label: "same venue / different market key" },
  fully_different_route: { glyph: "≠", label: "fully different" },
};

function classBadge(value) {
  const spec = CLASS_DISPLAY[value] || { glyph: "·", label: value || "unclassified" };
  return el("span", { class: `rel rel-${value === "exact_route_stable" ? "same_value" : "no_match"}` }, [
    el("span", { "aria-hidden": "true", text: `${spec.glyph} ` }),
    el("span", { text: spec.label }),
  ]);
}

function selector(batches, current) {
  const wrap = el("div", { class: "batches" });
  for (const batch of batches) {
    const eligible = batch.eligible_for_instruction_diff;
    wrap.append(
      el(
        "a",
        {
          class: `batch ${eligible ? "batch-eligible" : "batch-rejected"}`,
          href: `#/explore/dflow-slippage/route?batch=${batch.batch_index}`,
          "aria-current": batch.batch_index === current ? "page" : undefined,
          title: eligible ? "eligible" : "rejected",
        },
        [
          String(batch.batch_index).padStart(2, "0"),
          el("small", { text: eligible ? "✓ elig" : "✕ rej", "aria-hidden": "true" }),
        ],
      ),
    );
  }
  return wrap;
}

function bracketStrip(batch) {
  const strip = el("div", { class: "bracket" });
  for (const request of batch.requests) {
    const isTreatment = request.role === "T";
    strip.append(
      el("div", { class: `leg ${isTreatment ? "leg-treatment" : "leg-anchor"}` }, [
        el("div", { class: "leg-role", text: `${request.role} · ${request.slippage_bps} bps` }),
        el("div", { class: "leg-venue" }, [venue((request.route?.venues || []).join(" → "))]),
        el("div", { class: "leg-fp", text: request.route?.fingerprint_short || "—" }),
        el("div", { class: "muted", style: "font-size:11.5px;margin-top:8px" }, [
          el("span", { class: "mono", text: "Q " }),
          el("span", { class: "mono", text: groupDigits(request.out_amount) }),
        ]),
      ]),
    );
  }
  return strip;
}

export function renderRoute(root) {
  const { batches: projection, extract } = evidence();
  if (!projection) {
    root.append(note("The batch evidence projection is not available in this deployment."));
    return;
  }
  const batches = projection.batches || [];
  const requested = Number(state.params.get("batch"));
  const current = batches.some((b) => b.batch_index === requested)
    ? requested
    : batches[0]?.batch_index;
  const batch = batches.find((b) => b.batch_index === current);

  root.append(el("h2", { text: "Route stability" }));
  root.append(
    prose(
      "A bracket is eligible only if its route topology held still across all three requests — a rule frozen before any bytes were compared. Matching anchors are a drift sentinel; they say nothing about the treatment request between them.",
    ),
  );

  root.append(selector(batches, current));
  root.append(
    el("p", { class: "muted", style: "font-size:12.5px" }, [
      "✓ elig — carried into the byte search   ·   ✕ rej — excluded, with reasons recorded",
    ]),
  );

  if (!batch) {
    root.append(note("No batch selected."));
    return;
  }

  root.append(el("h3", { text: `bracket ${String(batch.batch_index).padStart(2, "0")} · ${batch.pattern}` }));
  root.append(bracketStrip(batch));

  root.append(
    table(
      ["comparison", "route class"],
      [
        ["A1 → T", batch.route_class_a1_t],
        ["T → A2", batch.route_class_t_a2],
        ["A1 → A2 (anchor control)", batch.route_class_a1_a2],
      ].map(([label, value]) =>
        el("tr", {}, [el("td", { text: label }), el("td", {}, [classBadge(value)])]),
      ),
    ),
  );

  const verdict = batch.eligible_for_instruction_diff ? "Eligible" : "Rejected";
  root.append(
    el("div", { class: "card" }, [
      el("h4", {}, []),
      el("div", { class: "badges" }, [
        badge(`Result: ${verdict}`, batch.eligible_for_instruction_diff ? "type" : undefined),
        badge(`exact route stable: ${batch.exact_route_stable}`),
        badge(`topology A1–T: ${batch.topology_stable_a1_t}`),
        badge(`topology T–A2: ${batch.topology_stable_t_a2}`),
      ]),
      ...(batch.ineligibility_reasons || []).length
        ? [
            el("h3", { text: "why it was rejected" }),
            el(
              "ul",
              { class: "prose" },
              (batch.ineligibility_reasons || []).map((r) => el("li", { text: r })),
            ),
          ]
        : [],
    ]),
  );

  if (batch.route_class_a1_a2 === "exact_route_stable" && !batch.eligible_for_instruction_diff) {
    root.append(
      el("p", { class: "note warn" }, [
        el("strong", { text: "Matching anchors, rejected bracket. " }),
        "A1 and A2 returned the same route and the same fingerprint, so the sentinel looks clean — and the treatment request in between still routed somewhere else. Anchor agreement is useful for detecting drift; it does not establish that the treatment route remained stable.",
      ]),
    );
  }

  const control = (extract?.anchor_control || []).find((p) => p.batch_index === batch.batch_index);
  if (control) {
    const controlTable = table(
      ["observation", "value"],
      [
        ["A1 quote", groupDigits(control.q_a1)],
        ["A2 quote", groupDigits(control.q_a2)],
        ["quote differs between anchors", String(control.quote_differs)],
        ["candidate site carries each response's own quote", String(control.candidate_carries_own_quote)],
        ["route same", String(control.route_same)],
        ["topology same", String(control.topology_same)],
      ].map(([k, v]) =>
        el("tr", {}, [el("td", { text: k }), el("td", { class: "mono", text: v })]),
      ),
    );
    root.append(
      details("same-treatment control — A1 vs A2", [
        controlTable,
        note(
          "Both anchors used the same slippage setting, so the contrast between them is zero. The quote still moved — which is why a difference across the treatment cannot be read as an effect of the treatment.",
        ),
      ]),
    );
  }

  root.append(
    el("p", { class: "note" }, [
      el("strong", { text: "Not attributable to slippage. " }),
      "A route difference across the treatment is a difference in a variable that sits downstream of both the assigned parameter and the contemporaneous market state. This view records what the router returned; it does not assign a cause. See ",
      el("a", { href: "#/explore/dflow-slippage/identification", text: "Identification" }),
      ".",
    ]),
  );

  root.append(
    details(
      `all brackets — ${batches.length}`,
      table(
        [numHeader("bracket"), "pattern", "A1→T", "T→A2", "A1→A2", "eligible"],
        batches.map((b) =>
          el(
            "tr",
            {
              onclick: () => {
                state.params = new URLSearchParams(`batch=${b.batch_index}`);
                location.hash = `#/explore/dflow-slippage/route?batch=${b.batch_index}`;
                render();
              },
              style: "cursor:pointer",
            },
            [
              numCell(String(b.batch_index).padStart(2, "0")),
              el("td", { class: "mono", text: b.pattern }),
              el("td", {}, [classBadge(b.route_class_a1_t)]),
              el("td", {}, [classBadge(b.route_class_t_a2)]),
              el("td", {}, [classBadge(b.route_class_a1_a2)]),
              el("td", { class: "mono", text: b.eligible_for_instruction_diff ? "✓" : "✕" }),
            ],
          ),
        ),
      ),
    ),
  );
}
