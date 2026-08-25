// Reproduce: the two-mode model, stated without overclaiming.

import { el, note, prose, table, ascii, badge, details } from "../../components/format.js";
import { evidence } from "./index.js";
import { repoUrl } from "../../app.js";

export function renderReproduce(root) {
  const { extract } = evidence();

  root.append(el("h2", { text: "Reproduce" }));
  root.append(
    prose(
      "Two modes, and the difference between them is not cosmetic. One works on a clean clone; the other needs the original captures, which are not published.",
    ),
  );

  root.append(el("h3", { text: "Public verification" }));
  root.append(el("pre", { text: "./scripts/reproduce_slippage_article.sh" }));
  root.append(
    ascii(`tracked Rust evidence snapshot
        ▼
   schema + required fields
        ▼
   threshold identity recomputed from published inputs
        ▼
   every summary re-aggregated from published detail
        ▼
   claim table`),
  );
  root.append(
    prose(
      "Works on a clean clone with no private data and no network. It re-derives the arithmetic with the verifier's own implementation, and re-aggregates each published summary from the snapshot's per-request detail — so a summary that drifted from its own evidence fails.",
    ),
  );
  root.append(
    el("div", { class: "badges" }, [
      badge("recomputed — re-derived from published inputs"),
      badge("cross-checked — summary re-aggregated from detail"),
      badge("attested — inputs not published; needs local rebuild"),
    ]),
  );
  root.append(
    el("p", { class: "note warn" }, [
      el("strong", { text: "Public verification is not a raw-data reproduction. " }),
      "It verifies the published evidence snapshot. It does not rebuild the recorded provider responses, because those are not published: they carry the requester's wallet pubkey.",
    ]),
  );

  root.append(el("h3", { text: "Local full rebuild" }));
  root.append(el("pre", { text: "./scripts/reproduce_slippage_article.sh --from-recorded-run" }));
  root.append(
    ascii(`private recorded raw run
        ▼
   Rust analysis
        ▼
   regenerated evidence extract
        ▼
   field-by-field comparison against the tracked publication extract`),
  );
  root.append(
    prose(
      "Requires the recorded run to be present locally. It makes no network requests, regenerates the snapshot through the Rust pipeline, and fails on any divergence — reporting the exact differing field. It also runs one recorded response through the production extract → lineage → verify path.",
    ),
  );

  root.append(
    details(
      "capability matrix — public vs local rebuild",
      table(
        ["capability", "public", "local rebuild"],
      [
        ["validate schema and required fields", "yes", "yes"],
        ["recompute the threshold identity", "yes", "yes"],
        ["cross-check summaries against detail", "yes", "yes"],
        ["re-derive minOutAmount equality from source", "no — attested", "yes"],
        ["rebuild the extract from raw responses", "no", "yes"],
        ["byte-search transactions from scratch", "no", "yes"],
        ["network requests", "none", "none"],
      ].map(([capability, pub, local]) =>
        el("tr", {}, [
          el("td", { text: capability }),
          el("td", { class: "mono", text: pub }),
          el("td", { class: "mono", text: local }),
        ]),
        ),
      ),
    ),
  );

  if (extract) {
    root.append(
      details(
        "what the snapshot carries",
        table(
        ["field", "value"],
        [
          ["experiment_id", extract.experiment_id],
          ["generated_by", extract.generated_by],
          ["schema_version", extract.schema_version],
        ].map(([k, v]) =>
          el("tr", {}, [el("td", { class: "mono", text: k }), el("td", { class: "mono", text: v })]),
          ),
        ),
      ),
    );
  }

  root.append(
    el("p", { class: "prose" }, [
      "Script: ",
      el("a", {
        href: repoUrl("scripts/reproduce_slippage_article.sh"),
        text: "scripts/reproduce_slippage_article.sh",
        class: "mono",
        rel: "noopener",
      }),
      " · assertions: ",
      el("a", {
        href: repoUrl("src/reference_case.rs"),
        text: "src/reference_case.rs",
        class: "mono",
        rel: "noopener",
      }),
    ]),
  );
  root.append(
    note(
      "The orchestration script contains no empirical logic: every value, comparison and verdict lives in Rust and is asserted by Rust tests.",
    ),
  );
}
