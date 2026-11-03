#!/bin/sh
# Checks the engine's model (model/) with TLC, in Docker: nothing to
# install but Docker. `check` runs each scenario (model/MC*.cfg) with its
# rules, reports every scenario that breaks one, then fails.
# tla2tools.jar is downloaded once into target/model/ and checked against
# the SHA-256 pinned here (GitHub publishes none for it); the Java image is
# pinned by its digest.
#
# Docker: the `docker` CLI as configured (set DOCKER_CONTEXT to pick one).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
version=1.7.4
sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
image=eclipse-temurin@sha256:0b99b3376fbb50dbf9c0af8535fdd5b8eb0d3fdfc8b1e4967f383b6919f6901a
jar=target/model/tla2tools-$version.jar

fetch() {
	[ -f "$root/$jar" ] && return 0
	mkdir -p "$root/target/model"
	curl -fsSL -o "$root/$jar.part" \
		"https://github.com/tlaplus/tlaplus/releases/download/v$version/tla2tools.jar"
	got=$( (shasum -a 256 "$root/$jar.part" 2>/dev/null || sha256sum "$root/$jar.part") | cut -d' ' -f1)
	if [ "$got" != "$sha256" ]; then
		rm -f "$root/$jar.part"
		echo "tla2tools.jar $version: SHA-256 $got, not $sha256" >&2
		exit 1
	fi
	mv "$root/$jar.part" "$root/$jar"
}

# tlc SPEC CONFIG: TLC on model/SPEC with CONFIG (a path from the repo's
# root); its states stay inside the container. No deadlock check: a
# scenario that has run its course stops, and the liveness rules say
# whether it stopped where it should.
tlc() {
	docker run --rm -v "$root":/src -w /src/model "$image" \
		java -XX:+UseParallelGC -cp "/src/$jar" tlc2.TLC \
		-workers auto -deadlock -metadir /tmp/tlc \
		-config "/src/$2" "$1"
}

check() {
	failed=
	for cfg in "$root"/model/MC*.cfg; do
		s=$(basename "$cfg" .cfg)
		echo "== $s"
		tlc "$s.tla" "model/$s.cfg" || failed="$failed $s"
	done
	if [ -n "$failed" ]; then
		echo "rules broken in:$failed" >&2
		exit 1
	fi
}

case "${1:-}" in
check) fetch && check ;;
*)
	echo "usage: sh scripts/model.sh check" >&2
	exit 2
	;;
esac
