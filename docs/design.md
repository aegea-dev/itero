# itero: design

itero (Latin: "I repeat") runs workflows: templates of steps that run again
and again, on a schedule or when something happens, on one machine, using
the command line tools already there. It began as the lifecycle engine
inside primum (a markdown backlog) and stands on its own: apps send it
events and receive its actions; primum will be its first app.

This document records what was decided with the owner on 2026-10-15
(sections 1 to 9, built in iterations 1 to 3) and what was discussed on
2026-10-16 for the iterations after them (sections 10 to 20). Each later
section says whether it is **decided** (the owner agreed), **proposed** (a
recommendation the owner has not confirmed) or **open**.

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
- **`itero service install`** registers a per-user service (launchd,
  systemd) that runs **`itero watchdog`** in the workspace at login; the
  watchdog runs `itero daemon` and restarts it when it exits, stops
  answering `/health` or stops making passes. Both come from primum, as do
  secrets (the OS secret store) and leases.
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
- **an event**: `"on": "photo.added"`, an event an app sends; the event's
  `data` is the run's `input`;
- for primum later: a ticket attached to a workflow.

A trigger has one of `schedule` and `on`, and gives the run its `args`. A
workflow declares its own triggers (`"triggers": [...]` in its file). An
app's event starts one run per matching trigger, in one transaction with
handling the event, so an event delivered again starts nothing twice; an
event nothing waits for is handled with a note.

The workspace's `triggers/<workflow>.json` adds triggers to a workflow's
own, or replaces them (`"replace": true`). A workflow's version leaves its
triggers out: when it runs is the workspace's business, not part of what
a run is pinned to. A schedule's last slot is kept under its workflow and
cron text, so merging or reordering triggers keeps it.

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
| `notify@1` | a desktop notification: on macOS through `Itero.app` (sent as itero, with its own entry in System Settings, Notifications), else `osascript`; on Linux the first of `notify-send`, `gdbus` (freedesktop notifications), `kdialog` |
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

- **events in**: `POST /events` (`{"type", "data"?, "run"?}`) or `itero
  emit` (which writes the journal directly, so it works with the daemon
  stopped); later a file in the workspace's `inbox/`, and NATS. With
  `run`, the event goes to that run; without, it may start runs. The
  engine's own types (`action.*`, `timer.*`, `run.*`) are refused;
- **actions out**: actions that call the app (its CLI or API).

The daemon's API listens on 127.0.0.1:<port> only: `GET /health`, `POST
/events`, `GET /runs` (`?workflow=`, `?limit=`), `GET /runs/{id}` (with its
events), `GET /workflows`, `GET /workflows/{name}/memory`. A request whose
`Host` is not this port on 127.0.0.1 or localhost is refused (no reaching
it by DNS rebinding), and events must be `application/json` (a page on
another site cannot post one without a preflight, which is never granted).

## Installing

`install.sh` (curl or wget, POSIX sh) installs a release for macOS (Apple
Silicon, Intel) or Linux (x86_64, aarch64): it checks the archive's SHA-256
before unpacking, puts `itero` in `~/.local/bin` and, on macOS, `Itero.app`
in `~/Applications`, and never edits a shell profile. Homebrew
(`brew install aegea-dev/tap/itero`) installs the same archives, with
`Itero.app` in the formula's `libexec`, where the worker also looks. `make
dist` makes the archives; the release workflow publishes them on a version
tag and writes their checksums into the tap's formula.

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

Iteration 2 (`docs/plan-2-api-events-install.md`) added the HTTP API, event
triggers and the installer; iteration 3
(`docs/plan-3-service-triggers-homebrew.md`) the service at login, the
workspace's `triggers/` and Homebrew. Section 20 orders what comes next.

---

## 10. Where it goes next

The mail check (now "v1") polls: every 5 minutes it reads the mailbox,
whether or not mail came. The owner's next step is **v2, the mail check on
events**: a notification as soon as mail arrives, with nothing waiting on a
clock. Getting there raised wider questions, recorded here: how things are
named, how a transition is taken, steps that run side by side, publish and
subscribe, long-lived runs, failures and their guards, self-recovery with
an AI assistant, and how the outside world reaches itero (sources and
connectors).

The terms are in the documentation's glossary
(`docs/src/content/docs/reference/glossary.md`); new terms from these
sections are added there when they are built.

## 11. Identity: names and UUIDs

**Decided**: `name@version` is for people, UUIDs are for the machine.

- **People** write and read names: in the files, the command line, the
  API's paths, logs and errors. A workflow, state, transition or step is
  `mail-check@e734495fe858/check`.
- **The machine** executes by UUID. When the daemon first pins a workflow
  version, it gives every entity in it (the workflow version, each state,
  transition and step) a UUID v7, stored in the journal with the pinned
  definition. Runtimes, deliveries, subscriptions and timers refer to
  entities by UUID; a name path resolves to its UUID through the pin, and
  back.
- **The files carry no UUIDs**: people and assistants never write, read or
  copy them. A version keeps its UUIDs for good (they are assigned once, at
  pinning); a new version is a new definition with new UUIDs, and the names
  link one version's entities to the next's.

The reasoning that led here, kept for the record:

**Records get UUID v7 ids.** Everything that happens at a moment: events,
runs, jobs, and later deliveries and fix proposals. Events and runs have
them already (`Envelope::new`, run ids like
`01a14427-7964-743c-9286-66325b11c347`). They are unique across machines
(ready for NATS) and sort by time, which is what a log needs.

**Definitions get readable names.** Workflows, states, transitions, steps,
actions and sources are written by people and by AI assistants, read in
diffs, logs and error messages, and copied from one project to another. A
UUID there:

- is unreadable: `transition 0193c2a1-...` in an error says nothing, where
  `transition "start -> done"` or step `check` does;
- does not make a copy unique: copying a workflow copies its UUIDs, so
  uniqueness would still need tooling to renumber;
- adds nothing to pinning: a run is already pinned to the workflow's
  version, a hash of exactly what it runs.

So in the files every definition that something refers to has a name
(lowercase letters, digits and `-`), unique in its scope, and the engine
refuses a duplicate; the UUIDs above live only in the journal. Transitions get an optional `id` (today they are found by their
position in the file, which an edit can shift; a run is pinned to its
version, so this is safe today, but nested steps and subscriptions will
refer to them). A full address is the workflow, its version and the path
of ids:

```
mail-check@e734495fe858/check
review@9b1f02c4aa71/asked/approve
```

Packages that are shared (connectors, section 18) get reverse-DNS names,
`dev.aegea.mail`, which are global and still readable.

## 12. How a transition is taken

**Decided** as the list of ways; the additions are proposed.

Today a transition is taken in one of five ways:

| Way | How it is written |
|---|---|
| the previous step finished | no `on`: taken as soon as the run enters `from` |
| an event arrived | `on`: an event of that type, sent to the run |
| a condition holds | `when`, with either of the above |
| time ran out | `timeout` and `on_timeout` |
| an action failed | `on_fail`, else the engine's `failed` |

Data moves with it: a transition's `input` is the event's data, or the
output that ended the previous transition; `args`, `memory` and `run` are
always there.

Proposed additions:

- **several steps finished**: a join, section 13;
- **at a time**: `"at": "<RFC 3339 time or expression>"`, a timer set when
  the run enters `from` (`"at": "${args.remind_at}"`);
- **from any state**: `"from": "*"`, for a transition such as `cancel` that
  applies wherever the run is; a transition from the run's own state wins
  over a `*` one;
- **an event from anyone**: `on` with `match`, section 14.

## 13. Steps side by side: groups, race, all

**Decided**: steps can run side by side, a step can wait for other steps,
and groups nest. The key names `steps`, `all`, `race`, `some` and `join`
are agreed for now.

The owner's example: three steps start together, one of them asynchronous
(it waits for something outside), two of them computation; one step waits
for them to finish, takes their results, and continues as one.

Today a transition's `do` is a chain: one step after the other. It becomes
a tree. A step is one of:

- **an action**, as now;
- **a wait** for an event: `{ "id": "paid", "wait": "payment.confirmed",
  "match": "...", "timeout": "1h" }`; its output is the event's data;
- **a group** of steps, which may hold groups:

| Group | Starts | Done when |
|---|---|---|
| `"steps": [...]` | one after another (what `do` is today) | the last is done |
| `"all": [...]` | all at once | every one is done (`waitAll`) |
| `"race": [...]` | all at once | the first is done; the others are cancelled |
| `"some": n, "of": [...]` | all at once | the first `n` are done; the others are cancelled |

A group may name the members it waits for with `join`; the others go on
detached, and their outputs land in `steps` when they finish, for a later
step or transition that wants them:

```json
"do": [
  { "id": "prepare", "all": [
      { "id": "resize", "action": "photos.resize@1" },
      { "id": "tag", "action": "photos.tag@1" },
      { "id": "upload", "wait": "upload.confirmed", "timeout": "10m" } ],
    "join": ["resize", "upload"] },
  { "action": "notify@1",
    "with": { "title": "Photo", "text": "${steps.resize.json.size} at ${steps.upload.url}" } }
]
```

Rules:

- **Data.** Each member gets the group's `input`, and as `prev` the output
  of the step before the group. A member with an `id` is `steps.<id>`. A
  group's own output is an object of its members' outputs by id; a race's is
  the winner's, with `steps.<group>.winner` naming it.
- **Failure.** `all` fails when a joined member fails; `race` when every
  member failed; `some` when fewer than `n` can still finish. The members
  still going are cancelled: their jobs' process groups are killed, their
  waits dropped.
- **Limits.** Groups nest up to 8 deep, and a transition holds at most 100
  steps in all; both are checked with the workflow.
- **Asynchronous by nature.** The engine already is: the manager never
  waits, every job's result comes back as an event (`action.*`), every
  timeout as a `timer.*` event. A group is bookkeeping in the run's runtime:
  which members are pending, done, failed or cancelled. A sequence is just
  a group whose members start one after the other, so every workflow is
  asynchronous and nothing changes for workflows written today: `do` is a
  `steps` group.

## 14. Publish, subscribe, and runs that live long

**Decided**: publish and subscribe, built on the journal. The key names and
the delivery table are proposed.

### Where it lives

The journal is already a durable event log and the daemon already routes
it. Each event has a UUID v7 id, a type, a `subject` and a `cause` (the
event that led to it), CloudEvents-shaped. Subjects today:

| Subject | Means |
|---|---|
| `itero.app` | for the workspace: it may start runs (triggers) |
| `itero.run.<id>` | for one run |
| `itero.worker` | a job for a worker |

Publish and subscribe needs no new store, only a routing table:

- **topics** are event types;
- **subscribers** are, first, **triggers** (`on` a type: start a run), and
  second, **waiting runs**: a run whose current state has a transition
  `on` that type with a `match`. Later: connectors and other machines.

Today an event is handled once (`events.handled`). One event for many
subscribers needs a record per delivery:

```sql
CREATE TABLE deliveries (
  event INTEGER NOT NULL,      -- events.seq
  subscriber TEXT NOT NULL,    -- "trigger:<workflow>#<n>" or "run:<id>"
  handled TEXT, error TEXT,
  PRIMARY KEY (event, subscriber)
);
```

Each delivery is handled in its own transaction, as an event is now: an
event delivered twice to the same subscriber does nothing twice. An event
nothing subscribes to is handled with a note, as now. The migration adds
the table; events already handled stay handled.

### Correlation

Which waiting run gets an event is a condition over the event and the run:

```json
{ "from": "waiting", "to": "replied", "on": "mail.received",
  "match": "event.data.thread == args.thread" }
```

Without `match`, a transition `on` a type gets only events sent to its own
run, as today, so workflows written today behave the same.

### Publishing

A step publishes to everyone subscribed:

```json
{ "publish": "mail.notified", "data": { "id": "${steps.check.json.seen}" } }
```

Like `event.emit@1` today, the event is sent only when the chain succeeds.

### Explicit keys, not omitted ones

The owner suggested smart configuration: a transition without `to`
publishes, one without `from` subscribes. **Decided** instead: explicit
keys (`publish`, `on` with `match`, `"from": "*"`). itero's files have one
form and every key is checked; a forgotten key must be an error, never a
different meaning that runs quietly.

### Long-lived runs

Some runs should live for good and react to each event, like a loop in
libuv: a state with a transition to itself, on a type, with a match:

```json
{ "from": "listening", "to": "listening", "on": "sensor.reading",
  "match": "event.data.room == args.room",
  "do": [ { "action": "sh@1", "with": { "run": "...", "env": { "T": "${input.celsius}" } } } ] }
```

Short runs stay the default (a recurring job is many short runs); a long
run suits something with a lifecycle: a conversation, an approval, a
device's session. Its history grows in the journal, so compaction of old
handled events comes with it.

### NATS later

The subjects are NATS-shaped on purpose. With NATS, the same subjects
travel between machines, a JetStream durable consumer is a subscriber, and
the journal stays the local record of what this daemon did.

## 15. Failures, guards and logs

**Decided**: richer failure handling (retry, rollback) waits; for now a
failed run is simply run again by its next trigger. Two things are needed
now: **guards**, so nothing runs or retries for ever, and **errors logged**
where they can be looked after.

Guards that exist: a job's time limit (60 seconds unless the action says),
a lost job run at most 3 times, at most 100 moves in one decision,
`on_fail`.

Guards to add (**proposed**):

- **`concurrency`** on a workflow: `1` runs one run at a time and queues
  the rest (`"concurrency": { "max": 1, "then": "queue" | "skip" }`). v2
  needs it: two quick changes to the mailbox must not start two runs that
  both read the old memory and both notify.
- **A breaker**: after 5 failed runs in a row (`"pause_after": 5`), the
  workflow is paused: its triggers stop starting runs, a `workflow.paused`
  event is published and a notification shown. `itero resume <workflow>`
  starts it again, and so does a new version of it.
- **Attempts**: `run.attempts` counts how often the run entered each state,
  and `max_attempts` on a workflow fails a run that loops through
  `on_fail` back to an earlier state too often.
- **A run's time**: `limits.run`, the longest a run may take before it
  fails (long-lived runs set none).

Logs (**proposed**): every failure is in the journal (the event's `error`,
the run's state `failed` and why) and in `.itero/daemon.log`. `itero runs
--failed`, `itero errors` (the newest failures with their step and stderr
tail) and `GET /runs?state=failed` show them. The engine publishes
`run.failed` and `workflow.paused` to subscribers (they stay types nothing
outside may send), which is what section 16 builds on.

Later: `retry` on an action (`{"times": 3, "wait": "30s"}`), and `undo`
(compensation, the saga pattern): when a later step fails, the undo actions
of the finished steps run in reverse order. Side effects cannot be rolled
back (a sent mail stays sent); itero's own changes already are: memory
changes and published events are held until a chain succeeds.

## 16. Self-recovery: maintenance workflows

From the owner's idea: when a workflow fails, ask an AI assistant what went
wrong and fix the workflow: the ultimate self-recovery mode.

**Decided**: recovery is done by **explicit maintenance workflows**. A
maintenance workflow is an ordinary workflow that a workspace installs on
purpose; it subscribes to the engine's own events (`run.failed`,
`workflow.paused`) and does something about them. Nothing happens on a
failure that no installed workflow says, and how a fix is applied is
written in the maintenance workflow itself, not set by a default hidden in
the engine. There will be many kinds, offered on the marketplace next to
connectors: one that only notifies, one that restarts or pauses, one that
cleans the journal, and the doctor below.

The **doctor** is the first, a maintenance workflow that asks an AI
assistant. It subscribes to `run.failed` and `workflow.paused`:

1. **Gather**: the workflow's file and version, the failed step, its error,
   the stderr tail, the last runs.
2. **Ask**: `http.request@1` to an assistant's API (the key as
   `${env.ANTHROPIC_API_KEY}`), or `command@1` running a local assistant's
   CLI. The answer is a proposed new workflow file, or a fix to an action.
3. **Check**: `itero check` on the proposal, then a dry run (`itero daemon
   --once` against a copy of the journal, or the workflow's test fixtures).
4. **Apply** or **ask**, as the doctor's own transitions say: one doctor
   shows the proposal and waits in a state for an approval event; another
   applies it as soon as the check and the dry run pass.

Guards:

- The choice between asking and applying is explicit, in the maintenance
  workflow a workspace installs; the engine has no recovery setting.
- Proposals are files (`.itero/proposals/<workflow>/<id>.json`) and, when
  the workflows folder is a git repository, a commit on a branch: never an
  edit in place that no one can see or undo.
- At most 3 fix attempts per failure; then the workflow stays paused and a
  person is told.
- The doctor never repairs itself.
- Secrets never reach the assistant: they are never in the journal or the
  files it is given.
- A fix is a new version: runs stay pinned to theirs, the failed runs can be
  run again on the new one, and `git revert` undoes it.

A workflow can also handle its own failure (`on_fail` to a state whose
chain asks the assistant); a separate maintenance workflow is the
recommended way, since a broken workflow is the last thing to trust with
its own repair.

## 17. How the outside world reaches itero

Software learns that something happened in three ways:

| Way | Means | Examples |
|---|---|---|
| **push** | the thing tells us | webhooks, message queues (MQTT from sensors, NATS), IMAP IDLE, OS notifications |
| **watch** | we see a change as it happens | file system events, a socket, a process that exits |
| **poll** | we ask again and again | v1's 5-minute check; the fallback when a system gives no feedback |

People get signals through devices (phone, watch, TV) and channels (calls,
mail, messages); machines through sensors, queues, motors and circuits;
software through applications, which may or may not expose what happens in
them, and networks.

## 18. Sources and connectors

**Decided**: sources of all three kinds, offered as parts of connectors; a
workspace may use any or all of them at once. The term is **connector**.
The formats are proposed.

### A source

A **source** turns signals from outside into itero events. Unlike an
action, which runs once and exits, a source lives as long as the daemon.

The Unix way: **a source is a program that prints one JSON event per line
on stdout**:

```json
{"type": "mail.received", "data": {"from": "...", "subject": "..."}}
```

- It gets its settings as one JSON line on stdin when it starts.
- itero supervises it: starts it with the daemon, restarts it with a
  growing wait when it exits, and pauses it after too many restarts (a
  guard, logged).
- Each line becomes an event in the journal, its CloudEvents `source` the
  source's name, for the workspace (`itero.app`).
- **A cursor**: a line `{"cursor": ...}` is kept in the journal and given
  back on the next start, so a source resumes where it stopped, with no
  duplicates (an IMAP UID, a file offset).
- stderr goes to the daemon's log.

Built-in sources (**proposed**):

| Source | Kind | Does |
|---|---|---|
| `watch@1` | watch | a file or folder changed (FSEvents or kqueue on macOS, inotify on Linux); debounced (500 ms quiet) and coalesced; data `{path, kind}` |
| `poll@1` | poll | runs a program every interval; its stdout lines are events |
| `hook@1` | push | `POST /hooks/<name>` on the local API, for webhooks relayed to this machine |

A workspace declares its sources in `sources/<name>.json`
(`sources = ["sources"]` in `itero.toml`):

```json
{ "source": "watch@1", "with": { "path": "/var/mail/me" }, "emit": "mail.changed" }
```

**Open**: the file watching crate. `notify` is CC0-1.0, outside the
MIT-or-Apache-2.0 rule for crates. Either the owner allows it, or itero
uses the OS's own file events.

| | `notify` | the OS's own events |
|---|---|---|
| Code to write | little: one crate, mature, used widely | a few hundred lines: kqueue (macOS) and inotify (Linux) through `libc`, already a dependency |
| License | CC0-1.0: a public domain dedication that grants no patent license; an exception to the rule | nothing new |
| Reach | macOS, Linux, Windows, BSD; recursive folders; a polling fallback | what itero needs: macOS and Linux, a file or a folder; recursive folders on macOS later need FSEvents |
| Edge cases | handled by the crate (renames, atomic saves, queue overflow) | ours to handle and test on both systems |
| Debouncing | a separate crate (`notify-debouncer-full`) | ours, in the watch source |
| Releases | 9.0 is a release candidate; 8 is the stable line | none to follow |

Recommendation: the OS's own events. v2 watches one file, itero runs only
on macOS and Linux, and the edge cases are few for that scope, so the
rule holds at a small cost. `notify` stays the fallback if folder watching
grows into a large part of the sources.

### A connector

A **connector** is everything for one outside system (the stakeholder):
the unit people share, and what a marketplace offers.

```
connectors/mail/
  connector.json    name (dev.aegea.mail), version, the settings and secrets it needs
  sources/          push: imap-idle; watch: mbox; poll: imap-check
  actions/          mail.send, mail.mark-read
  workflows/        ready-made: notify about new mail
```

A workspace lists connectors in `itero.toml` and switches on the parts it
wants: the IMAP push where the server supports IDLE, the poll where it does
not, the mbox watch for local mail. All of a connector's sources emit the
same events (`mail.received`), so a workflow never knows, or cares, which
one fired.

## 19. v2: the mail check on events

**Decided** as the next step.

v1 runs every 5 minutes. v2 runs when the mailbox changes:

```json
{ "source": "watch@1", "with": { "path": "/var/mail/me" }, "emit": "mail.changed" }
```

```json
{
  "name": "mail-check",
  "description": "Notifies about mail not seen before, as it arrives.",
  "concurrency": { "max": 1, "then": "queue" },
  "triggers": [ { "on": "mail.changed" }, { "schedule": "0 * * * *" } ],
  "states": ["start", "done"],
  "start": "start",
  "end": ["done"],
  "transitions": [
    { "from": "start", "to": "done",
      "do": [
        { "id": "check", "action": "mail.check@1",
          "with": { "mailbox": "/var/mail/me",
                    "seen": "${has(memory.seen) ? memory.seen : ''}" } },
        { "action": "notify@1", "when": "steps.check.json.new > 0",
          "with": { "title": "Mail", "text": "${steps.check.json.summary}" } },
        { "action": "memory.set@1", "with": { "seen": "${steps.check.json.seen}" } }
      ] }
  ]
}
```

- The watch source fires within a second of mail arriving.
- `concurrency` keeps two quick changes from notifying twice.
- The hourly schedule stays as a safety net (a missed file event, a
  mailbox replaced while the daemon was down): sources of different kinds
  side by side, as section 18 says.
- `mail.check@1` and the memory work as in v1.

Done when: a mail sent locally gives a notification within 2 seconds; two
mails a moment apart never notify about the same message twice; a daemon
restart loses and repeats nothing; a watcher that dies is restarted, and
one that keeps dying is paused and logged. Tests write to a temporary mbox
file and stub the notifier, as v1's do.

## 20. The order

1. **Iteration 4, v2**: sources (`watch@1`, program sources, supervision,
   cursors), `sources/` in a workspace, `concurrency`, the breaker.
2. **Iteration 5, publish and subscribe**: deliveries, `match`, `publish`,
   `"from": "*"`, long-lived runs, `run.failed` and `workflow.paused`,
   `itero errors`.
3. **Iteration 6, steps side by side**: `wait` steps, `all`, `race`, `some`,
   `join`, nesting.
4. **Iteration 7, maintenance workflows**: the doctor, proposals, checks,
   dry runs.
5. **Later**: connectors as packages and the marketplace, `poll@1` and
   `hook@1`, NATS, `retry` and `undo`, `at` timers, the inbox, primum as an
   app, journal compaction.

Answered on 2026-10-16: names for people and UUIDs for the machine
(section 11), explicit keys (section 14), the group names for now (section
13), explicit maintenance workflows (section 16).

Open: the `notify` crate's CC0 license, or the OS's own file events
(section 18).
