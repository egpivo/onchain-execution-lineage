// DFlow platform-fee accounting — a second, smaller use case.
//
// Deliberately its own page: it is a different experiment on a different
// surface, and folding it into the slippage case would mix unrelated runs
// merely because an older evidence lab did so.

import { el, note, prose, table, badge, metric, link, groupDigits } from "../components/format.js";
import { artifact, readPath, useCase, repoUrl } from "../app.js";

export function renderDflowFee(root) {
  const config = useCase("dflow-platform-fee");
  const fee = artifact("fee_quote");

  root.append(el("p", { class: "eyebrow" }, [link("#/explore", "Explore"), " / DFlow"]));
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

  if (!fee) {
    root.append(note("The fee evidence artifact is not available in this deployment."));
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
      `${fee.design.requests} requests against ${fee.design.provider}, ${fee.design.pair}, with the platform fee varied across the declared treatments. Read-only throughout.`,
    ),
  );

  root.append(el("h2", { text: "What the responses showed" }));
  root.append(
    table(
      ["fee (bps)", "gross out", "platform fee", "net out", "mode", "threshold"],
      (fee.treatments || []).map((t) =>
        el("tr", {}, [
          el("td", { class: "mono", text: String(t.fee_bps) }),
          el("td", { class: "mono", text: groupDigits(t.gross_out_amount) }),
          el("td", { class: "mono", text: groupDigits(t.platform_fee_amount) }),
          el("td", { class: "mono", text: groupDigits(t.net_out_amount) }),
          el("td", { class: "mono", text: t.fee_mode }),
          el("td", { class: "mono", text: groupDigits(t.other_amount_threshold) }),
        ]),
      ),
    ),
  );

  root.append(el("h2", { text: "Where it stops" }));
  root.append(
    el("p", { class: "note warn" }, [
      el("strong", { text: "No transaction was returned. " }),
      fee.evidence_ceiling.note,
    ]),
  );
  root.append(
    prose(
      "That is the whole finding, and it is a real one: this surface is quote-only, so there is nothing downstream of the response to check. No route observation, no transaction construction, no settlement.",
    ),
  );

  root.append(
    el("p", { class: "prose" }, [
      "Evidence: ",
      el("a", {
        href: repoUrl("artifacts/analysis/fee_quote_evidence.json"),
        text: "fee_quote_evidence.json",
        class: "mono",
        rel: "noopener",
      }),
      " · ",
      link("#/explore/dflow-slippage", "the slippage use case →"),
    ]),
  );
}
