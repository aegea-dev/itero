#!/bin/sh
# Installs itero from a local "release" made by make dist, through the
# installer's test seam, into temp folders only, never on PATH. Checks: a
# good archive installs and runs; a corrupted one is refused before
# anything is written; uninstall removes what install put there.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tag=v0.0.0-deneme
fail() {
	echo "FAIL: $*" >&2
	exit 1
}

if [ "$(uname)" = Linux ]; then
	# the Linux release is the static musl build; a release from before it
	# has only the glibc one, which the script installs instead
	musl="$(uname -m)-unknown-linux-musl"
	old=v0.0.0-eski
	make -s -C "$root" dist TAG="$tag" TARGET="$musl" DIST="$tmp/rel/$tag" >/dev/null
	make -s -C "$root" dist TAG="$old" TARGET="$(uname -m)-unknown-linux-gnu" DIST="$tmp/rel/$old" >/dev/null
else
	make -s -C "$root" dist TAG="$tag" DIST="$tmp/rel/$tag" >/dev/null
fi

# a good install
if ! HOME="$tmp/ev" ITERO_INSTALL_BASE_URL="file://$tmp/rel" \
	sh "$root/docs/public/install" --version "$tag" --to "$tmp/bin" >"$tmp/out" 2>&1; then
	cat "$tmp/out"
	fail "install"
fi
"$tmp/bin/itero" --version | grep -q itero || fail "the installed itero does not run"
if [ "$(uname)" = Linux ]; then
	grep -qF "($musl)" "$tmp/out" || {
		cat "$tmp/out"
		fail "Linux did not get the musl build"
	}
	# a release with no musl build: its glibc one, said so
	if ! HOME="$tmp/ev4" ITERO_INSTALL_BASE_URL="file://$tmp/rel" \
		sh "$root/docs/public/install" --version "$old" --to "$tmp/bin4" >"$tmp/out4" 2>&1; then
		cat "$tmp/out4"
		fail "install of a release without a musl build"
	fi
	grep -q "has no musl build" "$tmp/out4" || {
		cat "$tmp/out4"
		fail "the glibc build was installed without a word"
	}
	"$tmp/bin4/itero" --version | grep -q itero || fail "the glibc itero does not run"
fi
# no service in this HOME: a first install restarts nothing
if grep -q "Restarted the service\|still runs the itero" "$tmp/out"; then
	cat "$tmp/out"
	fail "a first install touched a service"
fi
if [ "$(uname)" = Darwin ]; then
	[ -x "$tmp/ev/Applications/Itero.app/Contents/MacOS/itero-notifier" ] || fail "no Itero.app"
fi

# a corrupted archive is refused, and nothing is installed
cp -R "$tmp/rel" "$tmp/bozuk"
for a in "$tmp/bozuk/$tag"/*.tar.gz; do printf 'bozuk' >>"$a"; done
if HOME="$tmp/ev2" ITERO_INSTALL_BASE_URL="file://$tmp/bozuk" \
	sh "$root/docs/public/install" --version "$tag" --to "$tmp/bin2" >"$tmp/out2" 2>&1; then
	fail "a corrupted archive was installed"
fi
grep -q "checksum mismatch" "$tmp/out2" || {
	cat "$tmp/out2"
	fail "no checksum message"
}
[ ! -e "$tmp/bin2/itero" ] || fail "something was installed from a corrupted archive"
[ ! -e "$tmp/ev2/Applications/Itero.app" ] || fail "Itero.app was installed from a corrupted archive"

# uninstall
if ! HOME="$tmp/ev" sh "$root/docs/public/install" --uninstall --to "$tmp/bin" >"$tmp/out3" 2>&1; then
	cat "$tmp/out3"
	fail "uninstall"
fi
[ ! -e "$tmp/bin/itero" ] || fail "uninstall left the binary"
[ ! -e "$tmp/ev/Applications/Itero.app" ] || fail "uninstall left Itero.app"
echo "install tests: ok"
