#!/usr/bin/env bash
#
# Reproducibility entry point for the DFlow slippage reference case.
#
#   ./scripts/reproduce_slippage_article.sh                      public verification
#   ./scripts/reproduce_slippage_article.sh --from-recorded-run  local full rebuild
#
# This script is orchestration only. Every empirical value, comparison and
# verdict is computed in Rust (src/reference_case.rs) and asserted by Rust
# tests; nothing here re-implements the arithmetic, the byte search, the
# eligibility rule or the encoding classification.
#
# Neither mode signs, submits, or makes a network request.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="public"
skip_tests=0
passthrough=()

for arg in "$@"; do
  case "$arg" in
    --from-recorded-run) mode="local"; passthrough+=("$arg") ;;
    --skip-tests)        skip_tests=1 ;;
    -h|--help)
      sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) passthrough+=("$arg") ;;
  esac
done

if [[ "$skip_tests" -eq 0 ]]; then
  echo "== Rust assertions =="
  cargo test --quiet --test article_reproducibility
  cargo test --quiet --lib reference_case
  echo
fi

echo "== Reference case =="
# The `+` expansion keeps `set -u` happy with an empty array on bash 3.2.
cargo run --quiet --bin onchain-execution-lineage -- reference-case \
  ${passthrough[@]+"${passthrough[@]}"}

if [[ "$mode" == "public" ]]; then
  echo
  echo "For the end-to-end rebuild from the original captures (requires the private"
  echo "recorded run under artifacts/experiments/), re-run with --from-recorded-run."
fi
