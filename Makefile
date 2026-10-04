# memgraph — build entry points.
#
#   make build          resolve dependencies from crates.io (normal, networked)
#   make build-offline  build against the vendored crates in vendor/ (no network)
#
# The offline path uses .cargo/config.vendored.toml, which cargo only reads when
# it is passed explicitly, so `make build` is unaffected by the vendor submodule.

CARGO       ?= cargo
PKG         ?= memgraph
LOCK        ?= --locked
OFFLINE_CFG ?= .cargo/config.vendored.toml

.DEFAULT_GOAL := build
.PHONY: help build build-offline vendor-init check test run fmt clippy clean

help: ## Show this help
	@printf 'Usage: make [target]\n\nTargets:\n'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build from crates.io (needs network)
	$(CARGO) build $(LOCK)

build-offline: vendor-init ## Build from vendor/ with no network access
	$(CARGO) --config $(OFFLINE_CFG) build --offline $(LOCK)

vendor-init: ## Populate vendor/ if the submodule is not checked out
	@ls vendor/*/.cargo-checksum.json >/dev/null 2>&1 || { \
		echo "vendor/ is empty — initialising vendor submodule"; \
		git submodule update --init vendor; \
	}

check: ## Type-check the workspace
	$(CARGO) check $(LOCK)

test: ## Run the test suite
	$(CARGO) test $(LOCK)

run: ## Run the memgraph binary
	$(CARGO) run -p $(PKG) $(LOCK)

fmt: ## Format all code
	$(CARGO) fmt --all

clippy: ## Lint all targets, warnings denied
	$(CARGO) clippy --all-targets $(LOCK) -- -D warnings

clean: ## Remove build artifacts
	$(CARGO) clean
