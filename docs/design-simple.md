# itero, simple

This replaces the file formats in `design.md`. The engine underneath
stays: the journal, the daemon, and runs that survive a restart. What
people write becomes as small as it can be.

The rule is KISS:
- as few fixed words as possible;
- tools already on the machine (`sh`, `curl`, `jq`) before programs of
  our own;
- defaults instead of settings.

## A workspace

```
itero.toml                    the daemon's settings (the port)
workflows/order.json          a workflow; its name is the file name
actions/check-payment         an action: an executable; its name is the file name
actions/notify@0.2            a version, in the name
sources/imap-idle             a source: an executable holding a connection open
args/check-payment.json       data for an action, written by `itero args`
.itero/                       what itero keeps: the journal, the log, memory
```

Everything a workspace's programs write goes in `.itero/`; the rest of
the folder is what people write.

## Names at a glance

| Written | Is |
|---|---|
| `#check-payment` | an action |
| `order-process` | a workflow |
| `order.placed` | a message type: two or more words joined by `.` |
| `$order` | a value in the run's state |
| `$$API_KEY` | an environment variable or a secret |

## A workflow

```json
{
  "on": [{ "event": "order.placed" }],
  "steps": [
    ["#check-payment", "#check-stock"],
    "order-process",
    "#send-mail"
  ],
  "success": ["#log-result"],
  "failure": ["#log-result"],
  "error": ["#log-result", "#alert-team"]
}
```

Its words: `on`, `steps`, and optionally `success`, `failure`, `error`
and `timeout`.

### `on`: when it runs

A list of objects. Each says what it is with its key:

```json
"on": [
  { "event": "order.placed" },
  { "schedule": "0 9 * * *" },
  { "path": "/var/mail/$$USER" },
  { "$paid": true },
  { "has": "$order" },
  { "has": ["$total", "$$API"] }
]
```

| Entry | Means |
|---|---|
| `{"event": "order.placed"}` | a message of this type starts a run |
| `{"schedule": "0 9 * * *"}` | the schedule starts a run (cron, local time) |
| `{"path": "/var/mail/$$USER"}` | a change to this file or folder starts a run (see Watching files) |
| `{"$paid": true}` | only if the state's `paid` is exactly `true` |
| `{"has": "$order"}` | only if the state has `order` |
| `{"has": "$$API"}` | only if `API` is defined, as an environment variable or a secret |

**One rule:** `event`, `schedule` and `path` entries each start a run,
any one of them. Every other entry is a condition, and they must all
hold. In one object, every value must hold: `{"$paid": true, "$stock":
true}`. `has` takes one name or a list. Values compare exactly: `0` is not
`false`.

A workflow with no `event`, `schedule` or `path` in its `on` never starts
by itself: it runs only as a step of another workflow, and only if its
conditions hold there.

### `steps`: what it does

They run in order. A step is:

- **an action**, `"#check-payment"`, or **a workflow**, `"order-process"`.
  A workflow used as a step shares the run's state;
- **a list**, `["#check-payment", "#check-stock"]`: those run side by
  side, and the next step waits for all of them. A list can hold actions
  and workflows: `["#check-payment", "stock-check"]`;
- **an object**, an action or workflow with data for it and an optional
  `on`, its conditions:

  ```json
  { "#ship": { "carrier": "PTT", "on": [{ "$paid": true, "$stock": true }] } }
  ```

  The step runs only if its conditions hold, and is skipped otherwise. The
  other keys are data for the action. A value that is exactly `"$name"` or
  `"$$NAME"` is replaced by that value:
  `{ "#ask-ai": { "key": "$$ANTHROPIC_KEY" } }`.

### `success`, `failure`, `error`: how it ended

Steps run at the end, by the outcome:

- **error**: something broke (an action exited with a code other than 0
  or 75, took too long, or a run waited too long). The steps stop at once
  and `error` runs.
- **failure**: the steps all ran, and the state says `"success": false`.
  That is a business outcome, not a broken workflow: out of stock, payment
  declined. `failure` runs.
- **success**: anything else. `success` runs.

The run ends as `done`, `failed` or `error`. These steps get `$why` and
`$failed_step` in the state. If one of them breaks, the run ends as
`error` and no other list runs.

### `timeout`

How long a run may wait for data (see exit 75), like `"2h"`. Unless
given, 1 day. A run still waiting then ends in `error`.

## An action

An executable file in `actions/`, in any language its first line names
(`#!/bin/sh`, `#!/usr/bin/env python3`), with no extension and no
description file.

- **Its name** is its file name. A version is part of it: `notify@0.2`.
  `#notify` is the highest version; `#notify@0.2` pins one.
- **It reads** its values as environment variables (`$host`, `$user`) and
  the run's whole state as JSON on stdin. A value that is not a string
  arrives as JSON text.
- **It writes** a JSON object on stdout, whose fields join the state. A
  field set to `null` is removed. Printing nothing adds nothing.
- **Its exit code:**
  - `0`: done;
  - `75`: not yet (the standard Unix "try again later"). What it printed
    still joins the state. The run waits, and the step runs again when
    matching data reaches the run, or every hour, with
    `$ITERO_WAITING_SINCE` set so it can decide to give up;
  - anything else: an error; why is the end of what it printed on stderr.
- **What it keeps between runs** (like "the last mail seen", for a
  workflow that polls) goes in the file `$ITERO_MEMORY`, one per workflow,
  in `.itero/`. Two runs of a polling workflow overlap only if one takes
  longer than its schedule's interval; itero does not guard against that.
- **It runs in the workspace,** and a file it writes there belongs to the
  person; output of its own goes in `.itero/` (the examples write theirs
  there).

`#notify` is built in: it shows `$text` (and `$title`) as a desktop
notification. A workspace action named `notify` replaces it.

Example, `actions/check-payment`:

```sh
#!/bin/sh
# asks the provider; a pending payment comes back later as payment.received
[ -n "$paid" ] && { echo "{\"paid\": $paid}"; exit 0; }
ref=$(curl -fsS --max-time 30 "https://pay.example.com/orders/$order" | jq -r .ref)
echo "{\"payment_ref\": \"$ref\"}"
exit 75
```

### Side by side

Actions and workflows in a list run at the same time, each starting from
the same state. A field that only one of them prints joins the state as it
is. A field that more than one prints is kept per name:

```json
"status": { "#check-payment": "ok", "stock-check": "low" }
```

## Values

An action's environment variables come from, in this order (the first
found wins):

1. **the step's data** in the workflow;
2. **its args**, `args/<action>.json`;
3. **the run's state.**

Secrets and environment variables enter only by name, with `$$`, in a
step's data, in args, or in a `path`. A secret is looked up in the OS
secret store (the macOS Keychain, the Secret Service on Linux), then in
the daemon's environment. It is never written into the state, the
journal, the log or a command line.

```sh
itero args check-payment '{"provider": "pay.example.com"}'
itero args check-payment --file args.json
itero secret API_KEY                    # asks for it, without echo
itero secret API_KEY --file key.txt
```

**A list in args runs the step once per item**, each with its own copy of
the state for the rest of the run:

```sh
itero args imap-check '[{"user": "me@example.com", "password": "$$ME_PASS"},
                        {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

## Messages

A message is a type and its data:

```sh
itero emit order.placed '{"order": 43, "total": 120}'
```
```
POST /events   {"type": "order.placed", "data": {"order": 43, "total": 120}}
```

`itero emit` says at once what the message will do, from the workflows on
disk: which workflows it starts, that it may reach waiting runs, or that
nothing starts on its type, with the nearest type that does
(`order.places`: did you mean `order.placed`?). If no daemon is running,
it says the message waits for one.

Messages come from:
- **push**: an app or a webhook calls `POST /events`, or a source holds a
  connection open and prints a line when told something;
- **pull**: a workflow on a schedule asks;
- **watch**: a `path` in a workflow's `on` (see Watching files).

A **source** is an executable in `sources/` that keeps running, for what
needs a connection held open: a mail server's IDLE, a queue
subscription. Each line it prints is the data of one message, of its file
name's type. The daemon starts it again when it stops. A source that
keeps its own "last seen" sends one message per new thing, so each run has
its own: one line per new mail, with its sender and subject.

What a message does:
- **It starts a run** of every workflow with its type in an `event` entry
  (and whose conditions hold), all in parallel. Its data is the run's
  first state.
- **It reaches waiting runs** that it matches: it shares at least one
  field with the run's state, and agrees with it on every field they
  share. Every run it matches gets it. A run that starts waiting also gets
  the matching messages that arrived since the run began, so none is
  missed by arriving early.
- **It is handled once**: the daemon marks it handled in the same
  transaction as the runs it starts and the copies it sends.
- **It is kept** until its runs end, then for `keep` (1 day unless set), then `keep` forgets it.

## Watching files

A `path` entry is watched by the daemon itself, with the OS's own file
events: kqueue on macOS, inotify on Linux. One watcher holds every path of
every workflow, in one thread: a path costs a file descriptor, and no CPU
until something changes. No process per watch.

- Each change starts a run with `{"path": "...", "kind": "changed"}`
  (`created`, `removed`); a burst of changes is one, once the path has
  been quiet for half a second.
- A path that is not there yet is watched for when it comes; its folder
  must be there.
- `$$NAME` in a path is an environment variable, like `$$USER`, filled in
  when the daemon reads the workflow. The path, filled in, is in the run's
  data and in the log, so a secret never goes in one. `~/` is the home
  folder; a relative path is in the workspace.
- The watcher reads the workflows' paths again every second, so editing a
  workflow moves its watch. A change is sent as the daemon's own message
  `itero.path.changed` `{"path", "kind"}`, which starts the workflows
  naming that path.

`itero watch <path>` stays, for a source that wants the same events.

## How it runs

One program per workspace, the daemon, on this machine. What it knows is
in `.itero/journal.sqlite`, so it can stop and start without losing
anything.

Something happens: a message arrives, a schedule comes due, a watched
path changes, or a waiting run's timer fires. Then the daemon makes a
pass, at once or within a second:

```
itero daemon  (one per workspace)
│
├─ sources and watched paths: beside it, feeding messages in
│
└─ every pass:
     1. messages   → start new runs, wake waiting runs
     2. runs       → decide each run's next steps
     3. steps      → run each step's action, side by side (up to 8 at a time),
                     each as its own process
     4. results    → saved; they are the next pass's input
```

A run moves through its workflow's steps:

```
run:  started ─► step ─► step ─► … ─► success / failure / error list ─► ended
                   │
                   └─ exit 75: waiting ─► (a matching message, or the hourly retry) ─► the step again
```

Every arrow is written to the journal before it happens, so a crash at any
point resumes there, with nothing lost and nothing done twice.

The docs explain itero with this picture: workspace, workflows, runs,
steps, the daemon and its passes. How a pass decides and runs steps
inside (separate short programs) is for `design.md`, not for people
writing workflows.

## IDs

Everything gets a UUIDv7, so it can be found and followed:

| ID | Of | Made |
|---|---|---|
| `workspace` | the workspace: its journal and history | once, when its journal is made; kept in it |
| `daemon` | one life of the daemon | each time it starts |
| `run` | one run | when it starts |
| `version` | the workflows a run is pinned to | the first time those files are seen |
| `message` | one message | when it is sent |

A new daemon carries on with the same journal: the runs, messages and
timers are the workspace's, and only the `daemon` ID is new. Deleting
`.itero/` starts a new history with a new `workspace` ID; copying a folder
with its `.itero/` copies its ID too, since it is the same history.
`/health` shows the `workspace` and `daemon` IDs.

## Metadata

Every workspace, daemon life, workflow and run has an identity itero gives
it, which nobody edits, and a few settings a person may give it.

### Given by itero (read-only)

| | Workspace | Daemon (each life) | Workflow | Run |
|---|---|---|---|---|
| `id` | UUIDv7, made with its journal | UUIDv7, at its start | UUIDv7, the first time its file is seen | UUIDv7, at its start |
| `name` | two random words: `quiet-river` | two random words: `brave-otter` | its file name: `order` | two random words: `misty-lake` |
| `created` | when its journal was made | when it started | when its file was first seen | when it started |
| `path` | the folder | the folder it runs | its file | |
| also | the itero version that made it | its pid, host and itero version | its current version, its last run and how it ended, its runs so far | its workflow and status |

- They are kept in the journal, never in files a person writes.
- A random name is an adjective and a noun, lowercase, joined by `-`,
  picked from built-in lists. It is a label to say aloud and type; the
  UUID stays the identity, and two with one name are told apart by it.
  Commands take either: `itero runs misty-lake`.
- A workflow's `id` follows its name: editing the file keeps the `id` and
  gives a new `version`; deleting the file and writing it again keeps it
  too, since the name is the same.

### Set by a person

Metadata is the read-only part above, plus what a person adds: a few keys
itero knows, and any others they like.

**A workflow:** an optional `meta` object, so the top level stays `on`
and `steps`:

```json
{
  "meta": {
    "description": "Orders from the shop",
    "from": "2026-11-01", "until": "2026-12-31", "off": false,
    "owner": "ayşe", "team": "shop", "tags": ["orders", "payments"]
  },
  "on": [{ "event": "order.placed" }],
  "steps": ["#check-payment", "#ship"]
}
```

| Key | Means |
|---|---|
| `description` | text for people |
| `from`, `until` | dates, or times in RFC 3339: outside them nothing starts it; runs under way finish |
| `off` | `true`: nothing starts it, without deleting it |
| `internal` | `true`: it looks after the workspace; listed apart, its runs hidden unless asked, outside the limits (see The daemon's own workflows) |
| any other | the person's own: text, a number, a list or an object; itero keeps it and shows it, and does nothing with it |

The read-only names (`id`, `name`, `created`, `path`, `version`) cannot be
set: `itero check` refuses them, naming the file.

**The workspace:** `itero.toml`, its `[meta]` the same way:

```toml
port = 8787

[meta]
description = "Home: mail and orders"
owner = "ayşe"
machine = "the mini in the hall"

[limits]
workflows = 100     # more is refused by itero check and at the daemon's start
runs = 1000         # runs under way at once; past it, messages wait their turn
steps = 8           # steps running at once
space = "1GB"       # .itero/: the journal, the log, memory
keep = "1d"         # how long ended runs and their messages are kept
log = "10MB"        # past it, the log is rolled (see The daemon's own workflows)
logs = 3            # rolled logs kept: itero.log.1 to itero.log.3
```

Each limit has the default shown. Past `space`, the daemon sends
`itero.space.full` and logs an `error` line, once, and its `keep`
workflow forgets every ended run, not only those past `keep` (see The
daemon's own workflows); until there is room, no new run starts but
internal ones, and the messages that would start one wait their turn.

### Where to see them

- `itero info`: the workspace (its read-only part and its `[meta]`) and the daemon running it, if any.
- `itero check`: each workflow's read-only part and its `meta`, the person's keys included.
- `/health`: the workspace and the daemon.
- The log: `daemon.started` carries every field of the workspace and the
  daemon; other lines carry their IDs, and their names too.

## The log

`.itero/itero.log`: one JSON object per line, written by the daemon,
everything that happens:

```json
{"time":"2026-10-21T13:43:24Z","workspace":"01a0f3…","daemon":"01a15f2c…","event":"run.started","workflow":"order","run":"01a15eb3…","message":"01a15eb2…"}
{"time":"2026-10-21T13:43:25Z","workspace":"01a0f3…","daemon":"01a15f2c…","event":"step.waiting","workflow":"order","run":"01a15eb3…","step":"#check-payment","output":{"payment_ref":"pay-1"}}
{"time":"2026-10-21T13:45:02Z","workspace":"01a0f3…","daemon":"01a15f2c…","event":"step.done","workflow":"order","run":"01a15eb3…","step":"#ship","output":{"tracking":"TR-1"}}
```

Every line has `time`, `workspace`, `daemon` and `event`; a line about a
run has `workflow`, `run` and `run_name`, and `"internal": true` when its
workflow is internal; the rest depends on the event:

| `event` | Also |
|---|---|
| `daemon.started` | every field of the workspace and the daemon |
| `daemon.stopped` | |
| `message` | `message`, `type`, `data`, and what it did: `started` (runs), `reached` (runs), or `nothing` |
| `run.started` | `message` (a path's change is the message `itero.path.changed`), or `schedule` |
| `step.started` | `step` |
| `step.done`, `step.waiting` | `step`, `output` |
| `step.failed` | `step`, `why` |
| `step.skipped` | `step`, `why` (its conditions did not hold) |
| `run.note` | `note`: what the run did with a message or a timer it was given |
| `run.ended` | `status` (`done`, `failed`, `error`, `split`), `why` |
| `run.skipped` | `workflow`, `schedule`, `why`: a schedule came due and `on` did not hold |
| `source.started`, `source.ended`, `source.gave-up` | `source`, `why` |
| `paths.watched` | `paths`: every path watched, when they change |
| `error` | `why`: what went wrong in the daemon itself |

- **It holds no secrets:** only what steps printed, never their
  environment.
- **It does not grow for ever:** past its `log` limit (10 MB) the daemon
  sends `itero.log.full`, and the `logs` workflow rolls it:
  `itero.log` becomes `itero.log.1`, `.1` becomes `.2`, and so on, keeping
  `logs` of them (3). A workspace can roll its own way instead: compress
  them, send them somewhere. The daemon opens the log for each line, so a
  file renamed under it is no trouble.
- **Reading it is `jq`'s job:**
  ```sh
  jq 'select(.run == "01a15eb3…")' .itero/itero.log            # one run
  jq 'select(.event == "step.failed")' .itero/itero.log         # what broke
  jq -r '"\(.time) \(.event) \(.workflow // "") \(.step // "")"' .itero/itero.log
  tail -f .itero/itero.log | jq .                              # live
  ```
- `.itero/daemon.log` stays only for what the service catches when the
  daemon itself crashes.

## Runs

- Each run has a UUIDv7 id, and keeps the workflow version it started
  with (also a UUIDv7): editing a file never changes a run under way.
- Everything runs in parallel: every run, every workflow.
- A step may take 10 minutes. Past that, or when the daemon stops while it
  runs, it is tried again, up to 3 times; then it is an error.

## The daemon's own workflows

The daemon looks after itself with the same parts as everything else:
messages about what it notices, and workflows that handle them. Nothing
is hidden in it: its own workflows show in `itero check`, `itero runs`
and the log like any other, and a workspace changes or turns them off the
same way.

**Messages from the daemon.** Their types start with `itero.`, which only
the daemon may send (refused from `itero emit`, `/events` and sources):

| Type | When | Data |
|---|---|---|
| `itero.daemon.started` | the daemon starts | `daemon`, `name`, `itero` (its version) |
| `itero.run.error` | a run ended in error | `workflow`, `run`, `failed_step`, `why` |
| `itero.run.failed` | a run ended as failed | `workflow`, `run`, `why` |
| `itero.source.gave-up` | a source kept ending and was given up on | `source`, `why` |
| `itero.space.full` | `.itero/` is past its `space` limit | `used`, `limit` |
| `itero.log.full` | the log is past its `log` limit | `path`, `size`, `limit` |

A message about a workflow's run never starts a run of that same workflow
(when it is about an error there), so a broken workflow cannot set itself
off again and again. No run waits for the daemon's own messages: they
only start workflows. `itero.path.changed` `{"path", "kind"}` is one too:
the change of a watched path (see Watching files).

**Internal workflows.** A workflow with `"meta": {"internal": true}` looks
after the workspace rather than doing its work. Nothing in its name says
so: any workflow can be internal. Being internal:

- `itero check` lists it apart, as internal;
- `itero runs` and `GET /runs` leave its runs out unless asked
  (`itero runs --all`, `?all=true`); `itero errors` shows its errors like
  any other's;
- its runs do not count against `limits.runs`, nor itself against
  `limits.workflows`;
- its lines in the log carry `"internal": true`.

**Built-in workflows.** itero carries two, internal, and runs them as if
they were in `workflows/`:

```json
{ "meta": { "internal": true, "description": "Forgets what ended long enough ago" },
  "on": [{ "schedule": "0 * * * *" }, { "event": "itero.space.full" }],
  "steps": ["#forget"] }
```

| Workflow | On | Does |
|---|---|---|
| `keep` | every hour, and `itero.space.full` | `#forget`: forgets ended runs past `keep` (sooner, when space is short) |
| `logs` | `itero.log.full` | `#roll-log`: rolls the log, keeping `logs` of them |

Their steps are built-in actions, like `#notify`: `#forget` and
`#roll-log`.

A workspace's own maintenance is the same thing: for example, rolling the
log and then compressing and sending away the oldest, with its own
`workflows/logs.json`:

```json
{ "meta": { "internal": true },
  "on": [{ "event": "itero.log.full" }],
  "steps": ["#roll-log", "#compress-old-logs", "#upload-old-logs"] }
```

**A workspace's own**, for what it wants done: a workflow on the daemon's
messages, written like any other:

```json
{ "on": [{ "event": "itero.run.error" }],
  "steps": [{ "#notify": { "title": "itero", "text": "$why" } }] }
```

- **Changing a built-in workflow:** a file of its name in `workflows/`
  (`workflows/keep.json`) is used instead.
- **Turning one off:** that file with `"meta": {"off": true}`.

Later, more of them, as real needs come: a doctor that asks an AI
assistant what went wrong in a failed run and proposes a fix, as the
earlier design describes.

## Commands

```
itero daemon                 run the workspace (itero service install: at login)
itero emit <type> '<data>'   send a message, and say what it starts
itero args <action> '<data>' set an action's data (or --file)
itero secret <NAME>          set a secret (asks; or --file)
itero runs                   runs, newest first, and how each ended
itero errors                 runs that ended in error: which step and why
itero check                  read every file; say what is wrong
itero workflows              each workflow, its runs and the last; whether a daemon runs
itero info                   the workspace and its daemon: IDs, names, limits
itero watch <path>           for sources: a line per change
```

## Examples

**New mail, as it arrives.** A source prints one line per new mail, with
its sender and subject. `workflows/mail-alert.json`:

```json
{ "on": [{ "event": "mail.received" }], "steps": ["#notify"] }
```

(`#notify` shows `$text`; the source prints `{"text": "Ayşe: Toplantı"}`.)

**Photos added to a folder.** No source needed:

```json
{ "on": [{ "path": "~/Pictures/inbox" }], "steps": ["#make-thumbnails"] }
```

**An IMAP mailbox, every minute** (pull). `workflows/imap-alert.json`:

```json
{
  "on": [{ "schedule": "* * * * *" }],
  "steps": [
    { "#imap-check": { "host": "imap.example.com", "user": "me@example.com", "password": "$$IMAP_PASS" } },
    { "#notify": { "on": [{ "has": "$text" }] } }
  ]
}
```

`#imap-check` is a few lines of shell around `curl` (it speaks IMAP, POP3
and SMTP), keeps the last mail seen in `$ITERO_MEMORY`, and prints
`{"text": …}` only when there is new mail. The password goes to `curl` on
stdin, never on its command line.

**An order.** `workflows/order.json` as above, and
`workflows/order-process.json`:

```json
{
  "on": [{ "has": ["$paid", "$stock"] }],
  "steps": [
    { "#ship":           { "on": [{ "$paid": true,  "$stock": true }] } },
    { "#remind-payment": { "on": [{ "$paid": false, "$stock": true }] } },
    { "#remind-stock":   { "on": [{ "$paid": true,  "$stock": false }] } },
    { "#cancel":         { "on": [{ "$paid": false, "$stock": false }] } }
  ]
}
```

1. `order.placed` with `{"order": 43, "total": 120}` starts a run.
2. `#check-payment` and `#check-stock` run side by side. The stock is
   there: `{"stock": true}`. The payment is pending: `#check-payment`
   prints `{"payment_ref": "pay_1"}` and exits 75.
3. The provider calls back: `payment.received` with
   `{"payment_ref": "pay_1", "paid": true}` matches by `payment_ref`;
   `#check-payment` runs again and prints `{"paid": true}`.
4. `order-process` runs `#ship`; `#send-mail` tells the customer;
   `success` runs `#log-result`; the run is `done`.
5. Had the stock been 0, `#remind-stock` would have printed
   `{"success": false}`, and `failure` would run instead.

**A page, summarized by an AI assistant, by mail.**
`workflows/page-digest.json`:

```json
{
  "on": [{ "schedule": "0 9 * * *" }],
  "steps": [
    { "#fetch": { "url": "https://example.com/news" } },
    { "#ask-ai": { "prompt": "Summarize this page in 5 bullets.", "key": "$$ANTHROPIC_KEY" } },
    { "#send-mail": { "to": "me@example.com", "password": "$$SMTP_PASS" } }
  ]
}
```

`#fetch` prints `{"page": …}`, `#ask-ai` sends `$prompt` and `$page` to
the assistant's API with `curl` and prints `{"text": …}`, and `#send-mail`
sends `$text` with `curl smtps://…`.

## What goes away

- `name`, `description`, `version`, and `action.json` altogether;
- `states`, `start`, `end`, `transitions`, `from`, `to`, `do`, `id`;
- `${...}` expressions, `when`, `expect`;
- `memory.set`, `concurrency`, `pause_after`, `limits`;
- workspace `triggers/`;
- the action types `python@`, `bun@` and `container` (a first line covers
  languages; a script can run `docker` itself);
- built-in actions other than `#notify`;
- `match`, `publish`, `"from": "*"`;
- `itero memory` and `itero resume` (no pausing, for now);
- the string forms of `on` (`"order.placed"`, `"$order"`) and its object
  of exact values: every entry is an object now.

They stay in git history, to come back only when a real workflow needs
one.

## Later, when needed

- **Settings for a side-by-side list**, like
  `["#a", ["#b", "#c", {"wait_all": true}]]`.
- **Pausing a workflow** after errors in a row: a daemon setting.
- **Commands to create things** without knowing the folders:
  `itero workflows new <name> '<json>'`, `itero workflows update …`,
  `itero actions new <name> --file …`, `itero sources new …`. Each writes
  the file a person would.
- **A web page** to see workflows and runs, then to edit them; xyflow for
  the graph once branches are drawn.
- **`itero sub <type>`** for apps to follow messages, and NATS when one
  machine is not enough.
- **A viewer for the log** (`itero log`), if `jq` is ever not enough.
