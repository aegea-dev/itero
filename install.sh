#!/bin/sh
#
# itero installer.
#
#   curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --version 0.2.0
#   curl -fsSL .../install.sh | sh -s -- --uninstall
#
# POSIX sh on purpose, so `| sh` and `| bash` both work. Nothing here reads
# stdin, because stdin is the script itself when this is piped.
#
# Tested by scripts/test-install.sh, which installs from an archive made by
# `make dist`, the same recipe the release workflow publishes with: that is
# what keeps the names below and the release's names one and the same.

set -eu

# The repository the releases come from: the one line to change when it moves.
REPO="aegea-dev/itero"

# Overridable only so the test suite can install from a local release and
# serve a deliberately corrupted archive, proving the checksum check fires.
# A test seam, not a feature: not documented, not in the flag parser.
BASE_URL="${ITERO_INSTALL_BASE_URL:-https://github.com/$REPO/releases/download}"

DEST="$HOME/.local/bin"
APPS="$HOME/Applications"
WANT_VERSION=""
DO_UNINSTALL=0

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
err() {
	printf 'error: %s\n' "$*" >&2
	exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
	cat <<'EOF'
itero installer

USAGE
    curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
    curl -fsSL .../install.sh | sh -s -- [OPTIONS]

OPTIONS
    --version X.Y.Z   Install a specific release instead of the latest
    --to DIR          Install into DIR instead of ~/.local/bin
    --uninstall       Remove what this script installed
    --help            Show this message

The archive is checked against the .sha256 published with each release
before it is unpacked. Nothing is downloaded from anywhere but GitHub
Releases, and your shell profile is never changed. On macOS, Itero.app goes
to ~/Applications: notifications then come from itero.
EOF
}

# ---------------------------------------------------------------- target

# Prints a Rust target triple, or exits with a message naming the reason.
detect_target() {
	os="$(uname -s)"
	arch="$(uname -m)"
	case "$os" in
	Darwin)
		# A shell under Rosetta 2 on Apple Silicon reports x86_64; the native
		# build is the better one there.
		if [ "$arch" = "x86_64" ] &&
			[ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
			arch="arm64"
		fi
		case "$arch" in
		arm64 | aarch64) printf 'aarch64-apple-darwin\n' ;;
		x86_64) printf 'x86_64-apple-darwin\n' ;;
		*) err "unsupported architecture on macOS: $arch" ;;
		esac
		;;
	Linux)
		case "$arch" in
		x86_64 | amd64) printf 'x86_64-unknown-linux-gnu\n' ;;
		aarch64 | arm64) printf 'aarch64-unknown-linux-gnu\n' ;;
		*) err "unsupported architecture on Linux: $arch (itero publishes x86_64 and aarch64)" ;;
		esac
		;;
	*) err "unsupported operating system: $os (itero publishes macOS and Linux builds)" ;;
	esac
}

# ---------------------------------------------------------------- download

pick_downloader() {
	if have curl; then
		DL="curl"
	elif have wget; then
		DL="wget"
	else
		err "neither curl nor wget is available; install one and try again"
	fi
}

# Sets HTTP_CODE, so "that release does not exist" reads differently from
# "the network is down".
HTTP_CODE=""
fetch() {
	# $1 url, $2 destination path
	if [ "$DL" = "curl" ]; then
		HTTP_CODE="$(curl -sL -o "$2" -w '%{http_code}' "$1" 2>/dev/null || printf '000\n')"
		case "$HTTP_CODE" in
		200) return 0 ;;
		# file:// reports 000, and only the test seam uses it
		000)
			[ -s "$2" ] && return 0
			return 1
			;;
		*) return 1 ;;
		esac
	else
		HTTP_CODE=""
		wget -q -O "$2" "$1" 2>/dev/null
	fi
}

# The newest tag, from the /releases/latest redirect: not api.github.com,
# which is rate limited and would need a JSON parser.
latest_tag() {
	url="https://github.com/$REPO/releases/latest"
	if [ "$DL" = "curl" ]; then
		final="$(curl -sSfL -o /dev/null -w '%{url_effective}' "$url")"
	else
		final="$(wget -q -S --spider "$url" 2>&1 |
			awk '/[Ll]ocation:/ { print $2 }' | tail -n 1)"
	fi
	tag="${final##*/}"
	case "$tag" in
	v[0-9]*) printf '%s\n' "$tag" ;;
	*) err "could not work out the latest version from $url" ;;
	esac
}

# ---------------------------------------------------------------- verify

# Checked before anything is downloaded: a machine that cannot verify is
# told at once, and nothing unverified is ever installed.
require_checksum_tool() {
	if have sha256sum; then
		SUM="sha256sum"
	elif have shasum; then
		SUM="shasum"
	else
		err "no sha256sum or shasum found, so the download cannot be verified.

Install one (coreutils on Linux) and run this again. itero will not install
an unverified archive."
	fi
}

sha256_of() {
	if [ "$SUM" = "sha256sum" ]; then
		sha256sum "$1" | awk '{print $1}'
	else
		shasum -a 256 "$1" | awk '{print $1}'
	fi
}

verify() {
	# $1 archive, $2 .sha256 file
	expected="$(awk '{print $1}' "$2" | head -n 1)"
	actual="$(sha256_of "$1")"
	[ -n "$expected" ] || err "the published checksum file was empty or malformed"
	if [ "$expected" != "$actual" ]; then
		err "checksum mismatch, refusing to install.

  expected  $expected
  actual    $actual

The download was corrupted in transit, or it is not the file the release
published. Nothing has been installed."
	fi
}

# ---------------------------------------------------------------- provenance

# Who owns a copy of itero: cargo, brew, or none (this script, or by hand).
owner_of() {
	p="$1"
	if [ -L "$p" ] && have readlink; then
		t="$(readlink "$p" 2>/dev/null || true)"
		case "$t" in
		/*) p="$t" ;;
		?*) p="$(dirname "$p")/$t" ;;
		esac
	fi
	case "$p" in
	"$HOME"/.cargo/bin/*)
		printf 'cargo\n'
		return
		;;
	/opt/homebrew/* | /usr/local/Cellar/* | /home/linuxbrew/.linuxbrew/*)
		printf 'brew\n'
		return
		;;
	esac
	printf 'none\n'
}

remove_command_for() {
	case "$1" in
	cargo) printf 'cargo uninstall itero\n' ;;
	brew) printf 'brew uninstall itero\n' ;;
	*) printf '\n' ;;
	esac
}

# Every executable itero on PATH, in PATH order.
copies_on_path() (
	IFS=:
	for d in $PATH; do
		[ -n "$d" ] || d="."
		[ -x "$d/itero" ] && printf '%s\n' "$d/itero"
	done
	true
)

# An older copy earlier on PATH keeps winning, and nothing looks wrong.
check_shadowing() {
	installed="$1"
	first="$(copies_on_path | head -n 1)"
	[ -n "$first" ] || return 0
	[ "$first" != "$installed" ] || return 0
	other_version="$("$first" --version 2>/dev/null || printf 'unknown\n')"
	warn "another itero comes first on your PATH and will keep winning:

  $first  ($other_version)
  $installed  (just installed)"
	cmd="$(remove_command_for "$(owner_of "$first")")"
	if [ -n "$cmd" ]; then
		say "  To remove it:  $cmd"
	else
		say "  Remove it, or put $DEST earlier on your PATH."
	fi
}

# ---------------------------------------------------------------- PATH advice

profile_for_shell() {
	case "${SHELL:-}" in
	*/fish) printf '%s\n' "~/.config/fish/config.fish" ;;
	*/zsh) printf '%s\n' "~/.zshrc" ;;
	*/bash) printf '%s\n' "~/.bashrc" ;;
	*) printf '%s\n' "your shell profile" ;;
	esac
}

path_line_for_shell() {
	case "${SHELL:-}" in
	*/fish) printf '%s\n' "fish_add_path $DEST" ;;
	*) printf '%s\n' "export PATH=\"$DEST:\$PATH\"" ;;
	esac
}

# Advice only: an installer never edits a dotfile.
check_on_path() {
	case ":$PATH:" in
	*":$DEST:"*) return 0 ;;
	esac
	say ""
	say "$DEST is not on your PATH. Add it to $(profile_for_shell):"
	say ""
	say "    $(path_line_for_shell)"
}

# ---------------------------------------------------------------- uninstall

uninstall() {
	target="$DEST/itero"
	removed=0
	if [ -e "$target" ]; then
		owner="$(owner_of "$target")"
		if [ "$owner" != "none" ]; then
			err "$target belongs to $owner, not to this installer.

Removing it directly would leave $owner thinking itero is still installed.
Use its own command instead:

    $(remove_command_for "$owner")"
		fi
		rm -f "$target"
		say "Removed $target"
		removed=1
	fi
	if [ -d "$APPS/Itero.app" ]; then
		rm -rf "$APPS/Itero.app"
		say "Removed $APPS/Itero.app"
		removed=1
	fi
	if [ "$removed" = "0" ]; then
		say "No itero installed at $target."
		exit 1
	fi
	say ""
	say "Left in place: each workspace's .itero folder (its journal and the"
	say "workflows' memory). They are your data; this installer does not know"
	say "where your workspaces are. Delete a workspace's .itero to forget it."
	exit 0
}

# ---------------------------------------------------------------- main

main() {
	while [ $# -gt 0 ]; do
		case "$1" in
		--help | -h)
			usage
			exit 0
			;;
		--uninstall)
			DO_UNINSTALL=1
			shift
			;;
		--version)
			[ $# -ge 2 ] || err "--version needs a value, for example --version 0.2.0"
			WANT_VERSION="$2"
			shift 2
			;;
		--to)
			[ $# -ge 2 ] || err "--to needs a directory"
			DEST="$2"
			shift 2
			;;
		*) err "unknown option: $1 (try --help)" ;;
		esac
	done

	if [ "$DO_UNINSTALL" = "1" ]; then
		uninstall
	fi

	pick_downloader
	require_checksum_tool
	target="$(detect_target)"

	if [ -n "$WANT_VERSION" ]; then
		case "$WANT_VERSION" in
		v*) tag="$WANT_VERSION" ;;
		*) tag="v$WANT_VERSION" ;;
		esac
	else
		tag="$(latest_tag)"
	fi

	archive="itero-$tag-$target.tar.gz"
	# the checksum is named after the archive's stem, not "<archive>.sha256"
	sums="itero-$tag-$target.sha256"
	url="$BASE_URL/$tag/$archive"
	sums_url="$BASE_URL/$tag/$sums"

	tmp="$(mktemp -d)"
	# shellcheck disable=SC2064
	trap "rm -rf '$tmp'" EXIT INT TERM

	say "Installing itero $tag ($target)"

	if ! fetch "$url" "$tmp/$archive"; then
		if [ "$HTTP_CODE" = "404" ]; then
			err "no such release: $tag

Check the version you asked for against the published releases:
  https://github.com/$REPO/releases"
		fi
		err "could not download $url ${HTTP_CODE:+(HTTP $HTTP_CODE)}

Check your network connection and try again."
	fi
	if ! fetch "$sums_url" "$tmp/$sums"; then
		err "downloaded the archive but not its checksum, so it cannot be verified"
	fi

	# Verify before unpacking: the order is the security property.
	verify "$tmp/$archive" "$tmp/$sums"

	mkdir "$tmp/x"
	tar xzf "$tmp/$archive" -C "$tmp/x"
	[ -f "$tmp/x/itero" ] || err "the archive did not contain an itero binary"

	mkdir -p "$DEST" || err "could not create $DEST"
	if ! mv "$tmp/x/itero" "$DEST/itero" 2>/dev/null; then
		err "could not write to $DEST. Choose somewhere else with --to DIR"
	fi
	chmod 755 "$DEST/itero"
	say "Installed $("$DEST/itero" --version 2>/dev/null || echo itero) to $DEST/itero"

	# Fetched with curl or wget, the app has no quarantine attribute, so
	# Gatekeeper does not stop its ad hoc signature.
	if [ -d "$tmp/x/Itero.app" ]; then
		mkdir -p "$APPS" || err "could not create $APPS"
		rm -rf "$APPS/Itero.app"
		mv "$tmp/x/Itero.app" "$APPS/Itero.app"
		say "Installed Itero.app to $APPS: notifications come from itero (macOS asks once to allow them)"
	fi

	check_on_path
	check_shadowing "$DEST/itero"

	say ""
	say "Next:  mkdir my-workflows && cd my-workflows && echo '{}' > itero.config.json && itero daemon"
	say "       and to start it at login, in that folder:  itero service install"
}

main "$@"
