---
title: Sources
description: Long-lived programs that tell itero when something happens outside, so workflows run at once instead of on a schedule.
---

A schedule asks again and again whether something happened. A **source**
is told: it watches a file, holds a connection to a server, or listens to
a device, and sends an event the moment something happens. A workflow with
a trigger `on` that event runs at once.

Software learns that something happened in three ways, and a source can
be any of them:

| Way | Means | Examples |
|---|---|---|
| **push** | the thing tells us | IMAP IDLE, webhooks, message queues |
| **watch** | we see a change as it happens | a file or folder changing |
| **poll** | we ask again and again | a server that offers nothing better |

## A source's file

Each source is a file in the workspace's `sources/` folder (or the folders
`sources` lists in [`itero.toml`](/reference/itero-toml/)). Its name is the
file's name.

```json title="sources/mailbox.json"
{ "source": "watch@1", "with": { "path": "/var/mail/${env.USER}" }, "emit": "mail.changed" }
```

| Key | Is |
|---|---|
| `source` | a built-in source, like `watch@1` |
| `program` | or a program and its arguments, run in the sources folder; a path with `/` is in that folder |
| `image` | for a program: a container image it runs in, the sources folder mounted read-only at `/source` |
| `with` | its settings; `${env.NAME}` is a [secret](/guides/secrets/) |
| `emit` | the type of the events it sends, like `mail.changed` |

One of `source` and `program`. `itero check` lists the sources it found.
`itero daemon` starts them; `itero daemon --once` does not.

## watch@1

Watches a file or a folder with the operating system's own events (kqueue
on macOS, inotify on Linux), and sends `emit` when it changes.

| `with` | Is |
|---|---|
| `path` | the file or folder; a file that is not there yet is watched for |
| `quiet` | how long changes must stop before one event goes (`500ms` unless given) |
| `max_wait` | the longest a steady stream of changes waits (`5s` unless given) |

Many changes close together are one event. Its data is `{path, kind}`,
`kind` one of `created`, `changed`, `removed`, `renamed` and `rescan`.
`rescan` means the watch may have missed something (the file was replaced,
or the kernel's queue overflowed): read the state again rather than trust
the events. A workflow that reads what is there, as the mail check does,
needs nothing more.

## Writing a source

A source is any program, in any language, that keeps to this:

1. It reads one line of JSON on stdin when it starts: its `name`, its
   `with` (secrets filled in), its `emit` and its last `cursor`.
2. It prints one JSON object per line on stdout:
   - an event: `{"type": "mail.received", "data": {...}}`; `type` may be
     left out when its file sets `emit`;
   - or a cursor: `{"cursor": ...}`, any JSON, given back as `cursor` the
     next time it starts, so it carries on where it stopped (an IMAP UID, a
     file offset).
3. It exits when stdin closes: the daemon has gone.
4. What it prints on stderr goes to the daemon's log.

A line that is not an event or a cursor is logged and skipped. Event types
are lowercase words joined by dots; the engine's own (`action.`, `timer.`,
`run.`, `workflow.`, `source.`) are refused.

## Kept running

The daemon supervises each source. A source that ends is started again
after a wait that doubles from 1 second up to a minute (and starts at 1
second again after a run of 5 minutes). One that is started again 10
times within 10 minutes is given up on, and the log says so. `GET /health`
shows each source: `running`, `waiting` or `gave up`, its restarts and how
it ended last.

## Example: IMAP IDLE

[`examples/mail-servers`](https://github.com/aegea-dev/itero/tree/main/examples/mail-servers)
has a push source: `idle.py`, a small IMAP client that waits in IDLE and
prints a line when the mailbox grows. It runs in a container, so the
machine needs Docker, not Python:

```json title="sources/imap-idle.json"
{ "program": ["python3", "idle.py"], "image": "python:3.13-alpine",
  "with": { "host": "imap.example.com", "port": 993, "tls": true,
            "user": "me@example.com", "password": "${env.IMAP_PASSWORD}",
            "mailbox": "INBOX" },
  "emit": "mail.changed" }
```

The `imap-check` workflow runs on `mail.changed`, and every 5 minutes as a
safety net. A source is often best paired with a slow schedule like this:
it catches anything the source could not say.
