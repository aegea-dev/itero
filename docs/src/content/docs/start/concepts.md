---
title: Workspaces, workflows and runs
description: The few things itero is made of.
---

Three kinds of things, each with its shape, as in the logo and in
pictures of a workflow: **● a workflow**, **■ an action** and **▲ a
connector**, where messages come from.

**A workspace** is a folder with an `itero.config.json`. One daemon runs one
workspace:

```
itero.config.json          the manifest: what it is, port, limits, and each workflow, action and connector
workflows/order.json       ● a workflow: its name is its file name
actions/check-payment      ■ an action: an executable, its name its file name
connectors/mail.received   ▲ a connector: its messages are of this type
.itero/                    the journal, the log and each workflow's memory
```

`workflows/`, `actions/` and `connectors/` may have subfolders, to group files
any way you like: `workflows/shop/order.json`, `actions/mail/imap-check`.
A name stays its file name, unique within its kind: two workflows of one
name are an error naming both files, but an action and a workflow may share
one (`#ship` and `ship`). Hidden folders and `node_modules` are never
looked into. [`itero which`](/commands/which/) says where a name is.

**● A workflow** says when it runs (`on`) and what it does (`steps`):

```json
{ "on": [{ "event": "order.placed" }], "steps": [["#check-payment", "#check-stock"], "order-process", "#send-mail"] }
```

**■ An action** (`#check-payment`) is a small program. It reads its values
as environment variables, prints JSON, and its exit code says how it
went. **A workflow** can be a step too (`order-process`).

**A message** has a type, two or more words joined by dots
(`order.placed`), and data. Apps send them with `itero emit` or `POST
/events`; connectors print them. A message starts a run of every workflow
whose `on` has an `event` of its type. A schedule or a change to a
watched file starts runs too.

**A run** is one go through a workflow, with an id and a name of two
random words (`misty-lake`). It has a **state**: the data of the message
that started it, plus what each step printed. A step that is
not ready yet (exit 75) makes the run **wait** until a message for it
arrives. A run ends as `done`, `failed` (the state says
`"success": false`) or `error` (something broke).

**The daemon** runs the workspace, one per workspace. When something
happens (a message arrives, a schedule comes due, a watched path changes,
a waiting run's timer fires) it makes a pass, at once; in between it waits,
with no CPU:

```
itero daemon  (one per workspace)
│
├─ connectors and watched paths: beside it, feeding messages in
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

Every arrow is written to the journal (`.itero/journal.sqlite`) before it
happens, so a daemon that stops carries on where it was, with nothing
lost and nothing done twice. What it did is in the log,
`.itero/itero.log` ([The log, IDs and names](/guides/log/)).

Names at a glance:

| Written | Is |
|---|---|
| `#check-payment` | an action |
| `order-process` | a workflow |
| `{"event": "order.placed"}` | a message type in `on` |
| `{"schedule": "0 9 * * *"}` | a schedule in `on` |
| `{"path": "inbox"}` | a watched file or folder in `on` |
| `$order` | a value in the run's state |
| `$$API_KEY` | an environment variable or a secret |
