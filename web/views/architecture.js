// Architecture page.
//
// Describes the software. Component names track the current Rust source; the
// repository links point at the module that owns each boundary.

import { el, link, prose, ascii, note, table, statusBadge } from "../components/format.js";
import { repoUrl } from "../app.js";

function moduleLink(path) {
  return el("a", { href: repoUrl(path), text: path, rel: "noopener", class: "mono" });
}

function layer(root, { title, source, diagram, body, points, ceiling }) {
  root.append(el("h2", { text: title }));
  if (source) {
    root.append(el("p", { class: "prose" }, ["Source: ", moduleLink(source)]));
  }
  if (diagram) root.append(ascii(diagram));
  root.append(prose(body));
  if (points) {
    root.append(
      el(
        "ul",
        { class: "prose" },
        points.map((p) => el("li", { text: p })),
      ),
    );
  }
  if (ceiling) root.append(el("p", { class: "note warn", text: ceiling }));
}

export function renderArchitecture(root) {
  root.append(el("p", { class: "eyebrow", text: "Architecture" }));
  root.append(el("h1", { text: "How the verifier is put together" }));
  root.append(
    el("p", {
      class: "lede",
      text: "One canonical path builds a lineage. Everything else consumes one.",
    }),
  );

  root.append(
    ascii(`raw response │ manifest │ transaction file │ signature
        └──────────┴────────────┴────────────┘
                        │  ingestion
                        ▼
              provider extraction (adapters/)
                        ▼
                 ExecutionContext
                        ▼
           Solana extraction (solana/), when a transaction exists
                        ▼
                  LineageBuilder
                        ▼
                  LineageBundle
                   │     │     │
             trace │     │     │ verify
           (explain)     │     (check)
                      diff / report / fingerprint`),
  );

  layer(root, {
    title: "1 · Provider boundary",
    source: "src/adapters/mod.rs",
    diagram: `raw provider response
        ▼
   ProviderAdapter::extract
        ▼
   ProviderExtraction`,
    body: [
      "An adapter takes a RawProviderArtifact — whatever the provider actually returned — and emits a ProviderExtraction built only from provider-neutral names.",
      "Provider-native field names stop here. Nothing downstream reads outAmount, otherAmountThreshold or routePlan; a test enforces that across the core modules.",
      "Raw leftovers are preserved, namespaced by provider, so nothing is silently dropped. Generic code may print them and must not branch on them.",
    ],
    points: [
      "DFlow — first complete integration: order and quote surfaces, threshold fields, execution mode, platform fee, inline transaction",
      "Jupiter — partial by declaration: it reports minOutAmount and platform-fee accounting as unsupported rather than implying parity",
      "JTX — sanitized capture envelopes, including transactions referenced as external files",
      "Generic — pre-normalized envelopes, or minimally described input with unambiguous neutral field names",
    ],
    ceiling:
      "An adapter never decodes the transaction it finds. It reports that a payload exists and hands the bytes on.",
  });

  layer(root, {
    title: "2 · ExecutionContext",
    source: "src/execution_context.rs",
    diagram: `ExecutionContext {
    provider,
    intent?,
    provider_response?,
    route?,
    transaction?,
    settlement?,
    provenance,
}`,
    body: [
      "The normalized execution state the verifier core operates on. Every stage is optional, so one model covers a response on its own, a response plus an unsigned transaction, a transaction on its own, and a full intent-to-settlement lineage.",
      "An absent stage means not observed — never observed-empty. A manifest is one convenient way to fill in identity and provenance; it is not required, and it is not the domain model.",
    ],
  });

  layer(root, {
    title: "3 · Chain extraction — currently Solana",
    source: "src/solana/mod.rs",
    diagram: `unsigned transaction bytes (+ optional RPC context)
        ▼
   SolanaExtractor::extract_base64
        ▼
   TransactionObservation`,
    body: [
      "Provider-independent by construction: nothing in this layer knows whether the bytes came from DFlow, Jupiter or a file on disk. It wraps the existing primitives rather than reimplementing them.",
    ],
    points: [
      "legacy / v0 message version, read from the encoding rather than inferred from lookup-table presence",
      "address-lookup-table resolution, with unresolved tables reported as unresolved",
      "exact loaded-account ordering: static keys, then ALT writable, then ALT readonly",
      "compiled-instruction extraction, program-ID attribution, compute-budget observation",
      "account-index validity against the loaded vector, computable offline",
      "signer observations: fee-payer position and empty signature slots — never consent",
    ],
    ceiling:
      "Table membership is not transaction relevance: only indexed entries are loaded. With a table unresolved, absence of an account cannot be claimed.",
  });

  layer(root, {
    title: "4 · LineageBuilder",
    source: "src/lineage_builder.rs",
    body: [
      "The builder's job is the joins: which mint the caller asked for versus which mint the quote priced versus which accounts the transaction touches.",
      "Every join becomes a LineageLink carrying a relationship, an evidence level, supporting evidence and an explicit claim ceiling.",
    ],
    points: [
      "same_value / value_mismatch — two stages agree, or they do not",
      "candidate_byte_match — a quoted integer's encoding occurs in an instruction payload",
      "not_recoverable — searched and not found, which is not the same as absent",
      "derived_from — one observation is downstream of another",
    ],
    ceiling:
      "A numeric coincidence is not a semantic fact. There is no path in the builder that turns a candidate into one.",
  });

  layer(root, {
    title: "5 · Verification",
    source: "src/checks/mod.rs",
    diagram: `trait ExecutionCheck {
    fn id(&self) -> &'static str;
    fn run(&self, ctx: &ExecutionContext, lineage: &LineageBundle) -> CheckResult;
}`,
    body: [
      "Checks are grouped by ownership: generic checks that run for every provider, provider-specific checks selected by the context's provider, Solana mechanics, and settlement.",
      "A result carries the check ID, the stages it spans, observed and expected values, evidence, provenance, an explanation and an evidence ceiling.",
    ],
  });

  root.append(
    table(
      ["status", "meaning"],
      [
        ["PASS", "The stated relationship holds on observed evidence."],
        ["FAIL", "The relationship is contradicted by observed evidence."],
        [
          "CANDIDATE",
          "Consistent with the claim, but the evidence cannot separate it from coincidence.",
        ],
        ["UNKNOWN", "The evidence could exist but was not observed. Not a failure."],
        ["NOT_APPLICABLE", "The check does not apply to this artifact at all."],
      ].map(([status, meaning]) =>
        el("tr", {}, [el("td", {}, [statusBadge(status)]), el("td", { text: meaning })]),
      ),
    ),
  );
  root.append(
    note(
      "UNKNOWN and NOT_APPLICABLE are kept apart on purpose: 'we did not resolve the lookup tables' and 'this response has no fee to account for' are different statements, and collapsing them would let a missing input read as a clean bill of health.",
    ),
  );

  layer(root, {
    title: "6 · Evidence model",
    source: "src/evidence.rs",
    diagram: `observation strength      →   maximum claim strength

direct_observation        →   present in a captured artifact
decoded_from_transaction  →   present in transaction bytes
resolved_from_rpc         →   filled from read-only RPC
external_program_label    →   known program ID from the verified registry
cross_artifact_inference  →   joined across artifacts, with caveats
candidate                 →   suggestive; needs repetition and controls
unresolved                →   not observable with current evidence`,
    body: [
      "Every attribution carries an explicit evidence level. There is no scalar confidence score, because a single number invites arithmetic nobody can defend.",
    ],
    ceiling:
      "Candidate evidence cannot silently become PASS. The check that reports byte relationships exists partly to make that ceiling machine-readable.",
  });

  root.append(el("h2", { text: "7 · Research and publication boundary" }));
  root.append(
    prose(
      "Some modules consume the verifier without defining its semantics. They are reference tooling, and the distinction is deliberate:",
    ),
  );
  root.append(
    table(
      ["module", "role"],
      [
        ["src/route_bracket.rs", "bounded A1/T/A2 experiment runner — makes live requests, outside the core"],
        ["src/experiment.rs", "manifest-declared bounded experiments"],
        ["src/evidence_extract.rs", "deterministic publication evidence extract"],
        ["src/reference_case.rs", "public verification and local rebuild of a published case"],
        [
          "artifacts/analysis/route_stable_causal_model.json",
          "authored, number-free explanatory DAG — data, never runtime logic",
        ],
      ].map(([mod, role]) =>
        el("tr", {}, [el("td", {}, [moduleLink(mod)]), el("td", { text: role })]),
      ),
    ),
  );
  root.append(
    note(
      "Verification never depends on the causal model, and no causal edge is derived from observed transaction data. The DAG is an assumption record, not a result.",
    ),
  );

  root.append(
    el("p", { class: "prose" }, [
      "Type-level details are in the ",
      link("#/docs/reference", "Reference"),
      ".",
    ]),
  );
}
