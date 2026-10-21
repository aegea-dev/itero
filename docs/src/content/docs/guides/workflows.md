---
title: Writing a workflow
description: on, steps, side by side, conditions, workflows as steps, and the lists run at the end.
---

A workflow is a JSON file in `workflows/`; its name is its file name.

```json
{
  "on": ["order.placed"],
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

A list of:

- **message types**: `order.placed` starts a run when such a message comes;
- **schedules**, in cron form and the machine's local time: `0 9 * * *`;
- **`$name`**: the value is in the run's state;
- **`$$NAME`**: that environment variable or secret is defined.

Message types and schedules are alternatives: any of them starts a run.
`$` and `$$` entries are conditions that must all hold:
`["order.received", "$$API"]` starts a run on `order.received` only if
`API` is defined.

`on` can also be an object of exact values: `{"$order": 43}` holds when the
state has `order` equal to `43`. Values compare exactly: `0` is not
`false`.

A workflow with no message type or schedule in its `on` never starts by
itself: it runs only as a step of another.

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
  { "#ship": { "carrier": "PTT Kargo", "on": { "$paid": true, "$stock": true } } }
  ```

  The step runs only if its `on` holds; otherwise it is skipped. The other
  keys are data for the action. A value that is exactly `"$name"` takes the
  state's value, `"$$NAME"` the secret's: `{"#ask-ai": {"key":
  "$$ANTHROPIC_KEY"}}`.

So branches are steps with conditions. `order-process` picks one of four:

```json
{
  "on": ["$paid", "$stock"],
  "steps": [
    { "#ship":           { "on": { "$paid": true,  "$stock": true } } },
    { "#remind-payment": { "on": { "$paid": false, "$stock": true } } },
    { "#remind-stock":   { "on": { "$paid": true,  "$stock": false } } },
    { "#cancel":         { "on": { "$paid": false, "$stock": false } } }
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

These steps get `$why` and `$failed_step`. The run ends as `done`,
`failed` or `error`; if one of these steps breaks, it ends as `error` and
no other list runs. A workflow used as a step has no end lists of its own:
the run's own workflow has them.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for a message that
matches it ([Messages and sources](/guides/messages/)). `timeout` is how
long a run may wait, like `"2h"`; unless given, a day.

## Two fields printed side by side

Steps side by side start from the same state. A field only one of them
prints joins the state as it is; one that both print is kept per step:

```json
"status": { "#check-payment": "ok", "stock-check": "low" }
```

All keys: [Workflow files](/reference/workflow-file/).
