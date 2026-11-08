#!/bin/sh
# Inside a dev container (`make test-devcontainer`): what it promises is
# there. try: its tools, and a workflow run to its end that opens a page in
# Chromium, reads it and notifies; develop: the pinned Rust and the tools.
set -eu
case "$1" in
try)
	itero --version
	jq --version
	chromium --version
	python3 --version
	bun --version
	d=$(mktemp -d)
	trap 'kill "$srv" 2>/dev/null || true; rm -rf "$d"' EXIT
	mkdir "$d/site" "$d/workflows"
	printf '<title>Deneme</title><h1>Merhaba dünya</h1>\n' >"$d/site/index.html"
	python3 -m http.server 8765 --bind 127.0.0.1 --directory "$d/site" >/dev/null 2>&1 &
	srv=$!
	n=0
	until curl -fs http://127.0.0.1:8765/ >/dev/null; do
		n=$((n + 1)); [ "$n" -lt 100 ] || { echo "the page server did not start" >&2; exit 1; }
		sleep 0.1
	done
	echo '{}' >"$d/itero.config.json"
	cat >"$d/workflows/dene.json" <<'JSON'
{ "on": [{ "event": "dene.istendi" }],
  "steps": [{ "#browser": { "args": { "open": "http://127.0.0.1:8765/" } } },
            { "#browser": { "args": { "read": "text" } } },
            { "#notify": { "args": { "title": "deneme", "text": "sayfa okundu" } } }] }
JSON
	cd "$d"
	itero ws emit dene.istendi '{}'
	itero daemon --once
	grep -q "sayfa okundu" "$ITERO_NOTIFY_LOG" || {
		echo "the workflow did not reach its notification:" >&2
		itero ws wf runs >&2 || true
		tail -20 .itero/itero.log >&2 || true
		exit 1
	}
	echo "try: a workflow opened a page in Chromium, read it and notified"
	;;
develop)
	want=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
	rustc --version | grep -q " $want " || { echo "rustc is not $want: $(rustc --version)" >&2; exit 1; }
	cargo clippy --version
	cmake --version | head -1
	chromium --version
	jq --version
	PATH="/usr/local/share/nvm/current/bin:$PATH"
	[ "$(node --version)" = "v$(cat docs/.node-version)" ] || { echo "node is not docs/.node-version's: $(node --version)" >&2; exit 1; }
	(cd docs && yarn --version)
	echo "develop: Rust $want and the tools are there"
	;;
*)
	echo "usage: devcontainer-check.sh try|develop" >&2
	exit 2
	;;
esac
