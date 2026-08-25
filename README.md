# Onchain Execution Lineage

[![Rust](https://github.com/egpivo/onchain-execution-lineage/actions/workflows/rust.yaml/badge.svg)](https://github.com/egpivo/onchain-execution-lineage/actions/workflows/rust.yaml)
[![codecov](https://codecov.io/gh/egpivo/onchain-execution-lineage/graph/badge.svg?token=URjc54t2hA)](https://codecov.io/gh/egpivo/onchain-execution-lineage)

Read-only Rust toolkit for reconstructing and verifying execution lineage across
provider responses, transaction construction, and settlement. It does not sign
or submit transactions, simulate fills, or infer causal effects.

- DFlow is the first complete provider integration.
- Solana is the first execution backend.
- Jupiter is partial, and says so per field rather than faking parity.

**[Project site →](https://egpivo.github.io/onchain-execution-lineage/)** —
overview, architecture, reference case, and an execution-lineage viewer.
The viewer renders Rust-generated artifacts; it does not define empirical
results.

| Layer | Role |
|---|---|
| CLI / Rust | extracts and verifies execution evidence |
| Web site | renders lineage, verification, and use cases |
| Reference case | DFlow slippage-threshold experiment |

## Pipeline

One path creates a `LineageBundle`. Everything else consumes one.

```text
raw response / manifest / tx file / signature
              │  ingestion
              ▼
     provider extraction (adapters/)
              ▼
        ExecutionContext
              ▼
     Solana extraction (solana/), when a tx exists
              ▼
         LineageBuilder → LineageBundle
              │              │
           trace          verify
```

`extract` constructs lineage, `trace` explains it, `verify` checks it.
Every `ExecutionContext` stage is optional: missing input means a missing
stage, never an empty fake one. Architecture detail lives on the
[project site](https://egpivo.github.io/onchain-execution-lineage/#/docs/architecture).

| Module | Role |
|---|---|
| `adapters/` | `RawProviderArtifact → ProviderExtraction` |
| `execution_context` | normalized, stage-optional execution state |
| `solana/` | decode, ALT, account ordering → `TransactionObservation` |
| `lineage_builder` | cross-stage joins → `LineageBundle` |
| `checks/` | `generic/`, `dflow/`, `solana/`, `settlement/` |
| `trace` | ingestion glue + rendering |
| `providers/` | compatibility shim; closed to new provider semantics |

## Reproduce the DFlow slippage reference case

No signing, no submission, no live request in either mode.

```bash
# Public verification — works on a clean clone
./scripts/reproduce_slippage_article.sh

# Local rebuild from recorded captures (gitignored)
./scripts/reproduce_slippage_article.sh --from-recorded-run
```

**Public mode** verifies the tracked snapshot
(`artifacts/analysis/route_stable_evidence_extract.json`). It re-derives
threshold arithmetic and re-aggregates summary claims from per-request detail.
It does not rebuild the experiment: the 30 raw responses are unpublished
because they embed the requester's wallet pubkey. Each row is tagged
`recomputed`, `cross-checked`, or `attested`.

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

**Local rebuild** regenerates the snapshot through the Rust pipeline from
`artifacts/experiments/`, diffs it against the tracked extract, then runs one
recorded response through `extract → lineage → verify`. Case walkthroughs:
[`#/explore/dflow-slippage`](https://egpivo.github.io/onchain-execution-lineage/#/explore/dflow-slippage).

On a recorded response, the quoted `outAmount` occurs at `instruction[2]+99`;
the slippage threshold occurs nowhere in any instruction payload. Byte
presence is not semantic decoding; non-recovery is not evidence of absence.

## Commands

Binary: `onchain-execution-lineage`. Deprecated alias `dflow-lineage` runs the
same CLI with a notice.

```bash
cargo run -- extract --provider dflow --response capture.json
cargo run -- trace   --lineage artifacts/lineage/<id>
cargo run -- verify  --lineage artifacts/lineage/<id>
cargo run -- verify  --provider dflow --response capture.json

make serve              # http://127.0.0.1:8080
make build-web          # regenerate web/data + web/samples from Rust
```

| Command | Role |
|---|---|
| `extract` | raw evidence → `ExecutionContext` + `LineageBundle` |
| `trace` | explain provenance / ingest a manifest into the pipeline |
| `verify` | run cross-layer checks |
| `decode` / `fetch-and-decode` / `map` | transaction decode + ALT / account map |
| `diff` | compare two bundles |
| `quote` | live DFlow-dev `/quote` capture |
| `fingerprint` | corpus group report |
| `experiment` / `route-bracket` | bounded mechanism experiments |
| `reference-case` | reproduce/verify the DFlow slippage case |

`extract` and `verify` are offline unless `--rpc-url` is given (ALT
resolution). `--enrich-settlement` additionally fetches settlement metadata
for a `--signature`.

To inspect your own run in the browser, produce artifacts with the CLI and
drop them on [`#/inspect`](https://egpivo.github.io/onchain-execution-lineage/#/inspect)
(File API only; nothing is uploaded).

## Evidence discipline

Attributions carry an explicit evidence level —
`direct_observation`, `decoded_from_transaction`, `resolved_from_rpc`,
`external_program_label`, `cross_artifact_inference`, `candidate`,
`unresolved`. There is no scalar confidence score.

| Status | Meaning |
|---|---|
| `PASS` | relationship holds on observed evidence |
| `FAIL` | contradicted by observed evidence |
| `CANDIDATE` | consistent, but indistinguishable from coincidence |
| `UNKNOWN` | evidence could exist but was not observed |
| `NOT_APPLICABLE` | check does not apply to this artifact |

A candidate never becomes a pass. No settlement input means no settlement
claim. Rust owns the empirical facts; Python/JS may cross-check and render,
not redefine fingerprints, topology, eligibility, byte-search results, or
verification outcomes.

## Layout

```text
Makefile       make help
src/           library + CLI
web/           static site (no Node build)
examples/      bundled site sample via the real pipeline
scripts/       build_web.sh, reproduce_slippage_article.sh
tests/         unit/integration + public fixtures
schemas/       machine-readable contracts
artifacts/     captures, experiments, lineage, analysis
.local/        private only (gitignored)
```

## Limits

- Not a trading bot, wallet, explorer, or provider SDK.
- Experiments compare mechanism evidence; they do not simulate fills,
  balances, wallets, realized fees, or PnL.
- Unsigned instructions are never described as executed.
- Priority fees are not Jito delivery proof.
- The causal model shipped with the experiments is explanatory and
  number-free; verification does not depend on it.

## Gates

```bash
make gates      # fmt --check + clippy -D warnings + test --all
```
