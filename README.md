# itero

itero (Latin: "I repeat") runs workflows: templates of steps that run again
and again, on a schedule or when something happens, on one machine, using
the command line tools already there. Apps send it events and receive its
actions; it decides nothing on its own beyond what a workflow says.

itero is young: schedules and app events start workflows with steps, each
workflow has a memory, actions run commands, shell lines and desktop
notifications, the daemon has a local HTTP API, and it starts at login.

## Install

```bash
brew install aegea-dev/tap/itero
# or
curl -fsSL https://raw.githubusercontent.com/aegea-dev/itero/main/install.sh | sh
```

Homebrew puts `Itero.app` in the formula's own folder. The script checks
the release archive's SHA-256 before unpacking, installs `itero` to
`~/.local/bin` and, on macOS, `Itero.app` to `~/Applications`, and never
edits your shell profile. `--version X.Y.Z`, `--to DIR` and `--uninstall`
are its options (`sh -s -- --help`).

## Build

```bash
cargo build
make test        # the test suite
make lint-check  # clippy, warnings fail
make fmt-check
```

On macOS, `make notifier` builds `Itero.app` next to the binary (and `make
install` puts it in `~/Applications`): notifications then come from itero,
which asks once to be allowed to show them. Without it they come through
`osascript`, shown as Script Editor's. On Linux, notifications go through
`notify-send` (or `gdbus`, or `kdialog`).

## A workspace

A folder with `itero.toml` is a workspace; `itero daemon` runs its
workflows, pass after pass.

```toml
port = 8787                    # the API, on 127.0.0.1
workflows = ["workflows"]      # any number of folders
actions = ["actions"]
```

- `workflows/<name>.json`: a workflow: states, transitions, and the actions
  each transition runs in order; `triggers` say when a run starts: a
  `schedule` (cron) or `on` an app's event, whose data is the run's input.
- `actions/<name>/action.json`: the workspace's own actions, programs that
  read a job as JSON on stdin and print their output.
- `.itero/journal.sqlite`: every event, run and job, and each workflow's
  memory. Local, never synced.

Commands: `itero daemon`, `itero emit <type> [--data JSON] [--run ID]`,
`itero check` (every workflow and action, with its version), `itero runs`,
`itero memory <workflow>`, `itero service install|status|uninstall`.

### Workspace triggers

`triggers/<workflow>.json` sets when a workflow runs in this workspace,
without editing the workflow: its triggers are added to the workflow's
own, or used instead of them with `"replace": true`. For the mail check, a
10 minute schedule and this machine's mailbox:

```json
{ "replace": true,
  "triggers": [ { "schedule": "*/10 * * * *", "args": { "mailbox": "/var/mail/me" } } ] }
```

### Start at login

In a workspace, `itero service install` runs its daemon as a per-user
service (a launchd agent on macOS, a systemd user unit on Linux) that
starts at login. A watchdog keeps it up: it restarts the daemon when it
exits, stops answering `/health`, or stops making passes. One service per
workspace, each on its workspace's port; it logs to `.itero/daemon.log` and
keeps the installing shell's `PATH` (install again after changing it).
`itero service status` shows the service and the daemon's health; `itero
service uninstall` removes it.

## The API

`itero daemon` serves on `127.0.0.1:<port>` only, and refuses a request
whose `Host` is not that address:

| Route | Does |
|---|---|
| `GET /health` | the loop's passes, the last one and its error |
| `POST /events` | an event, `{"type", "data"?, "run"?}` as `application/json`; 202 `{"id"}` |
| `GET /runs` | runs, newest first (`?workflow=`, `?limit=`) |
| `GET /runs/{id}` | a run and its events |
| `GET /workflows` | each workflow's name, version and triggers |
| `GET /workflows/{name}/memory` | a workflow's memory |

An event without `run` may start runs: every workflow with a trigger `on`
its type. For example, a workflow with `"triggers": [ { "on":
"photo.added" } ]` starts when an app sends:

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' \
  -d '{"type": "photo.added", "data": {"path": "trip/market.jpg"}}'
```

## The mail check

[`examples/mail-check`](examples/mail-check) reads the local mailbox every
5 minutes and shows a notification for mail it has not seen before:

```bash
cd examples/mail-check && cargo run -q -- daemon
# in another terminal:
echo "merhaba" | mail -s "deneme" "$USER"
```

Within 5 minutes one notification says `1 new: deneme`; later runs show
none until more mail comes. Ctrl-C stops the daemon.

## Design

[`docs/design.md`](docs/design.md) says what itero is made of and what was
decided; [`docs/plan-1-mail-check.md`](docs/plan-1-mail-check.md) and
[`docs/plan-2-api-events-install.md`](docs/plan-2-api-events-install.md)
are the plans the iterations followed.
