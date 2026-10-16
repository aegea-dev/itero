#!/bin/sh
# Runs the tests (and clippy) on Linux, in Docker, with the pinned Rust:
# what macOS cannot check, such as the inotify backend. Arguments go to
# `cargo test` (`sh scripts/test-linux.sh --test watch`). Builds into
# target/linux; the crate registry is cached in a Docker volume.
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
	sh -c 'cargo test --no-fail-fast "$@" && cargo clippy --all-targets -- -D warnings' sh "$@"
