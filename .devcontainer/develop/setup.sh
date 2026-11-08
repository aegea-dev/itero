#!/bin/sh
# Once, when the container is made: cmake and libclang build itero-ai's
# llama.cpp, jq and Chromium are for the tests; the pinned Rust is
# installed (rust-toolchain.toml), and Yarn for the docs. Nothing is built.
set -eu
sudo apt-get update -q
sudo apt-get install -y -q --no-install-recommends cmake libclang-dev jq chromium
rustup toolchain install
# the Node feature's Node, through nvm, is not on a plain sh's PATH
PATH="/usr/local/share/nvm/current/bin:$PATH"
corepack enable
cargo --version
node --version
