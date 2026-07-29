# DFlow Transaction Lineage

[![Rust](https://github.com/egpivo/dflow-transaction-lineage/actions/workflows/rust.yaml/badge.svg)](https://github.com/egpivo/dflow-transaction-lineage/actions/workflows/rust.yaml)
[![codecov](https://codecov.io/gh/egpivo/dflow-transaction-lineage/graph/badge.svg?token=URjc54t2hA)](https://codecov.io/gh/egpivo/dflow-transaction-lineage)

Rust lab for tracing DFlow quote metadata into unsigned Solana transactions.
Read-only: no signing, no submission.

Uses DFlow's public no-key developer quote endpoint (`dev-quote-api.dflow.net`)
via a local `reqwest` client — not an official SDK.

## Usage

```bash
cargo run -- quote --pair USDC/SOL --amount-usd 1000 --slippage-bps 50
cargo run -- fetch-and-decode --signature <transaction signature>
cargo run -- decode --file <path to base64 transaction>
cargo run -- lineage

cargo test
cargo test --test live_network -- --ignored   # hits DFlow + mainnet RPC
```

## Layout

```
src/              -- CLI, API client, capture, decode, lineage
tests/
├── fixtures/     -- frozen inputs for tests
├── quote_fixture.rs
└── live_network.rs
artifacts/
├── captures/     -- captured quote responses
└── analysis/     -- lineage CSV
```
