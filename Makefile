.DEFAULT_GOAL := help

# Every Rust command goes through the toolchain pinned in rust-toolchain.toml,
# never a bare `cargo`. The channel is read from that file so the version lives
# in exactly one place.
#
# `cargo +<toolchain>`, so a non-shim cargo first on PATH fails loudly instead
# of silently building with the wrong compiler.
TOOLCHAIN := $(shell sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
CARGO := cargo +$(TOOLCHAIN)

.PHONY: help
help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-16s\033[0m %s\n", $$1, $$2}'

.PHONY: build
build: ## Build the release binary (target/release/itero)
	$(CARGO) build --release

.PHONY: build-dev
build-dev: ## Build the debug binary (target/debug/itero)
	$(CARGO) build

# --no-fail-fast: one failing test binary must not hide the rest.
.PHONY: test
test: ## Run the full test suite
	$(CARGO) test --no-fail-fast

# rustfmt only visits files reachable through `mod`, so `cargo fmt` silently
# skips a file not yet wired in. Handing every source file to rustfmt closes
# that blind spot.
RUSTFMT := rustup run $(TOOLCHAIN) rustfmt --edition 2024
RUST_SOURCES = find src tests -name '*.rs'

.PHONY: fmt
fmt: ## Format all Rust code
	$(RUST_SOURCES) | xargs $(RUSTFMT)

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying files
	$(RUST_SOURCES) | xargs $(RUSTFMT) --check

.PHONY: lint
lint: ## Run clippy (lists warnings, does not fail on them)
	$(CARGO) clippy --all-targets

.PHONY: lint-check
lint-check: ## Run clippy: any warning fails the build
	$(CARGO) clippy --all-targets -- -D warnings

# Itero.app around itero-notifier, next to the itero binary, where `itero
# worker` looks for it: macOS shows notifications with a sender only when an
# app sends them. Signed ad hoc (no developer account needed on this Mac).
PROFILE ?= debug
APP = target/$(PROFILE)/Itero.app
.PHONY: notifier
notifier: ## Build Itero.app (macOS), which notify@1 shows notifications through
	$(CARGO) build --bin itero-notifier $(if $(filter release,$(PROFILE)),--release,)
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS
	cp packaging/macos/Info.plist $(APP)/Contents/Info.plist
	cp target/$(PROFILE)/itero-notifier $(APP)/Contents/MacOS/itero-notifier
	codesign --force --sign - $(APP)

.PHONY: install
install: ## Install itero to ~/.cargo/bin, and on macOS Itero.app to ~/Applications
	$(CARGO) install --path . --force --bin itero
	@if [ "$$(uname)" = Darwin ]; then \
	  $(MAKE) notifier PROFILE=release && \
	  mkdir -p $(HOME)/Applications && rm -rf $(HOME)/Applications/Itero.app && \
	  cp -R target/release/Itero.app $(HOME)/Applications/Itero.app && \
	  echo "Itero.app in ~/Applications"; \
	fi

.PHONY: clean
clean: ## Remove build artifacts (cargo clean)
	$(CARGO) clean
