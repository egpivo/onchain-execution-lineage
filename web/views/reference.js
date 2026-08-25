// Reference — lightweight project documentation.
//
// Names and shapes here track the current Rust source; each entry links to the
// module that defines it. Nothing is invented: if a type or flag is listed, it
// exists in the crate.

import { el, link, table, note, prose, statusBadge, ascii } from "../components/format.js";
import { repoUrl } from "../app.js";

function src(path, label) {
  return el("a", { href: repoUrl(path), text: label || path, rel: "noopener", class: "mono" });
}

const CLI = [
  [
    "extract",
    "raw evidence → ExecutionContext + LineageBundle",
    "--provider --response --transaction --manifest --signature --rpc-url --enrich-settlement --out-dir --stdout",
  ],
  [
    "trace",
    "explain provenance and cross-stage relationships, or ingest a manifest into the pipeline",
    "--lineage | --manifest --provider-json --transaction --signature --resolve-alts --out-json --out-context --out-md --out-csv --out-dot",
  ],
  [
    "verify",
    "run cross-layer checks against a lineage or a raw response",
    "--lineage | --provider --response --transaction --signature --rpc-url --enrich-settlement --out-json",
  ],
  ["reference-case", "public verification or local rebuild of a published case", "--from-recorded-run --extract --base-dir --json"],
  ["decode / fetch-and-decode / map", "transaction decode, ALT resolution, account map", "--file --signature --rpc-url --out-csv --out-md"],
  ["diff", "compare two lineage bundles", "--left --right --out-json --out-md"],
  ["quote", "live DFlow developer /quote capture", "--pair --amount-usd --slippage-bps"],
  ["fingerprint", "corpus group report; refuses n<2 promotion", "--corpus --group --base-dir --out"],
  ["experiment / route-bracket", "bounded mechanism experiments", "--manifest --base-dir --rpc-url --resolve-alts"],
  ["lineage", "deprecated static field-lineage CSV", "--out"],
];

const TYPES = [
  {
    name: "ProviderAdapter",
    path: "src/adapters/mod.rs",
    signature: `trait ProviderAdapter {
    fn provider_id(&self) -> ProviderId;
    fn detect(&self, raw: &RawProviderArtifact) -> bool;
    fn extract(&self, raw: &RawProviderArtifact) -> Result<ProviderExtraction>;
}`,
    body: "Implemented by dflow, jupiter, jtx and generic. `ProviderExtraction` carries the normalized intent, response, route, unsigned-transaction reference, namespaced extensions, and an explicit list of fields the adapter chose not to normalize.",
  },
  {
    name: "ExecutionContext",
    path: "src/execution_context.rs",
    signature: `struct ExecutionContext {
    schema_version: String,
    provider: ProviderId,
    intent: Option<ExecutionIntent>,
    provider_response: Option<ProviderResponse>,
    route: Option<RouteObservation>,
    transaction: Option<TransactionObservation>,
    settlement: Option<SettlementObservation>,
    provenance: Provenance,
}`,
    body: "Stage-optional normalized state. `Stage` enumerates intent, provider_response, route, transaction_construction, settlement. `has_settlement_evidence()` requires both applicability and a signature — a signature alone is a pointer, not an observation.",
  },
  {
    name: "TransactionObservation",
    path: "src/solana/mod.rs",
    signature: `struct TransactionObservation {
    version: TransactionVersion,     // Legacy | V0
    decoded: DecodedTransaction,
    topology: TransactionTopology,
    alt_resolution: AltResolution,
    account_index_validity: AccountIndexValidity,
    compute_budget: ComputeBudgetObservation,
    signers: SignerObservation,
    account_map: Option<InstructionAccountMap>,
}`,
    body: "Named apart from `lineage_model::TransactionConstruction`: one is what this chain's encoding shows, the other is the chain-agnostic stage summary inside a bundle.",
  },
  {
    name: "LineageBundle",
    path: "src/lineage_model.rs",
    signature: `struct LineageBundle {
    schema_version: String,
    capture, quote, fee, route,
    transaction_construction,
    execution, delivery, settlement,
    claims: Vec<AttributionClaim>,
    links: Vec<LineageLink>,
    unresolved: Vec<UnresolvedField>,
    raw_extensions: BTreeMap<String, Value>,
    decoded_transaction: Option<DecodedTransaction>,
}`,
    body: "The canonical cross-layer representation. `assert_unsigned_has_no_settlement_claims()` is a structural guard: an unsigned artifact that emits a settlement claim is an error, not a warning.",
  },
  {
    name: "LineageLink",
    path: "src/lineage_model.rs",
    signature: `struct LineageLink {
    id: String,
    from_stage: Stage,
    to_stage: Stage,
    relationship: String,       // same_value | value_mismatch |
                                // candidate_byte_match | not_recoverable
    subject: String,
    object: String,
    evidence_level: EvidenceLevel,
    claim_ceiling: String,
    explanation: String,
    evidence: Vec<String>,
}`,
    body: "Every link carries the strongest thing it is allowed to support. Ceilings are text because they are meant to be read, not compared.",
  },
  {
    name: "ExecutionCheck / CheckResult",
    path: "src/checks/mod.rs",
    signature: `trait ExecutionCheck {
    fn id(&self) -> &'static str;
    fn run(&self, ctx: &ExecutionContext, lineage: &LineageBundle) -> CheckResult;
}

struct CheckResult {
    check_id: String,
    status: CheckStatus,
    stages: Vec<Stage>,
    provider: ProviderId,
    observed: Option<String>,
    expected: Option<String>,
    evidence: Vec<String>,
    explanation: String,
    evidence_ceiling: String,
    provenance: Vec<String>,
}`,
    body: "Checks live under checks/generic, checks/dflow, checks/solana and checks/settlement. Provider-specific checks are selected by the context's provider, so a Jupiter artifact never runs DFlow's arithmetic.",
  },
];

export function renderReference(root) {
  root.append(el("p", { class: "eyebrow", text: "Reference" }));
  root.append(el("h1", { text: "Reference" }));
  root.append(
    el("p", {
      class: "lede",
      text: "The CLI surface and the types the site renders. Every entry links to the module that defines it.",
    }),
  );

  root.append(el("h2", { text: "CLI" }));
  root.append(
    prose(
      "The binary is onchain-execution-lineage; a deprecated dflow-lineage alias runs the same CLI. extract and verify are offline unless --rpc-url is given.",
    ),
  );
  root.append(
    table(
      ["command", "role", "principal flags"],
      CLI.map(([name, role, flags]) =>
        el("tr", {}, [
          el("td", { class: "mono", text: name }),
          el("td", { text: role }),
          el("td", { class: "mono muted", text: flags }),
        ]),
      ),
    ),
  );

  root.append(el("h2", { text: "Types" }));
  for (const type of TYPES) {
    root.append(el("h3", { text: type.name }));
    root.append(el("p", { class: "prose" }, ["Defined in ", src(type.path)]));
    root.append(ascii(type.signature));
    root.append(prose(type.body));
  }

  root.append(el("h2", { text: "Status semantics" }));
  root.append(
    table(
      ["status", "meaning", "what it must never be read as"],
      [
        ["PASS", "Holds on observed evidence.", "proof of on-chain execution"],
        ["FAIL", "Contradicted by observed evidence.", "a tooling error"],
        [
          "CANDIDATE",
          "Consistent, but indistinguishable from coincidence.",
          "a weak PASS, or a decoded field",
        ],
        ["UNKNOWN", "Evidence could exist; it was not observed.", "a failure"],
        ["NOT_APPLICABLE", "The check does not apply here.", "a skipped failure"],
      ].map(([status, meaning, never]) =>
        el("tr", {}, [
          el("td", {}, [statusBadge(status)]),
          el("td", { text: meaning }),
          el("td", { class: "muted", text: never }),
        ]),
      ),
    ),
  );

  root.append(el("h2", { text: "Evidence levels" }));
  root.append(
    table(
      ["level", "meaning"],
      [
        ["direct_observation", "present in a captured JSON/UI artifact"],
        ["decoded_from_transaction", "present in decoded transaction bytes"],
        ["resolved_from_rpc", "filled via read-only RPC"],
        ["external_program_label", "known program ID from the verified registry"],
        ["cross_artifact_inference", "joined across artifacts, with stated caveats"],
        ["candidate", "suggestive; needs repetition and negative controls"],
        ["unresolved", "not observable with current evidence"],
      ].map(([level, meaning]) =>
        el("tr", {}, [el("td", { class: "mono", text: level }), el("td", { text: meaning })]),
      ),
    ),
  );
  root.append(el("p", { class: "prose" }, ["Defined in ", src("src/evidence.rs")]));

  root.append(el("h2", { text: "Reproducibility" }));
  root.append(
    el("pre", {
      text: [
        "# public verification of the tracked evidence snapshot",
        "./scripts/reproduce_slippage_article.sh",
        "",
        "# local rebuild from the original recorded captures (private)",
        "./scripts/reproduce_slippage_article.sh --from-recorded-run",
        "",
        "# regenerate this site's bundled artifacts from Rust",
        "./scripts/build_web.sh",
        "",
        "# gates",
        "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all",
      ].join("\n"),
    }),
  );
  root.append(
    note(
      "Public verification checks the published evidence snapshot; it does not rebuild the experiment from raw captures, because those are not published.",
    ),
  );
  root.append(
    el("p", { class: "prose" }, [
      "Full crate documentation: ",
      src("src/lib.rs", "src/lib.rs"),
      " · ",
      link("#/docs/architecture", "Architecture"),
    ]),
  );
}
