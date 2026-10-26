---
title: Workflow files
description: Every key of a workflow file, and every form of a step.
---

`workflows/<name>.json`, or in any subfolder of `workflows/`; the
workflow's name is the file name, unique among the workflows. An unknown key
is an error.

| Key | Is | |
|---|---|---|
| `on` | when it runs: a list of objects (below) | none: it runs only as a step |
| `steps` | what it does, in order | needed |
| `success` | steps run when it went well | |
| `failure` | steps run when the state, or the last step's result, says `"success": false` | |
| `error` | steps run when something broke | |
| `timeout` | how long a run may wait for data, like `"2h"` | a day |

## `on`

| Entry | Means |
|---|---|
| `{"event": "order.placed"}` | an event of this type starts a run |
| `{"schedule": "0 9 * * *"}` | a schedule starts a run (cron, local time) |
| `{"path": "inbox"}` | a change to this file or folder starts a run; `~/` is home, `$$NAME` an environment variable, a relative path is in the workspace |
| `{"$order": 43}` | condition: `order` is exactly `43` |
| `{"$output.paid": true}` | condition: one field of a value, here the step before's result |
| `{"has": "$order"}` | condition: `order` is in the state |
| `{"has": ["$order", "$$API"]}` | condition: all are there; `$$API` is defined (environment or secret) |

`event`, `schedule` and `path` are alternatives, each alone in its
object; conditions must all hold. Any other key is an error naming it.

What a workflow is (its description, `off`, `from`, `until`, `internal`,
its settings) is in its entry in
[`itero.config.json`](/reference/itero-config/), not in its file.

## A step

| Form | Is |
|---|---|
| `"#name"` | an action, its latest version |
| `"#name@0.2"` | an action at a version |
| `"name"` | a workflow, run here with the same state; its result is its last step's |
| `["#a", "b"]` | side by side; the next step waits for all, and their results merge into one |
| `{"#name": {...}}` | with keys of its own (below) |

| A step's key | Is |
|---|---|
| `args` | its values: an object, or a `"$name"` that is one (`"$output"`); they join the run's state when it starts |
| `on` | when it runs; otherwise it is skipped |
| `undo` | what takes it back when a later step breaks; given this step's result as `$output` |
| `strict` | `true`: its args reach its action only, not the state (an action's step) |

In `args`, `"$name"` is the state's value with its type, `"$name.field"`
one field of it, `"$$NAME"` a secret's. Inside text, and in lists and
objects, each `$name` is filled (`""` when it is not there); `\\$` is a
dollar sign; a secret inside text is refused. `$output` is the step
before's result; `output` is no arg's name. See [Workflows](/guides/workflows/).
