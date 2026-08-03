// Lineage-driven stage view: the five stages as they were actually observed
// in one artifact, and the normalized Rust data behind whichever is selected.
//
// The provider-neutral explainer used on the homepage is in `stage-flow.js`.
//
// The stage *state* shown here is read from what Rust serialized — a stage is
// "observed" because the context has it, "not applicable" because the adapter
// said the surface has none, "unknown" because the evidence was not recovered.
// This file classifies presence; it never classifies evidence.

import {
  el,
  stageBadge,
  definitionList,
  chips,
  groupDigits,
  shorten,
  stageLabel,
  note,
  table,
  addr,
  hash,
} from "./format.js";

const STAGES = ["intent", "provider_response", "route", "transaction_construction", "settlement"];

/**
 * Presence classification, from the serialized context alone.
 *
 * `not_applicable` is reserved for the case Rust makes explicit: the provider
 * surface returned no transaction. Everything else absent is `unknown`,
 * because "not observed" is not "does not exist".
 */
export function stageState(context, stage) {
  const present = (context.stages_present || []).includes(stage);
  if (present) return "observed";

  if (stage === "transaction_construction") {
    const ref = context.provider_extraction?.transaction;
    if (ref && ref.present === false) return "not_applicable";
    if (ref && ref.present === true) return "unknown";
  }
  if (stage === "settlement") {
    const s = context.settlement;
    if (!s) return "not_applicable";
    if (s.signature && !s.applicable) return "unknown";
  }
  return "unknown";
}

/**
 * `stages_present` is not serialized on the context; derive it for display.
 * Exported so callers that need the normalized context outside this module's
 * own rendering (e.g. a stage checklist header) do not re-derive it.
 */
export function normalizeContext(context) {
  const present = [];
  if (context.intent) present.push("intent");
  if (context.provider_response) present.push("provider_response");
  if (context.route) present.push("route");
  if (context.transaction) present.push("transaction_construction");
  if (context.settlement?.applicable) present.push("settlement");
  return { ...context, stages_present: context.stages_present || present };
}

function headline(context, stage) {
  const r = context.provider_response || {};
  switch (stage) {
    case "intent":
      return context.intent
        ? `${shorten(context.intent.input_mint)} → ${shorten(context.intent.output_mint)}`
        : "not recovered";
    case "provider_response":
      return r.out_amount ? `out ${groupDigits(r.out_amount)}` : "no quoted amount";
    case "route": {
      const legs = context.route?.legs || [];
      return legs.length ? `${legs.length} leg: ${legs.map((l) => l.venue_or_label).join(" → ")}` : "no route";
    }
    case "transaction_construction": {
      const t = context.transaction;
      return t ? `${t.version} · ${t.topology.num_instructions} instructions` : "no transaction decoded";
    }
    case "settlement":
      return context.settlement?.applicable ? context.settlement.status || "landed" : "no settlement evidence";
    default:
      return "";
  }
}

function stageDetail(context, stage) {
  const wrap = el("div", { class: "card" });
  wrap.append(el("h2", { text: stageLabel(stage) }));

  const provenance = (context.provenance?.stages || []).filter((p) => p.stage === stage);

  if (stage === "intent") {
    const i = context.intent;
    if (!i) return wrap.append(note("No execution intent was recoverable from this artifact.")), wrap;
    wrap.append(
      definitionList([
        ["input mint", addr(i.input_mint)],
        ["output mint", addr(i.output_mint)],
        ["in amount", groupDigits(i.in_amount)],
        ["slippage bps", i.slippage_bps],
        ["recovered from", i.recovered_from],
      ]),
    );
    if (i.recovered_from === "provider_response_echo") {
      wrap.append(
        note(
          "The intent was read back out of the provider's own response. It is the provider's account of the request, not an independent record of it.",
        ),
      );
    }
  }

  if (stage === "provider_response") {
    const r = context.provider_response || {};
    wrap.append(
      definitionList([
        ["provider", context.provider],
        ["surface", context.provenance?.surface],
        ["input mint", addr(r.input_mint)],
        ["output mint", addr(r.output_mint)],
        ["in amount", groupDigits(r.in_amount)],
        ["out amount", groupDigits(r.out_amount)],
        ["min out amount", groupDigits(r.min_out_amount)],
        ["other amount threshold", groupDigits(r.other_amount_threshold)],
        ["slippage bps", r.slippage_bps],
        ["execution mode", r.execution_mode],
        ["request / quote id", r.request_or_quote_id],
        ["platform fee", r.platform_fee ? r.platform_fee.visible : undefined],
      ]),
    );
  }

  if (stage === "route") {
    const legs = context.route?.legs || [];
    if (!legs.length) return wrap.append(note("The artifact carries no route observation.")), wrap;
    wrap.append(
      definitionList([["provider route label", context.route.provider_route_label || "none declared"]]),
    );
    for (const [i, leg] of legs.entries()) {
      wrap.append(el("h3", { text: `leg ${i}` }));
      wrap.append(
        definitionList([
          ["venue", leg.venue_or_label],
          ["market key", addr(leg.market_key)],
          ["input mint", addr(leg.input_mint)],
          ["output mint", addr(leg.output_mint)],
          ["in amount", groupDigits(leg.in_amount)],
          ["out amount", groupDigits(leg.out_amount)],
        ]),
      );
    }
    wrap.append(
      note("A route is the provider's own account of where the order went. It is not on-chain proof of execution."),
    );
  }

  if (stage === "transaction_construction") {
    const t = context.transaction;
    if (!t) {
      const ref = context.provider_extraction?.transaction;
      wrap.append(
        note(
          ref && ref.present === false
            ? "This provider surface returned no transaction. Absence is a property of the surface, not a failure."
            : "No transaction was decoded in this run.",
        ),
      );
      return wrap;
    }
    wrap.append(
      definitionList([
        ["message version", t.version],
        ["transaction sha256", hash(t.transaction_sha256)],
        ["instructions", t.topology.num_instructions],
        ["static keys", t.topology.num_static_keys],
        ["lookup tables", t.topology.num_lookup_tables],
        ["ALT-loaded accounts", t.topology.num_alt_loaded_accounts],
        ["account vector length", t.topology.account_vector_len],
        ["fee payer", addr(t.signers?.fee_payer)],
        ["signature slots", t.signers?.signature_slots],
        ["all slots empty (unsigned)", String(t.signers?.all_signature_slots_empty)],
        ["compute budget", t.compute_budget?.present ? "present" : "absent"],
        ["account indexes in range", String(t.account_index_validity?.all_indexes_in_range)],
      ]),
    );

    wrap.append(el("h3", { text: "programs" }));
    wrap.append(chips(t.topology.program_labels || []));
    if ((t.topology.unknown_program_ids || []).length) {
      wrap.append(el("h3", { text: "not in the verified registry" }));
      wrap.append(chips(t.topology.unknown_program_ids));
    }

    const alt = t.alt_resolution || {};
    if ((alt.tables_referenced || []).length) {
      wrap.append(el("h3", { text: "address lookup tables" }));
      wrap.append(
        definitionList([
          ["referenced", (alt.tables_referenced || []).length],
          ["resolved", (alt.tables_resolved || []).length],
          ["resolution attempted", String(alt.attempted)],
        ]),
      );
      if (!alt.complete) {
        wrap.append(
          note(
            "The account vector is incomplete: at least one lookup table was not resolved. Absence of an address from this transaction cannot be claimed.",
          ),
        );
      }
    }

    wrap.append(
      note(
        "An unsigned transaction shows what was constructed. It does not show that anyone signed it, submitted it, or that it landed.",
      ),
    );
  }

  if (stage === "settlement") {
    const s = context.settlement;
    if (!s || !s.applicable) {
      wrap.append(
        note(
          s?.signature
            ? "A signature was supplied but no settlement metadata was fetched. A signature is a pointer, not an observation — no settlement claim is available."
            : "No settlement evidence exists for this artifact. No settlement input means no settlement claim.",
        ),
      );
      return wrap;
    }
    wrap.append(
      definitionList([
        ["signature", hash(s.signature)],
        ["status", s.status],
        ["slot", s.slot],
        ["compute units", s.compute_units_consumed],
        ["runtime programs", (s.runtime_program_set || []).length],
      ]),
    );
  }

  if (provenance.length) {
    wrap.append(el("h3", { text: "provenance" }));
    for (const p of provenance) {
      wrap.append(
        definitionList([
          ["source", p.source],
          ["path", p.source_path],
          ["sha256", hash(p.sha256)],
        ]),
      );
    }
  }
  return wrap;
}

export function renderLineage(root, bundleSet, state, onSelect) {
  const context = normalizeContext(bundleSet.context || {});
  root.append(el("h2", { text: "Execution" }));
  root.append(
    el("p", { class: "lede" }, [
      "Select a stage to see the normalized data and where it came from. ",
      el("strong", { text: "A missing stage means not observed, never observed-empty." }),
    ]),
  );

  const pipeline = el("div", { class: "flow" });
  for (const stage of STAGES) {
    const st = stageState(context, stage);
    const selected = state.stage === stage;
    pipeline.append(
      el(
        "button",
        {
          class: `flow-stage stage-${st}`,
          "aria-pressed": String(selected),
          type: "button",
          onclick: () => onSelect(stage),
        },
        [
          el("span", { class: "flow-name", text: stageLabel(stage) }),
          el("span", { class: "flow-headline", text: headline(context, stage) }),
          stageBadge(st),
        ],
      ),
    );
  }
  root.append(pipeline);
  root.append(stageDetail(context, state.stage || "provider_response"));

  const unresolved = bundleSet.lineage?.unresolved || [];
  if (unresolved.length) {
    root.append(el("h3", { text: "where the verifier stops" }));
    root.append(
      table(
        ["field", "reason"],
        unresolved.map((u) =>
          el("tr", {}, [el("td", { class: "mono", text: u.field }), el("td", { text: u.reason })]),
        ),
      ),
    );
  }
}
