# DFlow Transaction Lineage

[![Rust](https://github.com/egpivo/dflow-transaction-lineage/actions/workflows/rust.yaml/badge.svg)](https://github.com/egpivo/dflow-transaction-lineage/actions/workflows/rust.yaml)
[![codecov](https://codecov.io/gh/egpivo/onchain-execution-lineage/graph/badge.svg?token=URjc54t2hA)](https://codecov.io/gh/egpivo/onchain-execution-lineage)

**What survives from an execution interface into a Solana transaction?**

Read-only Solana execution mechanism and transaction-lineage experiments in Rust.

No signing. No submission. No wallet keys.

## Evidence stages

```text
UI / app claim
    → provider JSON (quote/order)
        → unsigned VersionedTransaction (if present)
            → loaded accounts / programs (decode + ALT resolve)
                → settlement (only with a signature)
```

Each attribution carries an explicit evidence level
(`direct_observation`, `decoded_from_transaction`, `resolved_from_rpc`,
`external_program_label`, `cross_artifact_inference`, `candidate`,
`unresolved`). There is no scalar confidence score.

## Layout

```text
src/           library + CLI
tests/         unit/integration tests and public fixtures
schemas/       machine-readable contracts (not prose docs)
artifacts/     runtime outputs (captures, analysis)
.local/        private only (gitignored): docs, research corpus
```

## Quick start

```bash
cargo test
cargo run -- lineage
cargo run -- quote --pair USDC/SOL --amount-usd 1000 --slippage-bps 50
```

## Trace / diff (public smoke fixture)

```bash
cargo run -- trace \
  --manifest tests/fixtures/manifests/valid_dflow_dev.json \
  --provider-json tests/fixtures/dev_quote_usdc_sol_no_tx.json \
  --out-json artifacts/analysis/dflow_dev_lineage.json
```

## Controlled experiments are not simulated fills

`experiment` runs a **bounded**, manifest-declared set of provider requests
(fixture JSON or live developer quotes). It compares quote fields, optional
unsigned transactions, and lineage diffs against a baseline.

It does **not** simulate fills, balances, wallets, realized fees, PnL, or
landed execution. Mechanism reports only bucket what changed / did not change /
cannot be observed without settlement.

```bash
cargo run -- experiment \
  --manifest tests/fixtures/experiments/fee_injection_synthetic.json
```

Public synthetic manifests: fee injection, slippage threshold encoding, and
size/route change under `tests/fixtures/experiments/`.

Private research captures and fingerprint corpora live under `.local/corpus/`
(not published). Docs / ADR / changelog also live under `.local/docs/`.

## Evidence levels

| Level | Meaning |
|---|---|
| `direct_observation` | Present in a captured JSON/UI artifact |
| `decoded_from_transaction` | Present in decoded transaction bytes |
| `resolved_from_rpc` | Filled via read-only RPC |
| `external_program_label` | Known program ID from the verified registry |
| `cross_artifact_inference` | Joined across artifacts with stated caveats |
| `candidate` | Suggestive; needs repetition + negative controls |
| `unresolved` | Not observable with current evidence |

## Commands

| Command | Role |
|---|---|
| `quote` | Live DFlow-dev `/quote` capture |
| `decode` / `fetch-and-decode` / `map` | Transaction decode + ALT map |
| `lineage` | Static DFlow-dev field-lineage CSV |
| `trace` | Build `LineageBundle` + Markdown/CSV/DOT |
| `diff` | Compare two bundles |
| `fingerprint` | Corpus group report (refuses n&lt;2 promotion) |
| `experiment` | Bounded fixture/live mechanism experiment (`/quote` or `/order`) |

## Limits

- Not a trading bot, wallet, block explorer, or DFlow SDK.
- DFlow program IDs are provider-generic, not JTX-specific.
- Priority fees ≠ Jito delivery proof.
- Unsigned instructions are never described as executed.
- Cross-Surface economic panel stays in Python; this repo is provenance-only.

## Reproducibility

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo test --test live_network -- --ignored   # optional network
```
