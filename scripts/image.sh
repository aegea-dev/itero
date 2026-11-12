#!/bin/sh
# Builds itero's two images for the Docker daemon's architecture:
# itero:dev (Debian) and itero:dev-alpine, from a static musl itero. On
# Linux with musl-gcc (musl-tools) it is built here; elsewhere (a Mac) in
# a rust container with musl-tools, as scripts/test-linux.sh builds. The
# release builds the same Dockerfiles from its archives' binaries
# (release.yml's image job). THIRD-PARTY-LICENSES comes from cargo-about
# when it is installed; else the file says this build has none, which
# only a dev image may.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
toolchain=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$root/rust-toolchain.toml")
docker_arch=$(docker version -f '{{.Server.Arch}}')
case "$docker_arch" in
amd64) arch=amd64 triple=x86_64-unknown-linux-musl ;;
arm64) arch=arm64 triple=aarch64-unknown-linux-musl ;;
*)
	echo "image.sh: no image for Docker's architecture $docker_arch" >&2
	exit 1
	;;
esac

if [ "$(uname -s)" = Linux ] && command -v musl-gcc >/dev/null 2>&1; then
	rustup target add "$triple" >/dev/null
	cargo build --release --target "$triple" --bin itero
	bin="${CARGO_TARGET_DIR:-$root/target}/$triple/release/itero"
else
	docker run --rm \
		-v "$root":/src -w /src \
		-v itero-cargo-registry:/usr/local/cargo/registry \
		-e CARGO_TARGET_DIR=/src/target/linux-musl \
		"rust:$toolchain" \
		sh -c 'apt-get -qq -o Acquire::Retries=3 update >/dev/null && apt-get -qq -o Acquire::Retries=3 install -y musl-tools >/dev/null &&
			rustup target add "$1" >/dev/null && cargo build --release --target "$1" --bin itero' sh "$triple"
	bin="$root/target/linux-musl/$triple/release/itero"
fi

context="$root/target/image"
rm -rf "$context" && mkdir -p "$context/bin/$arch"
cp "$bin" "$context/bin/$arch/itero"
if cargo about --version >/dev/null 2>&1; then
	cargo about generate --locked --fail --target "$triple" \
		-c "$root/packaging/licenses/about.toml" "$root/packaging/licenses/about.hbs" \
		-o "$context/bin/$arch/THIRD-PARTY-LICENSES"
else
	echo "A development image: no licence notices (cargo-about is not installed). Release images carry them." \
		>"$context/bin/$arch/THIRD-PARTY-LICENSES"
fi

docker build -f "$root/packaging/docker/Dockerfile" -t itero:dev "$context"
docker build -f "$root/packaging/docker/Dockerfile.alpine" -t itero:dev-alpine "$context"
