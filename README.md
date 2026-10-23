# itero: your domain, your solution, your tool

Build the harness you need, from anything, for anything. Chain scripts,
CLI tools, containers, APIs, message queues, any programming language, AI
agents and models: anything that runs becomes a step in your workflows.

You write the chain in a small JSON file, by hand or with AI; itero runs
it when something happens or on a schedule, with waiting, retries,
guards, undo and a record of every run.

Documentation: <https://aegea-dev.github.io/itero/>.

## In a minute: a beacon

Something to keep an eye on: a site, a local server, a backup. Every
minute itero asks it; when it does not answer, you get a notification
saying why.

```bash
mkdir beacon && cd beacon && echo '{}' > itero.config.json && mkdir actions workflows

cat > actions/check-site <<'EOF'
#!/bin/sh
curl -fs --max-time 10 -o /dev/null "$url" || { echo "$url is down" >&2; exit 1; }
EOF
chmod +x actions/check-site

cat > workflows/beacon.json <<'EOF'
{ "on": [{ "schedule": "* * * * *" }],
  "steps": [{ "#check-site": { "url": "http://localhost:3000" } }],
  "error": [{ "#notify": { "title": "beacon", "text": "$why" } }] }
EOF

itero daemon
```

Two small files and one command. [`examples/beacon`](examples/beacon) goes
further: many sites and the disk's free space, told only when something
changes (down, then back up).

## What people chain

- **Tell me when a mail arrives, and only the important ones**: a source
  for new mail and a notification; a model step to rate it decides which
  you hear about. [`mail-alert`](examples/mail-alert), a model on this
  machine in [`page-summary`](examples/page-summary)
- **Process every file dropped in this folder**: a watched folder, one run
  per burst of files. [`photos`](examples/photos)
- **Wait for a payment, then ship**: a step that waits for a message that
  comes later, minutes or days. [`order`](examples/order)
- **Make the cluster, refuse a dangerous plan, roll back on failure**:
  terraform and kubectl chained, a guard step, and undo.
  [`terraform`](examples/terraform)
- **My app needs a job done and wants the result**: `itero emit --wait`
  answers with the run's state, or a callback when it ends.
  [`languages`](examples/languages)
- **Every morning, a summary of these pages**: a schedule, a fetch, a
  model, a mail. [`page-digest`](examples/page-digest)
- **Is everything still up?**: health checks, told when something changes.
  [`beacon`](examples/beacon)

## Plain files in git: nothing lost, everything iterated

A workspace is a folder of plain files: workflows as JSON, actions as
executables. Keep it in git, share an action or a whole workflow as a
folder, clone someone's and improve it. A run keeps the version of the
workflow it started with, so editing a file never changes a run under
way.

## Next to the others

- **cron** runs a command at a time; itero chains steps, waits for
  messages, retries, undoes and keeps a record.
- **n8n, Zapier** offer the nodes they built; in itero any program is a
  step, and the workflow is a file you keep.
- **Temporal, Airflow** need their SDK and a server; itero is one binary,
  and a step is whatever runs on your machine.

## Install

```bash
brew trust aegea-dev/tap
brew install aegea-dev/tap/itero
# or
curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
```

Homebrew 7 and later load a tap's formulae only once you trust the tap,
hence `brew trust`. Homebrew puts `Itero.app` in the formula's own folder. The script checks
the release archive's SHA-256 before unpacking, installs `itero` to
`~/.local/bin` and, on macOS, `Itero.app` to `~/Applications`, and never
edits your shell profile. `--version X.Y.Z`, `--to DIR` and `--uninstall`
are its options (`sh -s -- --help`).

## Build

```bash
cargo build
make test         # the test suite
make lint-check   # clippy, warnings fail
make fmt-check    # rustfmt
make check        # cargo check, every target
make audit        # Cargo.lock against RustSec advisories (needs cargo-audit)
make test-linux   # the tests and clippy on Linux, in Docker
make test-docker  # the IMAP, POP3 and Gmail examples against a mail server in Docker
make docs         # the docs site at http://localhost:4321/itero/ (make docs-install once)
make verify       # every gate before a release: fmt, check, clippy, tests, audit, docs
```

On macOS, `make notifier` builds `Itero.app` next to the binary (and `make
install` puts it in `~/Applications`): notifications then come from itero.
Without it they come through `osascript`. Its icon is
`packaging/macos/Itero.icns`, built from `itero.svg` by `make icon`. On Linux, notifications go
through `notify-send` (or `gdbus`, or `kdialog`).

## A workspace

A folder with `itero.config.json` is a workspace (`{}` will do); `itero daemon` runs it.

```
itero.config.json          the manifest: what it is, port, limits, and each workflow, action and source
workflows/order.json       a workflow: when it runs (on) and its steps
actions/check-payment      an action: an executable, its name its file name
sources/mail.received      a source: each line it prints is a message of this type
.itero/                    the journal, the log (itero.log) and memory
```

`workflows/`, `actions/` and `sources/` may have subfolders to group files;
a name stays its file name, and `itero which <name>` says where it is.

- **A workflow** starts on a message (`{"event": "order.placed"}`), a
  schedule (`{"schedule": "0 9 * * *"}`) or a watched file
  (`{"path": "inbox"}`), under conditions (`{"$paid": true}`,
  `{"has": ["$order", "$$API"]}`). Its steps are actions (`#name`),
  workflows, lists that run side by side, and steps with data and their
  own `on`. `success`, `failure` and `error` run at the end.
- **An action** reads its values as environment variables (from the step,
  its args, then the run's state; `$$NAME` for a secret) and the state on
  stdin, prints a JSON object, and exits 0 (done), 75 (not yet: wait for a
  matching message) or anything else (an error). A program or a package it
  needs and lacks is named: in the run's error when a step fails, and by
  `itero check` before anything runs.
- **A message** starts runs of the workflows on its type, and reaches the
  runs waiting for it that it matches by their data.

Commands: `itero daemon`, `itero emit <type> '<data>'`, `itero args`, `itero config`,
`itero secret`, `itero check`, `itero which`, `itero workflows`, `itero info`, `itero runs [<run>]`,
`itero errors`, `itero watch`, `itero service install|status|uninstall`.
What the daemon does is in `.itero/itero.log`, one JSON line each.

## Examples

- [`examples/beacon`](examples/beacon): sites and the disk's free space
  checked on a schedule, with a notification when something changes.
- [`examples/mail-alert`](examples/mail-alert): a notification for each
  new mail in the local mailbox, as it arrives.
- [`examples/mail-servers`](examples/mail-servers): new mail on an IMAP or
  POP3 server, checked every minute with curl.
- [`examples/gmail`](examples/gmail): a notification for each new Gmail
  mail the moment it arrives, pushed by IMAP IDLE (Python, no polling).
- [`examples/order`](examples/order): payment and stock side by side, the
  payment waiting for its provider, then shipping and a mail.
- [`examples/page-digest`](examples/page-digest): a page, summarized by an
  AI assistant, by mail.
- [`examples/page-summary`](examples/page-summary): a page sent in a
  message, summarized by a model on this machine (LM Studio), printed in
  the daemon's terminal.
- [`examples/photos`](examples/photos): a thumbnail for each photo dropped
  into a folder the daemon watches.
- [`examples/languages`](examples/languages): actions in Python (uv), Node,
  Bun and Docker, each in its subfolder and bringing its own packages,
  answering `itero emit --wait`.
- [`examples/terraform`](examples/terraform): a k3d cluster of 3 machines
  with a web app applied by OpenTofu, a guard on the plan, and the cluster
  deleted when a step fails.

## Design

[`docs/design-simple.md`](docs/design-simple.md) is the design of the
files people write; [`docs/design.md`](docs/design.md) the engine
underneath. The `docs/plan-*.md` files are the plans the iterations
followed.
