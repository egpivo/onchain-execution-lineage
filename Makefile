# Onchain Execution Lineage — task shortcuts.
#
# Orchestration only: every empirical value comes from the Rust crate, and
# every target here just runs commands that already exist. No Node, no bundler.
#
#   make            serve the site at http://127.0.0.1:8080
#   make PORT=9000  serve on another port
#   make help       list everything else

PORT ?= 8080

.DEFAULT_GOAL := serve
.PHONY: help serve build-web test gates reproduce reproduce-local

help: ## list targets
	@grep -E '^[a-z][a-z-]*:.*##' $(MAKEFILE_LIST) \
	  | awk 'BEGIN {FS = ":.*## "}; {printf "  make %-16s %s\n", $$1, $$2}'

serve: web/data/use-cases.json ## serve the static site (default target)
	@echo "→ http://127.0.0.1:$(PORT)"
	python3 -m http.server --directory web $(PORT)

build-web: ## regenerate web/data + web/samples from the Rust pipeline
	./scripts/build_web.sh

# A fresh clone has no generated web data; build it once before serving.
web/data/use-cases.json:
	./scripts/build_web.sh

test: ## cargo test --all
	cargo test --all

gates: ## fmt --check + clippy -D warnings + full test suite
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test --all

reproduce: ## public verification of the published reference case
	./scripts/reproduce_slippage_article.sh

reproduce-local: ## rebuild from the private recorded run (requires captures)
	./scripts/reproduce_slippage_article.sh --from-recorded-run
