---
title: Glossary
description: itero's terms in plain words, in the order you meet them.
---

## Where things live

### workspace

A folder with an `itero.toml`. One daemon and one port per workspace; keep
work and home apart as two workspaces. Nothing is machine-wide: itero finds
the workspace from the folder it runs in.

### itero.toml

The workspace's settings: the port, and the folders that hold workflows,
actions and triggers. A folder may be anywhere, so an app's repository can
bring its own workflows. See [itero.toml](/reference/itero-toml/).

### journal

`.itero/journal.sqlite`: every event, run, job and answer, and each
workflow's memory. The one place itero keeps state. Local to the machine,
never synced.

## What runs

### workflow

A template of a state machine: its states, where a run starts, where it
ends, and the transitions between states. Written in JSON and checked
strictly: an unknown key is an error. A plain file, so a project keeps its
workflows in git and changes them as it likes; itero only reads them. See
[Writing a workflow](/guides/workflows/).

### version

A hash of what a workflow does, its descriptions and triggers left out.
`itero check` prints it.

### state

Where a run is. A workflow lists its own states; `failed` is the engine's.
A run's state is written to the journal, in the same transaction as the
decision that changed it, so it survives a crash or a restart.

### transition

A move from one state to another. One is taken:

- **when the run enters its `from` state**, for a transition without `on`
  (the previous transition finished, or the run just started);
- **when an event arrives**, for a transition with `on` that event's type,
  sent to the run;
- **only if its `when` holds**, in either case: a condition over the run's
  values;
- **when time runs out**: a transition with a `timeout` moves to its
  `on_timeout` state instead, when its event did not come or its chain did
  not finish in time;
- **when an action fails**: the run moves to the transition's `on_fail`
  state, or to `failed`.

A transition may run a chain of actions (`do`) on the way.

### run

One instance of a workflow. It starts in `start`, moves along transitions,
and ends in an `end` state or `failed`. A run is pinned to the workflow's
version it started with: editing a workflow never changes a run under way.
A recurring job is many short runs, not one long one.

### trigger

What starts a run: a `schedule` (cron, the machine's local time) or `on` an
event type. It gives the run its `args`. See [Triggers](/guides/triggers/).

### workspace triggers

`triggers/<workflow>.json`: triggers the workspace adds to a workflow's own,
or uses instead of them. When a workflow runs is the workspace's business.

### concurrency

How many runs of a workflow may be under way at once (`"concurrency":
{"max": 1}`); a start beyond that waits its turn (`queue`) or is dropped
(`skip`). It keeps two quick events from running the same check twice.

### pause (the breaker)

After `pause_after` failed runs in a row (5 unless said), a workflow is
paused: nothing starts it until `itero resume`, or a new version of its
file. The engine sends `workflow.paused`, which a maintenance workflow can
act on.

## What happens in a run

### action

One step a transition runs, named with its version, like `notify@1`:

- **built-in**: `command@1`, `sh@1`, `http.request@1`, `notify@1`,
  `memory.set@1`, `json.pick@1`, `event.emit@1`;
- **the workspace's own**: a folder with an `action.json` and a program
  (an executable, Python, Bun, or a program in a container image), named
  `<name>.<target>@<version>`.

See [Actions](/guides/actions/).

### container action

An action whose program runs in a Docker image (`"type": "container"`):
the job on stdin, the action's folder at `/action`, its settings passed by
name so no value stands on a command line. The machine needs Docker, not
the program's language.

### chain

A transition's `do`: its actions, run in order. Each gets the previous one's
output as `prev`; one with an `id` is `steps.<id>` for the rest of the
chain. `when` skips an action, `expect` judges its output.

### input

What a transition starts with: the event's data when an event took it, else
the output that ended the previous transition. A run's first transition
gets the trigger's event data.

### expression

A [CEL](https://cel.dev) expression: a condition (`when`, `expect`) or a
`${...}` template in an action's `with`. It sees `args`, `input`, `prev`,
`steps`, `memory` and `run`.

### memory

A small store of keys and JSON values per workflow, for its own bookkeeping
("seen up to"), never an app's records. Changed by `memory.set@1`, only
when the whole chain succeeds.

### secret

`${env.NAME}`: a value from the OS secret store, or the daemon's
environment. Filled in only when the job starts, so it never reaches the
journal. See [Secrets](/guides/secrets/).

## Who does the work

### daemon

`itero daemon`: makes passes, one a second. A pass saves new events, fires
schedules and timers, asks the manager what each run does next, saves each
answer in one transaction, and hands jobs to workers. It decides nothing
itself; with no workflows it is idle.

### manager

Decides what a run does next. Pure: JSON in, JSON out, no files, no network,
no clock. The same input always gives the same answer.

### worker

Runs one job in its own process, and exits. Dumb: the request, the command
or the program, as the job says.

### job

One action handed to a worker. It carries an idempotency key, the same
when it runs again. A lost job (killed, its lease ran out, the daemon
stopped) runs again, up to 3 times; an error fails the action at once.
`limits.timeout` caps it, `limits.heartbeat` makes it a lease.

## How the outside connects

### event

A typed message, like `photo.added`, with data. Sent to the workspace, it
starts runs of the workflows with a trigger `on` its type; sent to a run, it
may take one of the run's transitions. Types starting with `action.`,
`timer.`, `run.`, `workflow.` or `source.` are the engine's own and refused
from outside. See [Apps and events](/guides/apps/).

### source

A long-lived program that tells itero when something happens outside: it
watches a file, holds a connection to a server (IMAP IDLE), or asks again
and again, and prints one event per line. The daemon runs each source in
`sources/`, starts it again when it ends, and gives up on one that keeps
ending. `watch@1` is built in. See [Sources](/guides/sources/).

### cursor

Where a source stopped, as it said last (`{"cursor": ...}`): kept in the
journal and given back when it starts again, so it carries on without
repeating itself.

### connector

Everything for one outside system packaged together: its sources, its
actions and ready-made workflows, like a mail connector with IMAP IDLE,
POP3 and the local mailbox. Planned: the unit a marketplace would offer.

### app

Anything outside itero that works with it. Events in (the HTTP API or
`itero emit`), actions out (its command line or API). Never a library:
itero orchestrates, and never owns an app's data.

### HTTP API

The daemon's API on 127.0.0.1 only: `/health`, `/events`, `/runs`,
`/workflows`. A request naming another host is refused. See
[HTTP API](/reference/http/).

### service

`itero service install`: the daemon starts at login (launchd on macOS,
systemd on Linux), kept up by the **watchdog**, which restarts it when it
exits, stops answering or stops making passes. See
[Start at login](/guides/service/).

### Itero.app

The macOS notifier: notifications come from itero, with its own entry in
System Settings.

## Principles

- **The Unix way.** Steps lean on the command line tools already there.
  Values reach a command only as environment variables, never pasted into
  its text.
- **Events in, actions out.** Apps talk to itero through events; itero acts
  on apps through their own interfaces.
- **Files, and one journal.** Workflows, actions and triggers are files;
  everything that happens is in the journal.
