---
title: Messages and sources
description: Where messages come from, what they start, and how they reach the runs waiting for them.
---

A message is a type and its data. The type is two or more words joined by
dots.

```sh
itero emit order.placed '{"order": 43, "total": 120}'
```

```
POST http://127.0.0.1:8787/events
Content-Type: application/json

{"type": "order.placed", "data": {"order": 43, "total": 120}}
```

It is written to the journal at once, so one sent while the daemon is
stopped waits for it. Types starting with `itero.`, `action.`, `timer.`,
`run.`, `workflow.` or `source.` are the engine's own and refused.

`itero emit` prints the message's id, then says what it will do, from the
workflows on disk:

```
$ itero emit order.places '{"order": 43}'
01a15f2c-…
nothing starts on order.places: did you mean order.placed?
no daemon is running: it waits for itero daemon
```

It names the workflows the message starts (`starts: order`), and the runs
it may reach when some are waiting.

## What a message does

- **It starts a run** of every workflow whose `on` has an `event` of its
  type, whose conditions hold over its data, and that is not `off` or
  outside its `from` and `until`. Its data is the run's first state.
  Every such workflow starts, side by side. With `runs` runs under way
  ([`[limits]`](/reference/itero-toml/#limits)), it waits its turn.
- **It reaches the runs waiting for data** (a step exited 75) that it
  matches: it shares at least one field with the run's state and agrees
  with it on every one they share. `{"payment_ref": "pay-1", "paid": true}`
  matches the run whose state has `"payment_ref": "pay-1"`, and every run
  it matches gets it. Its data joins the state, and the waiting step runs
  again.
- A run that starts waiting gets the matching messages that came since it
  started, so a fast reply is not missed.

Messages are kept until their runs end, then `keep` more (a day).

## Getting an answer

A message is sent and forgotten unless its sender asks for what came of
it, one of two ways:

- **Wait for it**, for runs that end soon: `itero emit --wait` returns when
  the runs it started have ended and prints them as JSON, each with its
  status and state; `POST /events?wait=60` answers the same
  ([`itero emit`](/commands/emit/#waiting-for-the-answer)).
- **Be called back**, for runs that may wait for days: `--reply <url>`, or
  `"reply"` in `POST /events`. Each run the message starts is posted to the
  URL when it ends, and tried again until the app takes it
  ([Callbacks](/reference/http/#callbacks)).

Either way the workflow files stay as they are: the answer is how the runs
ended, and their state.

## Where messages come from

- **Push**: an app or a webhook posts to `/events`, or a source holds a
  connection open and prints a line when told something.
- **Pull**: a workflow on a schedule asks
  (`"on": [{ "schedule": "* * * * *" }]`), or a source that polls.
- **Watch**: a `path` in a workflow's `on`, below.

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
- A path that is not there yet is watched for when it comes; its folder
  must be there.
- `~/` is your home folder, `$$NAME` an environment variable (never a
  secret: the path is in the run's data and the log), and a relative path
  is in the workspace.
- Editing a workflow moves its watch within a second.

`examples/photos` makes a thumbnail of each photo dropped into its
`inbox/`.

## Sources

A source is an executable in `sources/` that keeps running. Each line it
prints is the data of one message, of the type its file name says:
`sources/mail.received` sends `mail.received` messages.

```sh
#!/bin/sh
# a message each time the file changes
exec itero watch "$HOME/notes.txt"
```

It runs in the workspace, with the daemon's `itero` first on its `PATH`.
Its stdin closes when the daemon goes; its stderr goes to the daemon's
stderr. The daemon starts it again when it ends, waiting longer each time,
and gives up on one that keeps ending (`itero.source.gave-up`); the log
and `/health` show each source.

`itero watch <path>` prints `{"path", "kind"}` each time a file or folder
changes, from the OS's own file events, calm for half a second: for a
source that wants the same events as a `path` in `on`.

A source that keeps its own "last seen" sends one message per new thing,
so each run has its own: `examples/mail-alert` prints one line per new
mail, with its sender and subject.
