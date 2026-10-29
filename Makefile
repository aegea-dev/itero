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
	$(CARGO) test --workspace --no-fail-fast

# rustfmt only visits files reachable through `mod`, so `cargo fmt` silently
# skips a file not yet wired in. Handing every source file to rustfmt closes
# that blind spot.
RUSTFMT := rustup run $(TOOLCHAIN) rustfmt --edition 2024
RUST_SOURCES = find src tests itero-ai-index -name '*.rs'

.PHONY: fmt
fmt: ## Format all Rust code
	$(RUST_SOURCES) | xargs $(RUSTFMT)

.PHONY: fmt-check
fmt-check: ## Check formatting without modifying files
	$(RUST_SOURCES) | xargs $(RUSTFMT) --check

.PHONY: lint
lint: ## Run clippy (lists warnings, does not fail on them)
	$(CARGO) clippy --workspace --all-targets

.PHONY: lint-check
lint-check: ## Run clippy: any warning fails the build
	$(CARGO) clippy --workspace --all-targets -- -D warnings

.PHONY: check
check: ## Type-check every target (the binary, tests), without building them
	$(CARGO) check --workspace --all-targets

# Cargo.lock against the RustSec advisory database. Needs cargo-audit
# (`cargo install cargo-audit --locked`); deliberate exceptions, each with its
# reason, go in .cargo/audit.toml.
.PHONY: audit
audit: ## Check Cargo.lock for known vulnerabilities (cargo-audit)
	$(CARGO) audit

.PHONY: verify
verify: fmt-check check lint-check test audit docs-build ## Every gate CI runs before a release: formatting, check, clippy, tests, audit, docs

.PHONY: docs
docs: ## Serve the docs site at http://localhost:4321/itero/ with live reload
	yarn --cwd docs dev

.PHONY: docs-install
docs-install: ## Install the docs site's dependencies (yarn 4, run once)
	yarn --cwd docs install --immutable

.PHONY: docs-build
docs-build: ## Build the docs site into docs/dist, and check its local assets
	yarn --cwd docs build

# Itero.app around itero-notifier, next to the itero binary, where `itero
# worker` looks for it: macOS shows notifications with a sender only when an
# app sends them. Signed ad hoc (no developer account needed on this Mac).
PROFILE ?= debug
APP = target/$(PROFILE)/Itero.app
.PHONY: notifier
notifier: ## Build Itero.app (macOS), which notify@1 shows notifications through
	$(CARGO) build --bin itero-notifier $(if $(filter release,$(PROFILE)),--release,)
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources
	cp packaging/macos/Info.plist $(APP)/Contents/Info.plist
	cp packaging/macos/Itero.icns $(APP)/Contents/Resources/Itero.icns
	cp target/$(PROFILE)/itero-notifier $(APP)/Contents/MacOS/itero-notifier
	codesign --force --sign - $(APP)

# Itero.icns from packaging/macos/itero.svg, every size macOS asks for
# (needs rsvg-convert, from librsvg); the .icns is kept in git.
ICONSET = target/Itero.iconset
.PHONY: icon
icon: ## Build packaging/macos/Itero.icns from itero.svg (macOS, rsvg-convert)
	rm -rf $(ICONSET) && mkdir -p $(ICONSET)
	for s in 16 32 128 256 512; do \
	  rsvg-convert -w $$s -h $$s packaging/macos/itero.svg -o $(ICONSET)/icon_$${s}x$${s}.png && \
	  rsvg-convert -w $$((s * 2)) -h $$((s * 2)) packaging/macos/itero.svg -o $(ICONSET)/icon_$${s}x$${s}@2x.png; \
	done
	iconutil -c icns $(ICONSET) -o packaging/macos/Itero.icns

# A release archive for one target (this machine's unless TARGET is given),
# and its checksum: what docs/public/install downloads and release.yml publishes.
# Always built with --target, so a host build and a cross build are one
# recipe. TAG names the release. The archive also holds THIRD-PARTY-LICENSES,
# the notices of the crates in the binary, made by cargo-about
# (`cargo install cargo-about --version 0.9.2 --features cli --locked`).
TAG ?= v$(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
TARGET_DIR ?= $(or $(CARGO_TARGET_DIR),target)
DIST ?= dist
NAME = itero-$(TAG)-$(TARGET)
OUT = $(TARGET_DIR)/$(TARGET)/release
.PHONY: dist
dist: ## Build the release archive and checksum (TAG=vX.Y.Z, TARGET=triple)
	$(CARGO) build --release --target $(TARGET) --bin itero
	rm -rf $(DIST)/stage && mkdir -p $(DIST)/stage
	cp $(OUT)/itero $(DIST)/stage/itero
	$(CARGO) about generate --locked --fail --target $(TARGET) \
	  -c packaging/licenses/about.toml packaging/licenses/about.hbs \
	  -o $(DIST)/stage/THIRD-PARTY-LICENSES
	@case "$(TARGET)" in *-apple-darwin) \
	  $(CARGO) build --release --target $(TARGET) --bin itero-notifier && \
	  mkdir -p $(DIST)/stage/Itero.app/Contents/MacOS $(DIST)/stage/Itero.app/Contents/Resources && \
	  cp packaging/macos/Info.plist $(DIST)/stage/Itero.app/Contents/Info.plist && \
	  cp packaging/macos/Itero.icns $(DIST)/stage/Itero.app/Contents/Resources/Itero.icns && \
	  cp $(OUT)/itero-notifier $(DIST)/stage/Itero.app/Contents/MacOS/itero-notifier && \
	  codesign --force --sign - $(DIST)/stage/Itero.app ;; \
	esac
	tar czf $(DIST)/$(NAME).tar.gz -C $(DIST)/stage .
	cd $(DIST) && (shasum -a 256 $(NAME).tar.gz 2>/dev/null || sha256sum $(NAME).tar.gz) > $(NAME).sha256
	rm -rf $(DIST)/stage

.PHONY: test-install
test-install: ## Install from a local release through the install script, in temp folders
	sh scripts/test-install.sh

.PHONY: test-linux
test-linux: ## Run the tests and clippy on Linux, in Docker
	sh scripts/test-linux.sh

.PHONY: test-docker
test-docker: ## Run the tests that need Docker: container actions, IMAP and POP3
	sh scripts/test-docker.sh

.PHONY: test-formula
test-formula: ## Install the formula from a local tap (macOS; changes Homebrew, undone after)
	sh scripts/test-formula.sh

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
