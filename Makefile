CARGO       ?= cargo
PKG         ?= memgraph
LOCK        ?= --locked
OFFLINE_CFG ?= .cargo/config.vendored.toml
RELEASE_DIR ?= target/release

-include Makefile.local

.DEFAULT_GOAL := build
.PHONY: help build build-offline release release-offline vendor-init check test run fmt clippy clean

help: ## Show this help
	@printf 'Usage: make [target]\n\nTargets:\n'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build from crates.io (needs network)
	$(CARGO) build $(LOCK)

build-offline: vendor-init ## Build from vendor/ with no network access
	$(CARGO) --config $(OFFLINE_CFG) build --offline $(LOCK)

release: ## Build the release binary from crates.io (needs network)
	$(CARGO) build --release $(LOCK)

release-offline: vendor-init ## Build the release binary from vendor/ with no network access
	$(CARGO) --config $(OFFLINE_CFG) build --release --offline $(LOCK)

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
