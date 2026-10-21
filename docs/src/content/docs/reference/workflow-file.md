---
title: Workflow files
description: Every key of a workflow file, and every form of a step.
---

`workflows/<name>.json`; the workflow's name is the file name. An unknown
key is an error.

| Key | Is | |
|---|---|---|
| `on` | when it runs: a list of message types, schedules, `$name` and `$$NAME`; or an object of exact `$name` values | none: it runs only as a step |
| `steps` | what it does, in order | needed |
| `success` | steps run when it went well | |
| `failure` | steps run when the state says `"success": false` | |
| `error` | steps run when something broke | |
| `timeout` | how long a run may wait for data, like `"2h"` | a day |

## `on`

| Entry | Means |
|---|---|
| `order.placed` | a message of this type starts a run |
| `0 9 * * *` | a schedule starts a run (cron, local time) |
| `$order` | condition: `order` is in the state |
| `$$API` | condition: `API` is defined (environment or secret) |
| `{"$order": 43}` | condition: `order` is exactly `43` |

Message types and schedules are alternatives; conditions must all hold.

## A step

| Form | Is |
|---|---|
| `"#name"` | an action, its latest version |
| `"#name@0.2"` | an action at a version |
| `"name"` | a workflow, run here with the same state |
| `["#a", "b"]` | side by side; the next step waits for all |
| `{"#name": {...}}` | with data for it; `on` in it is when it runs |

In a step's data, `"$name"` is the state's value, `"$$NAME"` a secret's.
See [Writing a workflow](/guides/workflows/).
