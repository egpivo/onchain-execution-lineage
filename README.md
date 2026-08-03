# onchain-execution-lineage

[![Rust](https://github.com/egpivo/onchain-execution-lineage/actions/workflows/rust.yaml/badge.svg)](https://github.com/egpivo/onchain-execution-lineage/actions/workflows/rust.yaml)
[![codecov](https://codecov.io/gh/egpivo/onchain-execution-lineage/graph/badge.svg?token=URjc54t2hA)](https://codecov.io/gh/egpivo/onchain-execution-lineage)

**onchain-execution-lineage is a read-only Rust toolkit for reconstructing and
verifying execution lineage across provider responses, transaction
construction, and settlement.**

- DFlow is the first complete provider integration.
- Solana is the first execution backend.
- Jupiter support is partial, and says so per field rather than faking parity.

The tool does **not** sign or submit transactions, does **not** simulate fills,
and does **not** infer causal effects.

**[Open the project site →](https://egpivo.github.io/onchain-execution-lineage/)** — overview, use cases, architecture, reference, and an execution-lineage viewer.

| Layer | Role |
|---|---|
| CLI / Rust | extracts and verifies execution evidence |
| Web site | renders Rust-generated lineage, verification results and use cases |
| Reference case | DFlow slippage-threshold experiment |

The viewer is a rendering layer. It is not where empirical results come from —
every value it shows was decided by the Rust pipeline and can be regenerated
from the CLI.

## Canonical lineage construction

There is exactly one path that creates a `LineageBundle`. Everything else
consumes one.

```text
raw response │ manifest │ transaction file │ signature
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
                      diff / report / fingerprint
```

`extract` constructs lineage, `trace` explains it, `verify` checks it. `trace`'s
manifest flags are ingestion only — a manifest supplies identity and
provenance, never normalization. A regression test asserts that `extract` and
`trace` over the same artifact resolve to byte-identical bundles once
manifest-supplied identity is normalized away.

## Architecture

```text
                     Execution Verifier Core
                              │
               ┌──────────────┼──────────────┐
               │              │              │
             DFlow         Jupiter        Generic
            adapter        adapter        adapter
               │
               ▼
      raw provider response
               ▼
      provider-specific extraction        ← provider field names stop here
               ▼
      Normalized ExecutionContext
               ▼
      generic Solana extraction           ← decode, ALT, account ordering
               ▼
          LineageBundle                   ← cross-stage links + ceilings
               ▼
             verify                       ← PASS / FAIL / CANDIDATE /
                                            UNKNOWN / NOT_APPLICABLE
```

Every stage of an `ExecutionContext` is optional. The same model covers a
response on its own, a response plus an unsigned transaction, a transaction on
its own, and a full intent → settlement lineage. A missing input produces a
missing stage, never an empty one.

| Module | Role |
|---|---|
| `adapters/` | provider adapter boundary — `RawProviderArtifact → ProviderExtraction` |
| `execution_context` | normalized, stage-optional execution state |
| `solana/` | provider-independent extraction → `solana::TransactionObservation`: decode, version, ALT resolution, loaded-account ordering, account-index validity, compute budget, topology |
| `lineage_builder` | cross-stage joins → `LineageBundle` |
| `checks/` | `generic/`, `dflow/`, `solana/`, `settlement/` |
| `trace` | ingestion glue + rendering over the pipeline above |
| `providers/` | compatibility shim, closed to new provider semantics |

`solana::TransactionObservation` is what a Solana transaction's bytes show;
`lineage_model::TransactionConstruction` is the chain-agnostic stage summary
inside a bundle. Two names because they are two things.

## Reference cases and reproducibility

### Reproduce the DFlow slippage reference case

One command, two honestly-separated modes. No signing, no submission, no live
request in either.

```bash
# Public verification — works on a clean clone
./scripts/reproduce_slippage_article.sh

# Local rebuild from the original recorded captures
./scripts/reproduce_slippage_article.sh --from-recorded-run
```

**Public mode** verifies the tracked Rust-generated evidence snapshot
(`artifacts/analysis/route_stable_evidence_extract.json`). It re-derives the
threshold arithmetic from the published inputs with the verifier's own
implementation, and re-aggregates every summary claim from the snapshot's
per-request detail. It does **not** rebuild the experiment: the 30 raw provider
responses are not published, because they carry the requester's wallet pubkey.
Each row says which it is — `recomputed`, `cross-checked`, or `attested`.

```text
requests                     30           PASS  cross-checked
threshold ceil identity      30/30        PASS  recomputed
floor identity               0/30         PASS  recomputed
quote literal matches        15/15        PASS  cross-checked
threshold literal matches    0            PASS  cross-checked
quote candidate site         ix2:99       PASS  cross-checked
same-treatment controls      5/5          PASS  cross-checked
settlement                   unavailable  PASS  attested

16/16 claims verified (2 attested, 12 cross-checked, 2 recomputed)
```

**Local rebuild mode** requires the recorded run under
`artifacts/experiments/` (gitignored). It regenerates the snapshot through the
Rust pipeline and compares it field by field against the tracked one, failing on
any divergence, then runs one recorded response through the production
`extract → lineage → verify` path. It proves the chain

```text
recorded raw artifacts → Rust analysis → evidence extract → tracked publication extract
```

with no Python, no frontend and no network step. Both modes are orchestration
only: every value, comparison and verdict lives in `src/reference_case.rs` and
is asserted by Rust tests.

### Project site

A static site over Rust-generated artifacts. Primary navigation is exactly
Home / Explore / Docs / GitHub — Architecture and Reference live under Docs,
and loading your own lineage is a task CTA ("Inspect lineage JSON"), not a nav
category. Old hash paths from the previous navigation
(`#/overview`, `#/use-cases/…`, `#/architecture`, `#/reference`, `#/load`)
still resolve to their new destination.

```bash
./scripts/build_web.sh                       # regenerate web/samples from Rust
python3 -m http.server --directory web 8080
```

Deep links used by the article:

| Route | Shows |
|---|---|
| `#/` | home: what the tool does, a minimal execution path, one real example |
| `#/explore` | the use-case collection |
| `#/explore/dflow-slippage` | reference-case overview |
| `#/explore/dflow-slippage/threshold` | S, Q, M per request and the verified identity |
| `#/explore/dflow-slippage/route?batch=6` | A1 → T → A2 for one bracket, with eligibility |
| `#/explore/dflow-slippage/identification` | the frozen structural model |
| `#/explore/dflow-slippage/bytes?batch=1` | M / Q / Q−M candidate-search results |
| `#/explore/dflow-slippage/reproduce` | the two reproducibility modes |
| `#/docs` | quick start, how it works, links to architecture/reference |
| `#/docs/architecture`, `#/docs/reference` | software documentation |
| `#/inspect` | load your own CLI output, browser-local |

To inspect your own run, produce artifacts with the CLI and drop them onto
`#/inspect` — the File API reads them in the tab and nothing is uploaded:

```bash
onchain-execution-lineage extract --provider dflow --response capture.json --out-dir ./my-lineage
onchain-execution-lineage verify  --lineage ./my-lineage --out-json ./my-lineage/verification.json
```

Bundled samples are synthetic responses run through the real pipeline; the
genuine DFlow reference lineage is not published because its context embeds the
requester's wallet pubkey.

### DFlow reference artifact

A single recorded DFlow `/order` response through the canonical pipeline
(requires the recorded run locally):

```bash
cargo run -- extract \
  --provider dflow \
  --response artifacts/experiments/dflow_order_slippage_route_stable_live/raw/b00_A1_50.json

cargo run -- verify --lineage artifacts/lineage/dflow_592152452cbc
```

```text
PASS           dflow.slippage_threshold_arithmetic  threshold equals ceil(out_amount * (10000 - slippage_bps) / 10000)
PASS           dflow.min_out_matches_threshold      minOutAmount and otherAmountThreshold carry the same value
PASS           solana.transaction_version           message version read from the encoded message
PASS           solana.account_index_validity        every account index resolves inside the loaded account vector
UNKNOWN        solana.alt_resolution                lookup tables referenced but resolution was not attempted (offline)
CANDIDATE      solana.candidate_byte_search         at least one response value appears verbatim in instruction bytes
NOT_APPLICABLE settlement.landed_status             artifact is unsigned or unsubmitted; no settlement evidence exists

PASS=13 FAIL=0 CANDIDATE=1 UNKNOWN=1 NOT_APPLICABLE=4
```

The quoted `outAmount` occurs at `instruction[2]+99`; the slippage threshold
occurs nowhere in any instruction payload. Both results carry an explicit
ceiling — byte presence is not semantic decoding, and non-recovery is not
evidence of absence.

`artifacts/lineage/` is gitignored: the generated context embeds the unsigned
transaction and its fee-payer pubkey.

## Commands

The binary is `onchain-execution-lineage`. A deprecated `dflow-lineage` alias
runs the same CLI and prints a notice, so existing scripts and published
commands keep working; it will be removed in a later milestone.

```bash
cargo run -- extract --provider dflow --response capture.json
cargo run -- trace   --lineage artifacts/lineage/<id>
cargo run -- verify  --lineage artifacts/lineage/<id>

# verify straight from a raw response (extracts first)
cargo run -- verify --provider dflow --response capture.json
```

| Command | Role |
|---|---|
| `extract` | raw evidence → `ExecutionContext` + `LineageBundle` |
| `trace` | explain provenance and cross-stage relationships (`--lineage`), or ingest a manifest into the canonical pipeline (legacy flags) |
| `verify` | run cross-layer checks |
| `decode` / `fetch-and-decode` / `map` | transaction decode + ALT / account map |
| `diff` | compare two bundles |
| `quote` | live DFlow-dev `/quote` capture |
| `fingerprint` | corpus group report (refuses n&lt;2 promotion) |
| `experiment` / `route-bracket` | bounded mechanism experiments |
| `reference-case` | reproduce/verify the DFlow slippage reference case |
| `lineage` | deprecated static field-lineage CSV |

`extract` and `verify` are offline unless `--rpc-url` is given. Adding it
enables address-lookup-table resolution; `--enrich-settlement` additionally
fetches settlement metadata for a `--signature`.

## Evidence discipline

Every attribution carries an explicit evidence level — `direct_observation`,
`decoded_from_transaction`, `resolved_from_rpc`, `external_program_label`,
`cross_artifact_inference`, `candidate`, `unresolved`. There is no scalar
confidence score.

Check results are not booleans:

| Status | Meaning |
|---|---|
| `PASS` | the relationship holds on observed evidence |
| `FAIL` | the relationship is contradicted by observed evidence |
| `CANDIDATE` | consistent with the claim, but indistinguishable from coincidence |
| `UNKNOWN` | the evidence could exist but was not observed |
| `NOT_APPLICABLE` | the check does not apply to this artifact |

A candidate never becomes a pass. No settlement input means no settlement
claim — a signature on its own is a pointer, not an observation.

Rust owns the empirical facts. Downstream Python and JavaScript may cross-check,
project, format and visualize; they do not define route fingerprints,
transaction topology, eligibility, byte-search results, canonical encoding
classification, or verification outcomes.

## Layout

```text
src/           library + CLI (see architecture table above)
web/           static viewer (no build step, no Node)
tests/         unit/integration tests and public fixtures
schemas/       machine-readable contracts (not prose docs)
artifacts/     recorded runs: captures, experiments, lineage, analysis
.local/        private only (gitignored): docs, research corpus
```

## Limits

- Not a trading bot, wallet, block explorer, or provider SDK.
- Experiments compare mechanism evidence; they do not simulate fills,
  balances, wallets, realized fees, or PnL.
- DFlow program IDs are provider-generic, not integrator-specific.
- Priority fees are not Jito delivery proof.
- Unsigned instructions are never described as executed.
- The causal model shipped alongside the experiments is explanatory and
  number-free. Verification does not depend on it, and no causal edge is
  derived from observed transaction data.

## Gates

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo test --test live_network -- --ignored   # optional network
```
