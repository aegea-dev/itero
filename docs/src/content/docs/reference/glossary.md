---
title: Glossary
description: itero's terms in plain words.
---

### workspace

A folder with an `itero.config.json`, its manifest. One daemon runs it
at a time. itero finds the workspace from the folder it runs in. The
machine's list holds the workspaces one `itero daemon --all` runs
([One itero per machine](/guides/machine/)).

### workflow <span class="wf">●</span>

A JSON file in `workflows/` or a subfolder of it, named by its file name:
when it runs (`on`) and its `steps`, with optional `success`,
`failure` and `error` lists and a `timeout`. A workflow can be a step of another. See
[Workflows](/guides/workflows/).

### step

One thing a workflow does: an action (`#name`), a workflow (`name`), a
list or group of them side by side, or one of those with its `args` and
an `on`.

### action <span class="act">■</span>

An executable file in `actions/` or a subfolder of it, any language, named
by its file name, a version in it if any (`notify@0.2`). It reads its values
as environment variables, prints its result (`$output`), and its exit code
says how it went: 0 done, 75 not yet, anything else an error. See
[Actions](/guides/actions/).

### subfolder

A folder in `workflows/`, `actions/` or `connectors/` that groups files; it
is never part of a name. Hidden folders and `node_modules` are not looked
into, and files that are not executable are not actions or connectors.
[`itero which`](/commands/which/) says where a name is.

### needs

What an action needs and lacks: a program its first line or a shell
command runs, or a Python, Node or Bun package. Named in the run's error
when a step fails, and by `itero check` before. See
[Actions](/guides/actions/#when-something-it-needs-is-missing).

### event

Also called a message: a type (two or more words joined by dots, like
`order.placed`) and its data. It starts runs and reaches runs waiting for
it. See [Connectors](/guides/connectors/).

### connector <span class="con">▲</span>

An executable in `connectors/` or a subfolder of it that keeps running; each
line it prints is one event of its file name's type.

### schedule

A cron line in `on`, in the machine's local time:
`{"schedule": "0 9 * * *"}`.

### path

A file or folder in `on`, watched by the daemon: `{"path": "inbox"}`.
Each change starts a run.

### run

One go through a workflow, started by an event, a schedule or a watched
path. It has an id (a UUIDv7) and a name (`misty-lake`), and keeps the
version of its workflows it started with. It ends as `done`, `failed` or
`error`, or `split` when it went on as one run per item of a list of
[args](#args).

### event

`$event`: the data of the event that started a run, the same in every
step. A workflow used as a step has its step's args as its `$event`. A
waiting action gets the event that matched it as `$event` too.

### output

`$output`: the result of the step just before, what it printed, read as
JSON when it is JSON. Each step's result takes its place.

### steps

`$steps.<name>`: an earlier step's result, by its name (`#fetch-rates` is
`$steps.fetch-rates`) or its `as`.

### state

A run's values as the journal keeps them: `event`, `output` and `steps`.
`itero wf runs <run>` shows it.

### waiting

A run whose step exited 75: it waits for an event that matches the step
(shares a field with what it printed and agrees on all it shares), which
its action then gets as `$event`; it runs the step again every hour, and gives
up after its `timeout`.

### side by side

A list in `steps`: its entries run at the same time, and the next step
waits for all of them. Their results merge into one `$output`, and each
is kept as `$steps.<name>`. An object
says how else to wait: `race`, `any`, `some`, `settled`; the steps that
lose are stopped. A list whose step breaks breaks the run once the others
have ended. See
[How steps side by side wait](/guides/workflows/#how-steps-side-by-side-wait).

### condition

An entry of `on` in a workflow or a step: `{"has": "$event.order"}`
(there), `{"has": "$$NAME"}` (defined), or `{"$event.kind": value}`
(exactly). A step whose `on` does not hold is
skipped.

### args

What an action gets, and all it gets: a step's `args` in its workflow.
An action's `repeat` in `itero.config.json`, a list, runs the step once
per item. Args are an object or a list of objects; a list item with
exactly `title` and `text` gives the arg `title` names its text, as text.
A value in an arg's text is filled (`"$event.from: $event.subject"`). A
connector has args too, `connector.<name>.args`: an object.

### secret

`$$NAME`: from the OS secret store, else the workspace's `.env`, else the
daemon's environment, bound only when a step starts, never written down.
Set with `itero secret`; values that are not secret go in `.env`.

### memory

`$ITERO_MEMORY`: a file an action may keep things in between runs, one
per workflow.

### journal

`.itero/journal.sqlite`: every event, run and answer, and the IDs.
Local, never synced. Ended runs and their events are forgotten a day
later (`keep`).

### daemon

`itero daemon`: runs one workspace, pass after pass (`--all`: every
workspace on the machine's list, each on its own). It starts connectors,
watches paths, takes events and schedules, decides what each run does
next, and runs each step's action in its own process. See
[Workspaces, workflows and runs](/start/concepts/).

### log

`.itero/itero.log`: one JSON line for everything the daemon does. See
[The log, IDs and names](/guides/log/).

### id and name

What itero gives the workspace, each daemon's life, each workflow and
each run: a UUIDv7, and a name (two random words, or a workflow's file
name). Commands take either: `itero wf runs misty-lake`.

### meta

What a person says about a workflow (its entry) or a workspace (the manifest's top), in `itero.config.json`:
a description, `from` and `until`, `off`, `internal`, and keys of their
own.

### internal

A workflow whose entry says `"internal": true`: it looks after the
workspace. Listed apart, outside the limits. `keep`, `logs` and `models`
are built in. See [The daemon's own workflows](/guides/maintenance/).

### service

`itero service install`: the daemon at login, kept up by a watchdog. See
[Start at login](/guides/service/).

### Itero.app

The macOS notifier: notifications come from itero, with its own entry in
System Settings.
