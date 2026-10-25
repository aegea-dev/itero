---
title: Connectors
description: Where events come from, what they start, and how connectors bring them in.
---

Workflows run when something happens. Each thing that happens is an
**event** (you may know it as a message): a type, such as `order.placed`,
and its data, a JSON object. An event comes from one of these:

- **an app or a script**: `itero emit`, or `POST /events` to the daemon;
- **a connector**: a program that keeps running and prints a line for
  each new thing it sees (a new mail, a sensor reading);
- **a schedule**: `"on": [{ "schedule": "0 9 * * *" }]` in a workflow;
- **a file that changed**: `"on": [{ "path": "~/Pictures/inbox" }]`;
- **itero itself**: `itero.run.error` when a run breaks, and others
  ([The daemon's own workflows](/guides/maintenance/)).

## Connectors <span class="con">▲</span>

A connector is an executable in `connectors/` (or a subfolder of it)
that keeps running. Each line it prints is the data of one event, of the
type its file name says: `connectors/mail/mail.received` sends
`mail.received` events.

```sh
#!/bin/sh
# an event each time the file changes
exec itero watch "$HOME/notes.txt"
```

It runs in the workspace, with the daemon's `itero` first on its `PATH`
and its args (`connector.<name>.args` in `itero.config.json`, set with
[`itero connector args set`](/commands/connector/#args)) as environment
variables, secrets among them:
[A connector's args](/guides/secrets/#a-connectors-args). Its stdin closes
when the daemon goes; its stderr goes to the daemon's stderr. The daemon
starts it again when it ends, waiting longer each time, and gives up on one
that keeps ending (`itero.connector.gave-up`); the log and `/health` show
each connector.

A connector that keeps its own "last seen" sends one event per new thing,
so each run has its own: `examples/mail-alert` prints one line per new
mail, with its sender and subject.

## Event types

A type is two or more words joined by dots, and reads best as who, then
what happened or what is asked:

| Type | Is |
|---|---|
| `gmail.received`, `order.placed` | a fact: something happened (past tense). Facts come in, from a connector or an app. |
| `gmail.send`, `app.notify` | a request: something asked for (imperative). |
| `stripe.invoice.paid` | words between add detail |

The first word is the service, the app or the thing (`gmail`, `stripe`,
`order`); the last says what. itero checks only the dots; the rest is a
convention, so that a type says at a glance which way it goes.

```sh
itero emit order.placed '{"order": 43, "total": 120}'
```

```
POST http://127.0.0.1:8787/events
Content-Type: application/json

{"type": "order.placed", "data": {"order": 43, "total": 120}}
```

It is saved at once, so one sent while the daemon is stopped waits for it.
Types starting with `itero.`, `action.`, `timer.`, `run.`, `workflow.` or
`connector.` are the engine's own and refused.

## What an event does

- **It starts a run** of every workflow whose `on` has an `event` of its
  type, whose conditions hold over its data, and that is not `off` or
  outside its `from` and `until`. Its data is the run's first state.
  Every such workflow starts, side by side. With `runs` runs under way
  ([`limits`](/reference/itero-config/#limits)), it waits its turn.
- **It reaches the runs waiting for data** (a step exited 75) that it
  matches: it shares at least one field with the run's state and agrees
  with it on every one they share. `{"payment_ref": "pay-1", "paid": true}`
  matches the run whose state has `"payment_ref": "pay-1"`, and every run
  it matches gets it. Its data joins the state, and the waiting step runs
  again.
- A run that starts waiting gets the matching events that came since it
  started, so a fast reply is not missed.

Events are kept until their runs end, then `keep` more (a day).

## Getting an answer

An event is sent and forgotten, unless its sender asks what came of it:

- **Wait for it**, for runs that end soon: `itero emit --wait` returns when
  the runs it started have ended and prints them as JSON, each with its
  status and state; `POST /events?wait=60` answers the same
  ([`itero emit`](/commands/emit/#waiting-for-the-answer)).
- **Let the workflow post it**, for runs that may wait for days: the app
  sends its URL in the event's data, and the workflow's `success`,
  `failure` and `error` steps post the result there
  ([Posting a result](/guides/workflows/#posting-a-result)).

## Watching files

```json
{ "on": [{ "path": "~/Pictures/inbox" }], "steps": ["#make-thumbnails"] }
```

The daemon watches every `path` of every workflow itself, with the OS's
own file events (kqueue on macOS, inotify on Linux), all in one thread: a
path costs a file descriptor and no CPU until something changes.

- Each change starts a run with `{"path": "...", "kind": "changed"}`
  (`created`, `removed`); a burst of changes is one, once the path has
  been quiet for half a second.
- A path that is not there yet is watched for until it comes, and so are
  its folders.
- `~/` is your home folder, `$$NAME` an environment variable (never a
  secret: the path is in the run's data and the log), and a relative path
  is in the workspace.
- Editing a workflow moves its watch at once.

`itero watch <path>` prints `{"path", "kind"}` each time a file or folder
changes, calm for half a second: for a connector that wants the same
events as a `path` in `on`. `examples/photos` makes a thumbnail of each
photo dropped into its `inbox/`.
