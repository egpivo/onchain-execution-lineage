# DFlow Transaction Lineage

Tracing quote metadata into compiled Solana instructions with Rust.

A Rust lab for tracing DFlow quote metadata into unsigned Solana
transactions, decoded instructions, accounts, and provenance boundaries.
Read-only throughout: no wallet is funded, no transaction is signed or
submitted, at any point.

## What this is (and isn't)

This is a Rust client over DFlow's public, no-key **developer** quote
endpoint (`dev-quote-api.dflow.net`) — not an official DFlow SDK. DFlow
does not publish a Rust HTTP client; `api.rs` is this project's own
`reqwest`-based client.

Separately, this project uses
[`dflow-amm-interface`](https://github.com/DFlowProtocol/dflow-amm-interface)
— DFlow's own real, published `Amm` trait (a fork of Jupiter's
`jupiter-amm-interface` pattern, pinned to `solana-sdk = "=2.3.*"`) — as a
reference for how a router can construct a venue-level swap instruction
through a generic adapter, without that instruction necessarily naming the
router as an invoked program. This project does not claim to have
implemented a full venue-specific adapter against that trait; doing so
requires reverse-engineering each venue's own on-chain account layout,
which is out of scope for this pass. Citing the trait's existence and
shape is not the same as citing a working implementation, and this README
does not conflate the two.

## Verified so far

- **DFlow's no-key developer quote endpoint returns no `transaction`
  field**, confirmed live (not assumed) across multiple real captures.
  This is a quote-only surface, distinct from an order/swap endpoint that
  would return a signable payload.
- **The transaction decoder works against real mainnet data**, not just a
  synthetic fixture: tested against a real, finalized Jupiter Aggregator
  v6 transaction, correctly identifying every instruction's program
  (System Program, Compute Budget, Associated Token Account, SPL Token,
  Jupiter Aggregator v6 itself) with zero unresolved "unknown" program
  IDs.
- **Address-lookup-table resolution works against real mainnet data**:
  resolved a real versioned transaction's lookup table to its full
  169-address on-chain list via a read-only RPC call.
- Every hand-recalled program ID and the Jito tip-account list were
  independently verified via web search / official documentation before
  being shipped in `program_registry.rs` — not trusted from memory.

## Not yet done

- No DFlow-order or JTX-quote transaction has actually been decoded --
  only DFlow's quote-only endpoint (no transaction available) and a real
  but unrelated mainnet transaction (used to verify the decoder itself
  works). Capturing a JTX unsigned transaction requires a browser-driven
  session (Privy login) this environment doesn't have; see the Sunday
  project's `research/engineering_capture_handoff.md` for that handoff.
- No specific AMM venue (e.g. BisonFi, the venue DFlow's own quotes have
  returned for USDC→SOL) has been implemented against the `Amm` trait.

## Usage

```bash
cargo run -- quote --pair USDC/SOL --amount-usd 1000 --slippage-bps 50
cargo run -- fetch-and-decode --signature <a real transaction signature>
cargo run -- decode --file <path to a file containing a base64 transaction>
cargo run -- lineage
```

## Project layout

```
src/
├── main.rs             -- CLI (clap)
├── api.rs               -- DFlow dev-quote-api client (reqwest)
├── models.rs             -- typed quote response
├── capture.rs             -- capture orchestration (raw + parsed + metadata)
├── transaction.rs          -- base64 transaction decode (solana-sdk)
├── lookup_tables.rs         -- ALT resolution (read-only RPC)
├── program_registry.rs       -- verified known program IDs, Jito tip accounts
└── lineage.rs               -- field-lineage CSV generation
captures/    -- real captured quote responses (raw + parsed + metadata)
analysis/    -- generated lineage CSV
```
