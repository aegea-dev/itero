# itero

itero (Latin: "I repeat") runs workflows: templates of steps that run again
and again, on a schedule or when something happens, on one machine, using
the command line tools already there. Apps send it events and receive its
actions; it decides nothing on its own beyond what a workflow says.

itero is at its first iteration: schedules, workflows with steps, a
workflow's memory, and actions that run commands, shell lines and desktop
notifications. The HTTP API, events from apps and the service come next.

## Build

```bash
cargo build
make test        # the test suite
make lint-check  # clippy, warnings fail
make fmt-check
```

## A workspace

A folder with `itero.toml` is a workspace; `itero daemon` runs its
workflows, pass after pass.

```toml
port = 8787                    # read, not served yet
workflows = ["workflows"]      # any number of folders
actions = ["actions"]
```

- `workflows/<name>.json`: a workflow: states, transitions, and the actions
  each transition runs in order; `triggers` say when a run starts.
- `actions/<name>/action.json`: the workspace's own actions, programs that
  read a job as JSON on stdin and print their output.
- `.itero/journal.sqlite`: every event, run and job, and each workflow's
  memory. Local, never synced.

Commands: `itero daemon`, `itero check` (every workflow and action, with
its version), `itero runs`, `itero memory <workflow>`.

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
decided; [`docs/plan-1-mail-check.md`](docs/plan-1-mail-check.md) is the
plan this first iteration followed.
