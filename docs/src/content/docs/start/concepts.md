---
title: Workspaces, workflows and runs
description: The few things itero is made of.
---

**A workspace** is a folder with an `itero.toml`. One daemon runs one
workspace:

```
itero.toml                 settings (the port)
workflows/order.json       a workflow: its name is its file name
actions/check-payment      an action: an executable, its name its file name
sources/mail.received      a source: its messages are of this type
args/check-payment.json    data for an action (itero args)
.itero/                    the journal and each workflow's memory
```

**A workflow** says when it runs (`on`) and what it does (`steps`):

```json
{ "on": ["order.placed"], "steps": [["#check-payment", "#check-stock"], "order-process", "#send-mail"] }
```

**An action** (`#check-payment`) is a small program. It reads its values
as environment variables, prints JSON, and its exit code says how it
went. **A workflow** can be a step too (`order-process`).

**A message** has a type, two or more words joined by dots
(`order.placed`), and data. Apps send them with `itero emit` or `POST
/events`; sources print them. A message starts a run of every workflow
whose `on` lists its type.

**A run** is one go through a workflow. It has a **state**: the data of
the message that started it, plus what each step printed. A step that is
not ready yet (exit 75) makes the run **wait** until a message for it
arrives. A run ends as `done`, `failed` (the state says
`"success": false`) or `error` (something broke).

**The daemon** takes the messages, asks the **manager** what each run
does next, and runs steps in **workers**. Everything is written to the
journal first, so a daemon that stops carries on where it was.

Names at a glance:

| Written | Is |
|---|---|
| `#check-payment` | an action |
| `order-process` | a workflow |
| `order.placed` | a message type |
| `0 9 * * *` | a schedule (it has spaces) |
| `$order` | a value in the run's state |
| `$$API_KEY` | an environment variable or a secret |
