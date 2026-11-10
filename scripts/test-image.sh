#!/bin/sh
# Builds the two images (scripts/image.sh) and runs each as a person
# would, with docs/public/container/compose.yaml itself: the first token
# made before the first start, the server up, tests/image/selam applied
# and run from this machine's itero over a context, the probes, the user,
# the Debian image's tools, and a stop. Containers, volumes and temporary
# folders are removed afterwards, whatever happened. Every wait has a
# time limit.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
sh "$root/scripts/image.sh"
cargo build --bin itero
cli="${CARGO_TARGET_DIR:-$root/target}/debug/itero"

work=$(mktemp -d)
project=itero-image-test
port=18443
dc() { docker compose -p "$project" -f "$work/compose.yaml" "$@"; }
cleanup() {
	dc down -v >/dev/null 2>&1 || true
	rm -rf "$work"
}
trap cleanup EXIT INT TERM
cp "$root/docs/public/container/compose.yaml" "$root/docs/public/container/server.json" "$work/"

fail() {
	echo "test-image: $*" >&2
	dc logs >&2 || true
	exit 1
}

# waits up to 60 s for $1 to answer 200
ready() {
	i=0
	until [ "$(curl -s -o /dev/null -w '%{http_code}' "$1")" = 200 ]; do
		i=$((i + 1))
		[ "$i" -le 120 ] || fail "$1 did not answer 200 in 60 s"
		sleep 0.5
	done
}

for image in itero:dev itero:dev-alpine; do
	echo "== $image"
	export ITERO_IMAGE="$image" ITERO_PORT="127.0.0.1:$port"
	api="http://127.0.0.1:$port"

	# the first token, before the first start
	token=$(dc run --rm -T itero token create sınama | tail -n 1)
	case "$token" in itero_*) ;; *) fail "no token: $token" ;; esac
	dc up -d
	ready "$api/readyz"
	[ "$(curl -s "$api/livez")" = ok ] || fail "/livez"
	[ "$(curl -s -o /dev/null -w '%{http_code}' "$api/health")" = 401 ] || fail "/health without a token"

	# from this machine: a context, the workspace applied, an event run
	home="$work/home-$(echo "$image" | tr ':' '-')"
	mkdir -p "$home"
	(
		export HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_STATE_HOME="$home/state" ITERO_NO_SECRET_STORE=1
		ITERO_TOKEN="$token" "$cli" context add kutu "$api" --ws selam
		cd "$root/tests/image/selam"
		"$cli" ws apply --context kutu
		answer=$("$cli" ws emit --wait --timeout 60s selam.geldi '{"ad": "ışık"}' --context kutu)
		echo "$answer" | jq -e '.runs[0].status == "done" and .runs[0].state.output == "merhaba ışık"' >/dev/null ||
			{
				echo "$answer" >&2
				exit 1
			}
	) || fail "the workspace did not run"

	[ "$(dc exec -T itero id -u)" = 10001 ] || fail "not user 10001"
	if [ "$image" = itero:dev ]; then
		# what they say only when one fails: Chromium's D-Bus complaints are noise
		said=$(dc exec -T itero sh -c 'python3 --version && magick -version && git --version &&
			chromium --headless --no-sandbox --dump-dom about:blank' 2>&1) ||
			fail "the Debian image's tools: $said"
	fi

	# a stop is SIGTERM: done well within its grace, exit 0
	start=$(date +%s)
	dc stop itero
	took=$(($(date +%s) - start))
	[ "$took" -le 10 ] || fail "the stop took ${took}s"
	code=$(docker inspect -f '{{.State.ExitCode}}' "$(dc ps -aq itero)")
	[ "$code" = 0 ] || fail "it ended with $code"

	dc down -v >/dev/null
done
echo "test-image: both images passed"
