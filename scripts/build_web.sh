#!/usr/bin/env bash
#
# Regenerate the artifacts the web viewer ships with.
#
#   ./scripts/build_web.sh
#
# Orchestration only. The sample lineage, verification report and evidence
# snapshot are produced by Rust (examples/generate_web_sample.rs) through the
# same pipeline a normal user runs. No Node, no bundler, no network.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo run --quiet --example generate_web_sample
echo
echo "web/ is a static site. Serve it locally with:"
echo "  make serve"
