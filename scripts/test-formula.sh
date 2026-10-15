#!/bin/sh
# Installs the Homebrew formula from a local tap, with a local "release"
# made by make dist, then removes both. macOS only. It changes Homebrew
# while it runs (a tap named itero-test/local and the itero formula), and
# undoes that on the way out, failed or not.
set -eu
[ "$(uname)" = Darwin ] || {
	echo "test-formula: macOS only" >&2
	exit 1
}
command -v brew >/dev/null || {
	echo "test-formula: no brew" >&2
	exit 1
}
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
tap=itero-test/local
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)
tag="v$version"
case "$(uname -m)" in
arm64) target=aarch64-apple-darwin ;;
*) target=x86_64-apple-darwin ;;
esac
cleanup() {
	brew uninstall --formula "$tap/itero" >/dev/null 2>&1 || true
	brew untap "$tap" >/dev/null 2>&1 || true
	rm -rf "$tmp"
}
trap cleanup EXIT INT TERM
fail() {
	echo "FAIL: $*" >&2
	exit 1
}

make -s -C "$root" dist TAG="$tag" TARGET="$target" DIST="$tmp/rel" >/dev/null
archive="$tmp/rel/itero-$tag-$target.tar.gz"
sum=$(awk '{print $1}' "$tmp/rel/itero-$tag-$target.sha256")

brew tap-new --no-git "$tap" >/dev/null
dir="$(brew --repository "$tap")/Formula"
# this machine's macOS url and checksum point at the local archive
TARGET="$target" URL="file://$archive" SUM="$sum" ruby -e '
  s = File.read(ARGV[0])
  url = s[/^\s*url "[^"]*#{ENV["TARGET"]}\.tar\.gz"/] or abort "no url for #{ENV["TARGET"]}"
  at = s.index(url)
  s[at, url.length] = url.sub(/"[^"]*"/, "\"#{ENV["URL"]}\"")
  rest = s[at..]
  rest =~ /sha256 "0+"/ or abort "no sha256 after it"
  s[at..] = rest.sub(/sha256 "0+"/, "sha256 \"#{ENV["SUM"]}\"")
  File.write(ARGV[1], s)
' "$root/packaging/homebrew/itero.rb" "$dir/itero.rb"

HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_FROM_API=1 brew install --formula "$tap/itero" >"$tmp/out" 2>&1 || {
	cat "$tmp/out"
	fail "brew install"
}
prefix=$(brew --prefix "$tap/itero")
"$(brew --prefix)/bin/itero" --version | grep -q "itero $version" || fail "the installed itero does not run"
[ -x "$prefix/libexec/Itero.app/Contents/MacOS/itero-notifier" ] || fail "no Itero.app in libexec"
brew test "$tap/itero" >/dev/null 2>&1 || fail "brew test"
echo "formula test: ok"
