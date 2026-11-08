#!/bin/sh
# Once, when the container is made: what the examples need (jq; Chromium
# for #browser, headless; Python and Bun for actions in those languages),
# then the latest itero, by the line the docs give. Both are linked into
# /usr/local/bin, so any user finds them (CodeSandbox runs as root).
set -eu
sudo apt-get update -q
sudo apt-get install -y -q --no-install-recommends jq chromium python3 unzip
curl -fsSL https://bun.sh/install | bash
curl -fsSL https://itero.sh/install | sh
sudo ln -sf "$HOME/.local/bin/itero" /usr/local/bin/itero
sudo ln -sf "$HOME/.bun/bin/bun" /usr/local/bin/bun
itero --version
