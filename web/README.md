# Onchain Execution Lineage — project site

Static documentation and interactive analysis site. No framework, no bundler,
no Node, no backend.

```text
web/
  index.html            shell, primary navigation, product boundary
  app.js                hash router, data loading, shared state, legacy aliases
  styles.css            restrained documentation style + status vocabulary
  components/           reusable pieces
    format.js           display-only helpers (DOM, digit grouping, badges)
    mini-flow.js         minimal 4-stage diagram (home only)
    stage-flow.js        provider-neutral five-stage explainer (Docs)
    stages.js            lineage-driven stage view
    checks.js            CheckResult cards grouped by stage
    links.js             LineageBundle.links with claim ceilings
    loader.js            browser-local File API loading
  views/
    home.js               first screen: what it does, one real example
    explore.js             data-driven use-case index (cards only)
    docs/index.js         quick start, how it works, links to architecture/reference
    architecture.js        software architecture
    reference.js           CLI and type reference
    inspect.js             lineage inspector: picker, then title/checklist/tabs
    dflow-fee.js            DFlow platform-fee use case
    dflow-slippage/         index, threshold, route, identification, bytes, reproduce
  data/                 tracked public artifacts + use-cases.json (site config)
  samples/              Rust-generated sample lineage for the inspector
```

## Navigation

Primary nav is exactly Home / Explore / Docs / GitHub. Architecture and
Reference live under Docs; loading a lineage is the "Inspect lineage JSON" CTA
on Home, not a nav category. `app.js`'s `parseHash()` aliases every hash path
from the previous navigation (`#/overview`, `#/use-cases/…`, `#/architecture`,
`#/reference`, `#/load?panel=…`) onto its new destination, so old links and
bookmarks keep resolving. `tests/web_contract.rs::legacy_hash_paths_still_resolve`
and `::primary_navigation_is_reduced_to_four_items` hold both properties.

## The boundary

Rust decides; this site explains. No file here decodes a transaction, evaluates
a threshold, searches bytes, classifies an encoding, resolves a lookup table,
compares routes, or assigns a check status. `tests/web_contract.rs` enforces it,
including a guard that rejects published values appearing as literals in JS.

## Adding a use case

1. Add an entry to `data/use-cases.json` — title, types, providers, chain,
   question, views, and metrics declared as paths into a tracked artifact.
2. Add a view module under `views/` and register it in `app.js`'s `CASE_VIEWS`.

Navigation, badges and figures follow from the config.

## Run locally

```bash
./scripts/build_web.sh                       # refresh data/ and samples/ from Rust
python3 -m http.server --directory web 8080
```
