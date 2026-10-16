---
title: Quickstart
description: A workspace that checks the local mailbox every 5 minutes and shows a notification for new mail.
---

This page builds the mail check: a workflow that reads the local mailbox
every 5 minutes and shows a desktop notification for mail it has not seen
before. The same files are in the repository as
[`examples/mail-check`](https://github.com/aegea-dev/itero/tree/main/examples/mail-check).

## 1. A workspace

A workspace is a folder with an `itero.toml`:

```bash
mkdir mail-check && cd mail-check
```

```toml title="itero.toml"
port = 8787
```

`port` is where the daemon's API listens, on 127.0.0.1. Workflows live in
`workflows/` and actions in `actions/` unless `itero.toml` lists other
folders.

## 2. An action

An action of your own is a folder in `actions/` with an `action.json` and
its program. This one reads a mailbox and prints what is new as JSON.

```json title="actions/mail/action.json"
{
  "name": "mail",
  "version": 1,
  "type": "executable",
  "description": "Reads a mailbox (mbox): what is new since the last message seen.",
  "limits": { "timeout": "30s" },
  "targets": { "check": { "executable": "./check.sh" } }
}
```

The program gets the job as one line of JSON on stdin; its `with` holds the
arguments the workflow gave. It prints
`{"new": n, "seen": "<id of the last message>", "summary": "..."}`. Copy
[`check.sh`](https://github.com/aegea-dev/itero/blob/main/examples/mail-check/actions/mail/check.sh)
from the example into `actions/mail/` and make it executable:

```bash
chmod +x actions/mail/check.sh
```

The workflow names this action `mail.check@1`: the folder's name, the
target, and the version.

## 3. A workflow

```json title="workflows/mail-check.json"
{
  "name": "mail-check",
  "description": "Notifies about mail not seen before.",
  "triggers": [ { "schedule": "*/5 * * * *", "args": { "mailbox": "" } } ],
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

Every 5 minutes a run starts in `start` and moves to `done`, running three
actions in order:

1. `mail.check@1` reads the mailbox. Its `id` makes its output
   `steps.check` for the rest of the chain.
2. `notify@1` shows a notification, only `when` there is new mail.
3. `memory.set@1` remembers the last message seen, for the next run.

An empty `mailbox` means the script's default, `/var/mail/<you>`.

## 4. Check and run it

```bash
itero check
```

prints every workflow and action it found, with its version, or the first
problem in a file, naming the file. Then start the daemon:

```bash
itero daemon
```

In another terminal, send yourself a mail:

```bash
echo "hello" | mail -s "test" "$USER"
```

Within 5 minutes one notification says `1 new: test`; later runs show none
until more mail comes. On macOS the first notification asks to be allowed.
Ctrl-C stops the daemon.

## 5. Look inside

```bash
itero runs                 # the runs, newest first
itero memory mail-check    # what the workflow remembers
curl -s http://127.0.0.1:8787/health
```

To keep it running after you close the terminal, and from login on, see
[Start at login](/guides/service/).
