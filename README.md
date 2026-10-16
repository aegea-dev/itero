# itero

itero (Latin: "I repeat") runs workflows: templates of steps that run again
and again, on a schedule or when something happens, on one machine, using
the command line tools already there. Apps send it events and receive its
actions; it decides nothing on its own beyond what a workflow says.

itero is young: schedules, app events and sources (a file watched, a mail
server's IDLE) start workflows with steps, each workflow has a memory,
actions run commands, shell lines, desktop notifications and programs in
containers, the daemon has a local HTTP API, and it starts at login.

Documentation: <https://aegea-dev.github.io/itero/>.

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
make test         # the test suite
make lint-check   # clippy, warnings fail
make fmt-check
make test-linux   # the tests and clippy on Linux, in Docker
make test-docker  # container actions, and the IMAP and POP3 examples against a mail server in Docker
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
  `schedule` (cron) or `on` an event, whose data is the run's input;
  `concurrency` how many runs at once; `pause_after` how many failed runs
  in a row pause it (5 unless said).
- `actions/<name>/action.json`: the workspace's own actions, programs that
  read a job as JSON on stdin and print their output: an executable,
  Python, Bun, or a program in a container image (`"type": "container"`).
- `sources/<name>.json`: sources, programs the daemon keeps running that
  tell it when something happens, one event per line: `watch@1` (a file
  or folder, with the OS's own events), or any program, in a container if
  it names an image.
- `.itero/journal.sqlite`: every event, run and job, and each workflow's
  memory. Local, never synced.

Commands: `itero daemon`, `itero emit <type> [--data JSON] [--run ID]`,
`itero check` (every workflow, action and source), `itero runs`, `itero
errors` (failed runs, where and why), `itero memory <workflow>`, `itero
resume <workflow>` (after a pause), `itero service
install|status|uninstall`.

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
| `GET /health` | the loop's passes, the last one and its error, and each source |
| `POST /events` | an event, `{"type", "data"?, "run"?}` as `application/json`; 202 `{"id"}` |
| `GET /runs` | runs, newest first (`?workflow=`, `?state=`, `?limit=`) |
| `GET /runs/{id}` | a run and its events |
| `GET /workflows` | each workflow's name, version, triggers and pause |
| `GET /workflows/{name}/memory` | a workflow's memory |

An event without `run` may start runs: every workflow with a trigger `on`
its type. For example, a workflow with `"triggers": [ { "on":
"photo.added" } ]` starts when an app sends:

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' \
  -d '{"type": "photo.added", "data": {"path": "trip/market.jpg"}}'
```

It also reaches the runs waiting for it: a run in a state with a
transition `on` its type and a `match` (`"match": "event.data.path ==
run.input.path"`) takes it when it is that run's. Workflows publish too,
with a step `{ "publish": "photo.tagged", "data": {...} }`, and the engine
publishes `run.failed` and `workflow.paused` for maintenance workflows.
`"from": "*"` is a transition from any state.

## The mail check

[`examples/mail-check`](examples/mail-check) shows a notification for
mail in the local mailbox it has not seen before, as it arrives: a source
watches the mailbox, and an hourly check is the safety net.

```bash
cd examples/mail-check && cargo run -q -- daemon
# in another terminal:
echo "merhaba" | mail -s "deneme" "$USER"
```

Within a second or two one notification says `1 new: deneme`. Ctrl-C
stops the daemon.

## Mail servers

[`examples/mail-servers`](examples/mail-servers) has two workflows, one
for an IMAP mailbox and one for POP3, each with its server and account in
its check step and the password a secret (`IMAP_PASSWORD`,
`POP3_PASSWORD`). The checks run in containers (`python:3.13-alpine`), so
the machine needs Docker, not Python. IMAP mail is notified as it arrives:
a source in a container waits in IMAP IDLE; POP3, which has nothing like
it, is checked every 5 minutes.

## Design

[`docs/design.md`](docs/design.md) says what itero is made of and what was
decided; the `docs/plan-*.md` files are the plans the iterations followed.
