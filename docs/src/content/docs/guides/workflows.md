---
title: Writing a workflow
description: on, meta, steps, side by side, conditions, workflows as steps, and the lists run at the end.
---

<span class="wf">●</span> A workflow says when something should happen, and what to do then,
step by step. It is a JSON file in `workflows/`, and its name is its file
name. Its steps are <span class="act">■</span> actions (small programs) and other workflows.

## Examples

These are from the `examples/` folder, each a workspace you can run.

**Check websites every minute** (`examples/beacon`, `workflows/sites.json`):

```json
{
  "on": [{ "schedule": "* * * * *" }],
  "steps": [
    { "#check-sites": { "sites": "$sites" } },
    "#changed",
    { "#notify": { "on": [{ "has": "$text" }], "title": "beacon: sites" } }
  ]
}
```

- `on`: every minute.
- `#check-sites` fetches each site in `$sites` (the workflow's
  [settings](#settings)) and says which are down.
- `#changed` keeps only what changed since the last run, as `$text`.
- `#notify` shows a notification, but only when there is a `$text`: no
  news, no notification.

**A notification for each new mail** (`examples/mail-alert`):

```json
{ "on": [{ "event": "mail.received" }], "steps": ["#notify"] }
```

- `on`: a `mail.received` event, which a <span class="con">▲</span> connector sends for each new
  mail ([Connectors](/guides/connectors/)).
- `#notify` shows it: the event's data (sender, subject) is the run's
  state, and `#notify` takes its title and text from there.

**A daily digest by mail** (`examples/page-digest`):

```json
{
  "on": [{ "schedule": "0 9 * * *" }],
  "steps": [
    { "#fetch": { "url": "https://example.com/news" } },
    { "#ask-ai": { "prompt": "Summarize this page in 5 bullets.", "key": "$$ANTHROPIC_KEY" } },
    { "#send-mail": { "host": "smtp.example.com", "user": "me@example.com",
                      "password": "$$SMTP_PASS", "to": "me@example.com",
                      "subject": "Daily digest" } }
  ]
}
```

- `on`: every day at 09:00.
- `#fetch` gets the page; what it prints joins the state for the next step.
- `#ask-ai` summarizes it, with the API key kept as a secret
  (`$$ANTHROPIC_KEY`, [Args and secrets](/guides/secrets/)).
- `#send-mail` mails the summary, its password a secret too.

**An order** (`examples/order`, `workflows/order.json`):

```json
{
  "on": [{ "event": "order.placed" }],
  "steps": [
    ["#check-payment", "#check-stock"],
    "order-process",
    "#send-mail"
  ],
  "success": ["#log-result"],
  "failure": ["#log-result"],
  "error": ["#log-result", "#alert-team"]
}
```

- `on`: an `order.placed` event, sent by the shop (`itero emit` or
  `POST /events`).
- `["#check-payment", "#check-stock"]` run side by side; the next step
  waits for both.
- `order-process` is another workflow, used as a step: it ships, reminds
  or cancels, depending on payment and stock ([below](#what-it-does-steps)).
- `#send-mail` tells the customer.
- At the end, `success`, `failure` or `error` runs, by how it went
  ([How it ended](#how-it-ended-success-failure-error)).

## When it runs: `on`

A list of objects, each saying one thing:

| Entry | Means |
|---|---|
| `{ "event": "order.placed" }` | an event of this type starts a run |
| `{ "schedule": "0 9 * * *" }` | a schedule starts a run (cron, the machine's local time) |
| `{ "path": "~/Pictures/inbox" }` | a change to this file or folder starts a run ([Watching files](/guides/connectors/#watching-files)) |
| `{ "$paid": true }` | condition: the state's `paid` is exactly `true` |
| `{ "has": "$order" }` | condition: `order` is in the state |
| `{ "has": ["$order", "$$API"] }` | condition: both are there; `$$API` is an environment variable or a secret |

`event`, `schedule` and `path` are alternatives: any of them starts a
run. The others are conditions that must all hold:

```json
"on": [{ "event": "order.received" }, { "has": "$$API" }]
```

starts a run on `order.received` only if `API` is defined. Values compare
exactly: `0` is not `false`. An `event`, `schedule` or `path` stands alone
in its object.

A workflow with no `event`, `schedule` or `path` never starts by itself:
it runs only as a step of another.

## About it: its entry

What a workflow is, apart from its work, is in the workspace's
`itero.config.json`, under its name, not in its file:

```json
{
  "workflow": {
    "order": {
      "description": "Orders from the shop",
      "from": "2026-11-01", "until": "2026-12-31",
      "owner": "ayşe", "tags": ["orders", "payments"]
    }
  }
}
```

| Key | Means |
|---|---|
| `description` | text for people |
| `from`, `until` | a date or an RFC 3339 time: outside them nothing starts it; runs under way finish. A date is a whole day. |
| `off` | `true`: nothing starts it, without deleting it |
| `internal` | `true`: it looks after the workspace ([The daemon's own workflows](/guides/maintenance/)) |
| `settings` | values its runs start with ([Settings](#settings)) |
| any other | yours: itero keeps it, `itero check` shows it, and nothing more |

`id`, `name`, `created`, `path` and `version` are itero's and cannot be
set ([The log, IDs and names](/guides/log/)). A `meta` in a workflow file
is an error saying where it goes.

## What it does: `steps`

Steps run in order. A step is:

- **an action**, `"#check-payment"`: an executable in `actions/`. `#name`
  is its latest version, `#name@0.2` a pinned one.
- **a workflow**, `"order-process"`: its steps run there, with the same
  state.
- **a list**, `["#check-payment", "stock-check"]`: those run side by side,
  actions or workflows, and the next step waits for all of them.
- **an object**: a name with data for it, and when it runs:

  ```json
  { "#ship": { "carrier": "PTT Kargo", "on": [{ "$paid": true, "$stock": true }] } }
  ```

  The step runs only if its `on` holds; otherwise it is skipped. `undo`
  names what takes it back ([Undo](#undo-taking-steps-back)). The other
  keys are data for the action. A value that is exactly `"$name"` takes the
  state's value, `"$$NAME"` the secret's: `{"#ask-ai": {"key":
  "$$ANTHROPIC_KEY"}}`.

So branches are steps with conditions. `order-process` picks one of four:

```json
{
  "on": [{ "has": ["$paid", "$stock"] }],
  "steps": [
    { "#ship":           { "on": [{ "$paid": true,  "$stock": true }] } },
    { "#remind-payment": { "on": [{ "$paid": false, "$stock": true }] } },
    { "#remind-stock":   { "on": [{ "$paid": true,  "$stock": false }] } },
    { "#cancel":         { "on": [{ "$paid": false, "$stock": false }] } }
  ]
}
```

## How it ended: `success`, `failure`, `error`

Steps run at the end, by the outcome:

- **error**: something broke: an action exited with a code other than 0
  or 75, a step was lost three times, or a run waited too long. The steps
  stop at once and `error` runs.
- **failure**: the steps all ran, and the state says `"success": false`, a
  business outcome such as out of stock. `failure` runs.
- **success**: anything else.

These steps get `$why` and `$failed_step` (and `$undo_failed` when an
undo broke). Before `error`, the steps that finished are undone ([Undo](#undo-taking-steps-back)). The run ends as `done`,
`failed` or `error`; if one of these steps breaks, it ends as `error` and
no other list runs. A run ending in error or failed also sends
`itero.run.error` or `itero.run.failed`, for workflows that watch over
others ([The daemon's own workflows](/guides/maintenance/)). A workflow used as a step has no end lists of its own:
the run's own workflow has them.

## Posting a result

An app that sends an event can get the result back when the run ends, even
days later: it sends its URL in the event's data, and the workflow's end
steps post the result there. From `examples/notify-api`:

```json
{ "on": [{ "event": "app.notify" }], "steps": ["#notify"], "timeout": "1h",
  "success": ["#post-answer"], "failure": ["#post-answer"], "error": ["#post-answer"] }
```

`actions/post-answer` posts the run's state, which an action gets on its
stdin:

```sh
#!/bin/sh
curl -fsS --retry 5 -H 'content-type: application/json' --data-binary @- "$reply_url"
```

The app sends `{"type": "app.notify", "data": {..., "reply_url":
"http://localhost:3000/notified"}}` and gets the state back when the
notification is answered, or `$why` when the run broke.

## Settings

A workflow you share, or take from someone, is used as it is. What
changes from one person to another goes apart from it: its settings, in
its entry in `itero.config.json`, set with
[`itero wf config set`](/commands/workflow/#config).

```json
{ "on": [{ "schedule": "* * * * *" }],
  "steps": [{ "#check-sites": { "sites": "$sites" } }, "#changed", "#notify"] }
```

```sh
itero wf config set sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
```

writes:

```json
{ "workflow": { "sites": { "settings": { "sites": ["https://example.com", "http://localhost:3000"] } } } }
```

- A run's state starts with the settings, then the event's data on top:
  `itero emit` can override a setting for one run. A step names one as
  `"$sites"`, an action gets it as `$sites`, and `on` conditions see it.
- Only the run's own workflow's settings: a workflow used as a step runs
  with the run's state.
- They are read when a run starts: editing them changes the next runs.

Args are an action's settings; settings are a workflow's.

## Undo: taking steps back

A step that makes something (a cluster, an order, a booking) can name
what takes it back, with `undo` in its data:

```json
{
  "on": [{ "event": "cluster.requested" }],
  "steps": [
    { "#cluster-create": { "undo": "#cluster-delete" } },
    "#tf-plan",
    "#check-plan",
    "#tf-apply",
    "#web-check"
  ],
  "error": [
    { "#notify": { "title": "$name", "text": "$why" } },
    { "#notify": { "on": [{ "has": "$undo_failed" }], "title": "$name is left", "text": "$undo_failed" } }
  ]
}
```

When a step breaks, the undos of the steps that finished run, newest
first, then the `error` list. If `#check-plan` refuses the plan, or
`#web-check` gets no answer: `#cluster-delete`, which takes the app with
it, then the notification. If `#cluster-create` breaks: nothing to undo.
This is `examples/terraform`'s `cluster` workflow.

- An undo is an action or a workflow. It gets the state, with `$why` and
  `$failed_step`.
- Only steps that finished are undone: a step that broke half way cleans
  up after itself, or the `error` list does.
- A workflow used as a step can have an undo, and its own steps can too:
  each finished step counts.
- An undo that breaks is noted in the log (`undo.failed`) and in
  `$undo_failed`, a list of `"#step: why"` lines, so the `error` list can
  say what may be left; the others still run, and the run ends as
  `error` with the first `$why`.
- A `failure` (`"success": false`) undoes nothing: the steps all worked.
- `itero wf runs <run>` marks them: `#cluster-delete (undo)  done`.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for an event that
matches it ([What an event does](/guides/connectors/#what-an-event-does)). `timeout` is how
long a run may wait, like `"2h"`; unless given, a day.

## Two fields printed side by side

Steps side by side start from the same state. A field only one of them
prints joins the state as it is; one that both print is kept per step:

```json
"status": { "#check-payment": "ok", "stock-check": "low" }
```

All keys: [Workflow files](/reference/workflow-file/).
