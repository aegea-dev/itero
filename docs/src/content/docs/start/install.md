---
title: Install
description: Install itero with Homebrew, the install script, or from source.
---

itero runs on macOS (Apple Silicon and Intel) and Linux (x86_64 and
aarch64) with glibc 2.34 or later: Ubuntu 22.04, Debian 12,
RHEL and Rocky 9, Fedora 36 and later. Alpine (musl) is not supported.

## Homebrew

```bash
brew trust aegea-dev/tap
brew install aegea-dev/tap/itero
```

Homebrew 7 and later load a tap's formulae only once you trust the tap:
without `brew trust`, the install stops with "Refusing to load formula
aegea-dev/tap/itero from untrusted tap". The formula puts `Itero.app`, the macOS notifier, in its own folder, where
itero finds it.

## The install script

```bash
curl -fsSL https://itero.sh/install | sh
```

The script downloads a release, checks the archive's SHA-256 before
unpacking it, installs `itero` to `~/.local/bin` and, on macOS, `Itero.app`
to `~/Applications`. It never edits your shell profile: if `~/.local/bin` is
not on your `PATH`, it says so.

| Option | Does |
|---|---|
| `--version X.Y.Z` | installs that release instead of the latest |
| `--to DIR` | installs `itero` to `DIR` |
| `--uninstall` | removes what it installed |

Pass options after `sh -s --`:

```bash
curl -fsSL https://itero.sh/install | sh -s -- --version 0.8.0
```

## From source

With a Rust toolchain:

```bash
git clone https://github.com/aegea-dev/itero
cd itero
make install
```

`make install` installs `itero` to `~/.cargo/bin` and, on macOS, builds
`Itero.app` and puts it in `~/Applications`.

## Notifications

On macOS, notifications come from `Itero.app`, which asks once to be
allowed to show them and then has its own entry in System Settings,
Notifications. itero keeps its own copy of it in
`~/Library/Application Support/itero/`, brought up to date as you
upgrade, so the permission stays. Without the app they go through
`osascript` and show as Script Editor's.

On Linux, itero uses the first of `notify-send` (libnotify), `gdbus` (the
freedesktop notification service) and `kdialog` (KDE) that it finds.

## Upgrading

Upgrade the files the way you installed them:

```bash
brew upgrade aegea-dev/tap/itero   # Homebrew; add aegea-dev/tap/itero-ai if you have it
curl -fsSL https://itero.sh/install | sh   # the install script
git pull && make install           # from source
```

A daemon that is already running keeps the itero it started with: the
new files change nothing until it starts again. If you run itero as a
service ([Start at login](/guides/service/)), restart it:

```bash
itero service restart
```

It stops the daemon and the itero-ai it started, and starts the daemon
again on the new version; itero-ai starts again, new too, with the next
`#ai` step. A run under way goes on where it was. Without the
service, stop `itero daemon` (Ctrl-C) and start it again.

Check that the daemon runs the new version: in a workspace,
[`itero info`](/commands/info/) shows the running daemon under `daemon`,
with its `itero` (the workspace's own `itero` is the one that made it).

A release that changes how workflows or actions are written says so in
its notes, under **Breaking**; run [`itero check`](/commands/check/) in
each workspace after upgrading, and it names what to change.

## Check it

```bash
itero --version
```

Next: the [quickstart](/start/quickstart/).
