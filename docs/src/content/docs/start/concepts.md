---
title: Workspaces, workflows and runs
description: The few things itero is made of.
---

Three kinds of things, each with its shape, as in the logo and in
pictures of a workflow: **a workflow <span class="wf">●</span>**, **an action <span class="act">■</span>** and **a
connector <span class="con">▲</span>**, where events (messages) come from.

**A workspace** is a folder with an `itero.config.json`. One daemon runs one
workspace:

```
itero.config.json          the manifest: what it is, port, limits, and each workflow, action and connector
workflows/order.json       a workflow ●: its name is its file name
actions/check-payment      an action ■: an executable, its name its file name
connectors/mail.received   a connector ▲: its events are of this type
.itero/                    the journal, the log and each workflow's memory
```

`workflows/`, `actions/` and `connectors/` may have subfolders, to group files
any way you like: `workflows/shop/order.json`, `actions/mail/imap-check`.
A name stays its file name, unique within its kind: two workflows of one
name are an error naming both files, but an action and a workflow may share
one (`#ship` and `ship`). Hidden folders and `node_modules` are never
looked into. [`itero which`](/commands/which/) says where a name is.

**A workflow** says when it runs (`on`) and what it does (`steps`):

```json
{ "on": [{ "event": "order.placed" }],
  "steps": [{ "#check-payment": { "args": { "order": "$event.order" } } },
            { "#ship": { "on": [{ "$output.paid": true }], "args": { "order": "$event.order" } } }] }
```

**An action** (`#check-payment`) is a small program. It gets its step's
args as environment variables, prints its result, and its exit code says
how it went. The next step has that result as `$output`; any later step
reads it as `$steps.check-payment`. **A workflow** can be a step too.

**An event** (a message) has a type, two or more words joined by dots
(`order.placed`), and data. Apps send them with `itero emit` or `POST
/events`; connectors print them. An event starts a run of every workflow
whose `on` has an `event` of its type. A schedule or a change to a
watched file starts runs too.

**A run** is one go through a workflow, with an id and a name of two
random words (`misty-lake`). Its steps read the event that started it
(`$event`), the step just before (`$output`) and any earlier step by its
name (`$steps.<name>`); each action gets only the args its step gives it.
A step that is not ready yet (exit 75) makes the run **wait** until an
event for it arrives. A run ends as `done`, `failed` (the last step says
`"success": false`) or `error` (something broke).

**The daemon** runs the workspace, one per workspace. When something
happens (an event arrives, a schedule comes due, a watched path changes,
a waiting run's timer fires) it makes a pass, at once; in between it waits,
with no CPU:

```
itero daemon  (one per workspace)
│
├─ connectors and watched paths: beside it, feeding events in
│
└─ every pass:
     1. events     → start new runs, wake waiting runs
     2. runs       → decide each run's next steps
     3. steps      → run each step's action, side by side (up to 8 at a time),
                     each as its own process
     4. results    → saved as each step ends, and decided on at once,
                     while other steps still run and new events come in
```

A run moves through its workflow's steps:

```
run:  started ─► step ─► step ─► … ─► success / failure / error list ─► ended
                   │
                   └─ exit 75: waiting ─► (a matching event, or the hourly retry) ─► the step again
```

Every arrow is written to the journal (`.itero/journal.sqlite`) before it
happens, so a daemon that stops carries on where it was, with nothing
lost and nothing done twice. What it did is in the log,
`.itero/itero.log` ([The log, IDs and names](/guides/log/)).

Names at a glance:

| Written | Is |
|---|---|
| `#check-payment` | an action |
| `order-process` | a workflow |
| `{"event": "order.placed"}` | an event type in `on` |
| `{"schedule": "0 9 * * *"}` | a schedule in `on` |
| `{"path": "inbox"}` | a watched file or folder in `on` |
| `$event.order` | a field of the event that started the run |
| `$output` | the step before's result |
| `$steps.check-payment` | an earlier step's result, by its name |
| `$config.description` | a value of `itero.config.json` |
| `$$API_KEY` | a secret, a value in `.env`, or an environment variable |
