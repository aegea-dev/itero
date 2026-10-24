---
title: Writing a workflow
description: on, meta, steps, side by side, conditions, workflows as steps, and the lists run at the end.
---

● A workflow is a JSON file in `workflows/`; its name is its file name.

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

## When it runs: `on`

A list of objects, each saying one thing:

| Entry | Means |
|---|---|
| `{ "event": "order.placed" }` | a message of this type starts a run |
| `{ "schedule": "0 9 * * *" }` | a schedule starts a run (cron, the machine's local time) |
| `{ "path": "~/Pictures/inbox" }` | a change to this file or folder starts a run ([Watching files](/guides/messages/#watching-files)) |
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

These steps get `$why` and `$failed_step`. Before `error`, the steps
that finished are undone ([Undo](#undo-taking-steps-back)). The run ends as `done`,
`failed` or `error`; if one of these steps breaks, it ends as `error` and
no other list runs. A run ending in error or failed also sends
`itero.run.error` or `itero.run.failed`, for workflows that watch over
others ([The daemon's own workflows](/guides/maintenance/)). A workflow used as a step has no end lists of its own:
the run's own workflow has them.

## Settings

A workflow you share, or take from someone, is used as it is. What
changes from one person to another goes apart from it: its settings, in
its entry in `itero.config.json`, set with
[`itero config`](/commands/config/).

```json
{ "on": [{ "schedule": "* * * * *" }],
  "steps": [{ "#check-sites": { "sites": "$sites" } }, "#changed", "#notify"] }
```

```sh
itero config sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
```

writes:

```json
{ "workflow": { "sites": { "settings": { "sites": ["https://example.com", "http://localhost:3000"] } } } }
```

- A run's state starts with the settings, then the message's data on top:
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
    { "#tf-apply": { "undo": "#tf-destroy" } },
    "#web-check"
  ]
}
```

When a step breaks, the undos of the steps that finished run, newest
first, then the `error` list. If `#web-check` breaks: `#tf-destroy`,
then `#cluster-delete`. If `#tf-plan` breaks: only `#cluster-delete`.

- An undo is an action or a workflow. It gets the state, with `$why` and
  `$failed_step`.
- Only steps that finished are undone: a step that broke half way cleans
  up after itself, or the `error` list does.
- A workflow used as a step can have an undo, and its own steps can too:
  each finished step counts.
- An undo that breaks is noted in the log (`undo.failed`); the others
  still run, and the run ends as `error` with the first `$why`.
- A `failure` (`"success": false`) undoes nothing: the steps all worked.
- `itero runs <run>` marks them: `#cluster-delete (undo)  done`.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for a message that
matches it ([Messages and connectors](/guides/messages/)). `timeout` is how
long a run may wait, like `"2h"`; unless given, a day.

## Two fields printed side by side

Steps side by side start from the same state. A field only one of them
prints joins the state as it is; one that both print is kept per step:

```json
"status": { "#check-payment": "ok", "stock-check": "low" }
```

All keys: [Workflow files](/reference/workflow-file/).
