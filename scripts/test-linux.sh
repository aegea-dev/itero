#!/bin/sh
# Runs the tests (and clippy) on Linux, in Docker, with the pinned Rust:
# what macOS cannot check, such as the inotify backend. Arguments go to
# `cargo test` (`sh scripts/test-linux.sh --test watch`). Builds into
# target/linux; the crate registry is cached in a Docker volume. jq and
# cmake are installed first: the beacon example's actions use jq, as on
# CI's runner, and cmake builds llama.cpp into itero-ai.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
toolchain=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$root/rust-toolchain.toml")

docker run --rm \
	-v "$root":/src -w /src \
	-v itero-cargo-registry:/usr/local/cargo/registry \
	-e CARGO_TARGET_DIR=/src/target/linux \
	"rust:$toolchain" \
	sh -c 'apt-get -qq update >/dev/null && apt-get -qq install -y jq cmake >/dev/null &&
		cargo test --workspace --no-fail-fast "$@" && cargo clippy --workspace --all-targets -- -D warnings' sh "$@"
