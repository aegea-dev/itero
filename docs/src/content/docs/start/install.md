---
title: Install
description: Install itero with Homebrew, the install script, or from source.
---

itero runs on macOS (Apple Silicon and Intel) and Linux (x86_64 and
aarch64).

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
curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
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
curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh -s -- --version 0.1.0
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
Notifications. Without the app they go through `osascript` and show as
Script Editor's.

On Linux, itero uses the first of `notify-send` (libnotify), `gdbus` (the
freedesktop notification service) and `kdialog` (KDE) that it finds.

## Check it

```bash
itero --version
```

Next: the [quickstart](/start/quickstart/).
