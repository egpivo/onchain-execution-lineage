# DFlow Transaction Lineage

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
```

## Layout

```
src/         -- CLI, API client, capture, decode, lineage
artifacts/
├── captures/   -- captured quote responses
└── analysis/   -- lineage CSV
```
