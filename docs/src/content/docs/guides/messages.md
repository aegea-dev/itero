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
stopped waits for it. Types starting with `action.`, `timer.`, `run.`,
`workflow.` or `source.` are the engine's own and refused.

## What a message does

- **It starts a run** of every workflow whose `on` lists its type, and
  whose conditions hold over its data. Its data is the run's first state.
  Every such workflow starts, side by side.
- **It reaches the runs waiting for data** (a step exited 75) that it
  matches: it shares at least one field with the run's state and agrees
  with it on every one they share. `{"payment_ref": "pay-1", "paid": true}`
  matches the run whose state has `"payment_ref": "pay-1"`, and every run
  it matches gets it. Its data joins the state, and the waiting step runs
  again.
- A run that starts waiting gets the matching messages that came since it
  started, so a fast reply is not missed.

Messages are kept until their runs end, then a day more.

## Where messages come from

- **Push**: an app or a webhook posts to `/events`, or a source holds a
  connection open and prints a line when told something.
- **Pull**: a workflow on a schedule asks (`"on": ["* * * * *"]`), or a
  source that polls.
- **Watch**: a source runs `itero watch <path>`.

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
log. The daemon starts it again when it ends, waiting longer each time,
and gives up on one that keeps ending; `/health` shows each source.

`itero watch <path>` prints `{"path", "kind"}` each time a file or folder
changes, from the OS's own file events, calm for half a second.

A source that keeps its own "last seen" sends one message per new thing,
so each run has its own: `examples/mail-alert` prints one line per new
mail, with its sender and subject.
