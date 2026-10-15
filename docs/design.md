# itero: design

itero (Latin: "I repeat") runs workflows: templates of steps that run again
and again, on a schedule or when something happens, on one machine, using
the command line tools already there. It began as the lifecycle engine
inside primum (a markdown backlog) and stands on its own: apps send it
events and receive its actions; primum will be its first app.

This document records what was decided with the owner on 2026-10-15. It is
built in iterations; section 9 says what the first one holds.

## 1. The pieces

```
  app or person ──events──►  ┌────────────────────┐
  (HTTP API, queue, inbox)   │      daemon        │  saves every event and
                             │ (one per workspace)│  answer (journal, SQLite)
  schedules (cron) ─────────►└──┬──────────────┬──┘  decides nothing
                                │              │
                       ┌────────▼───┐  ┌───────▼──────┐
                       │  manager   │  │    worker    │  one process per
                       │ pure: JSON │  │ runs one     │  action, any
                       │ in and out │  │ action, exits│  language
                       └────────────┘  └──────────────┘
```

- **`itero daemon`** serves an HTTP API on 127.0.0.1 (a port, 8787 by
  default) and runs passes: it saves events, fires schedules and timers,
  asks the manager, saves each answer in one transaction, and runs jobs in
  workers. With no workflow defined it does nothing.
- **`itero manager`** is pure: JSON in, JSON out, no IO, no clock, no
  randomness. The same input always gives the same answer.
- **`itero worker`** runs one action in its own process group and exits.
- The watchdog, the OS service (launchd, systemd), secrets (the OS secret
  store) and leases come from primum as they are.
- One daemon per workspace. A person may keep several workspaces apart
  (work, home); each has its own daemon and port.
- Installed by an install script, Homebrew and cargo.

## 2. The workspace

A folder with `itero.toml`:

```toml
port = 8787
workflows = ["workflows", "~/projects/videre/workflows"]  # any number
actions = ["actions"]                                     # any number
```

- `workflows/<name>.json`: workflow templates (primum called them
  lifecycles).
- `actions/<name>/action.json` and its program: project actions (primum
  called them actionables).
- `triggers/<name>.json`: when to run which workflow, with what arguments.
- `.itero/journal.sqlite`: the journal and the workflows' memory. Local,
  never synced.

The workspace's own folders hold standalone workflows (a mail check, disk
clean-up); an app's folder can bring its own.

## 3. Workflows and runs

A **workflow** is a template: states, transitions, actions, in primum's
lifecycle format (`states`, `start`, `end`, `transitions` with `from`,
`to`, `on`, `when`, `timeout`/`on_timeout`, `do`). A **run** is one
instance of it: it starts at `start`, moves through transitions, and ends
in an `end` state or `failed`. Runs are short; a recurring job is many runs.

Within a transition's `do` chain:

- an action may have an `id`; its output is then `steps.<id>` for the rest
  of the run (in templates and conditions), next to `prev` (the previous
  action's output);
- an action may have a `when`: false skips it (`prev` passes through).

## 4. Triggers

What starts a run:

- **a schedule**: `"schedule": "*/5 * * * *"` (cron, the machine's local
  time);
- **an event**: `"on": "photo.added"`, an event an app sends;
- for primum later: a ticket attached to a workflow.

A workflow can declare its own triggers (`"triggers": [...]` in its file),
for its housekeeping. The workspace's `triggers/` can add triggers, or
replace a workflow's own by name. A trigger gives the run its `args`.

## 5. Memory

Each workflow has a small store in the journal: keys and JSON values
("seen up to", "sent this year"), for its own bookkeeping, never an app's
records. A run reads it as `memory` in templates and conditions; the
built-in `memory.set@1` changes it, saved in the same transaction as the
answer that asked for it. An app's own data is read and written through
the app (its API or command line), by actions.

## 6. Actions

Built-ins:

| Action | Does |
|---|---|
| `http.request@1` | an HTTP request, with an idempotency key header |
| `command@1` | a program and its arguments, no shell |
| `sh@1` | a shell line (`sh -c`), for pipelines |
| `notify@1` | a desktop notification: osascript on macOS, notify-send on Linux |
| `memory.set@1` | sets keys in the workflow's memory (pure, in the manager) |
| `json.pick@1`, `event.emit@1` | as in primum (pure) |

- **Values reach a command only as environment variables**, never pasted
  into its text: `sh@1` takes `run` (fixed text) and `env` (templates
  allowed); a template in `run` is refused when the workflow is checked.
  A mail subject with `; rm -rf ~` in it stays text.
- **Output**: every action a worker runs as a program (`command@1`,
  `sh@1`, workspace actions) gives `output.exit`, `output.stdout`,
  `output.stderr`, `output.lines` (stdout's lines), and `output.json` when
  stdout parses as JSON. `command@1` and `sh@1` fail on a non-zero exit
  unless their `expect` says otherwise; a workspace action that exits
  non-zero fails.
- Project actions (`actions/`): `executable`, `python@`, `bun@`, as in
  primum's actionables.
- Every job carries an idempotency key, the same when it runs again.
- Lost jobs (killed, lease run out, daemon stopped) run again, up to 3
  runs; errors fail the action at once. Leases: `limits.heartbeat`,
  renewed by touching `$ITERO_HEARTBEAT`; `limits.timeout` is the cap.

## 7. Apps

Apps connect through the daemon, not as a library:

- **events in**: `POST /api/events` (type, data, optionally a run or a
  subject), a file in the workspace's `inbox/`, and later a message queue
  (the journal first; NATS later);
- **actions out**: actions that call the app (its CLI or API).

primum becomes an app: its ticket changes become events, and changing a
ticket is an action that runs `primum`.

## 8. What carries over from primum

From primum at commit f20debf: the journal (SQLite, schema versions), the
pure manager with CEL conditions and templates, the worker (HTTP, command,
executable, python@, bun@, process groups, time limits), lost jobs, leases
and heartbeats, a killed manager asked again, secrets, the watchdog, the
service, the HTTP routes, `die_at` and the kill tests, the demo. Not
carried: the ledger (items, board, sync, history), tickets as the unit,
primum's CLI.

## 9. The first iteration: the mail check

One example, end to end, and only what it needs:

```json
{
  "name": "mail-check",
  "description": "Notifies about mail not seen before.",
  "triggers": [ { "schedule": "*/5 * * * *" } ],
  "states": ["start", "done"],
  "start": "start",
  "end": ["done"],
  "transitions": [
    { "from": "start", "to": "done",
      "do": [
        { "id": "check", "action": "mail.check@1",
          "with": { "mailbox": "${args.mailbox}",
                    "seen": "${has(memory.seen) ? memory.seen : ''}" } },
        { "action": "notify@1", "when": "steps.check.json.new > 0",
          "with": { "title": "Mail", "text": "${steps.check.json.summary}" } },
        { "action": "memory.set@1", "with": { "seen": "${steps.check.json.seen}" } }
      ] }
  ]
}
```

`mail.check@1` is a workspace action (`actions/mail/`): a small POSIX
script reading the mbox file's `From ` and `Subject: ` lines with `grep`,
`awk` and `sed`, printing
`{"new": n, "seen": "<last message's id>", "summary": "..."}`.

Done when `itero daemon` in a workspace with this workflow runs it every 5
minutes, and a mail sent locally (`echo hi | mail -s deneme $USER`) gives
one notification on the next run and none after. Tests write a small mbox
file instead of sending mail, and stub the notifier.

Left for later iterations: the HTTP API (the port is read but nothing
listens yet), event triggers from apps, the queue, the inbox, primum as an
app, the service and watchdog commands, the install script, Homebrew, the
marketplace.
