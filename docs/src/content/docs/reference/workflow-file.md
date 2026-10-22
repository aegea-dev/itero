---
title: Workflow files
description: Every key of a workflow file, and every form of a step.
---

`workflows/<name>.json`, or in any subfolder of `workflows/`; the
workflow's name is the file name, unique among the workflows. An unknown key
is an error.

| Key | Is | |
|---|---|---|
| `meta` | about it: `description`, `from`, `until`, `off`, `internal`, and keys of your own | |
| `on` | when it runs: a list of objects (below) | none: it runs only as a step |
| `steps` | what it does, in order | needed |
| `success` | steps run when it went well | |
| `failure` | steps run when the state says `"success": false` | |
| `error` | steps run when something broke | |
| `timeout` | how long a run may wait for data, like `"2h"` | a day |

## `on`

| Entry | Means |
|---|---|
| `{"event": "order.placed"}` | a message of this type starts a run |
| `{"schedule": "0 9 * * *"}` | a schedule starts a run (cron, local time) |
| `{"path": "inbox"}` | a change to this file or folder starts a run; `~/` is home, `$$NAME` an environment variable, a relative path is in the workspace |
| `{"$order": 43}` | condition: `order` is exactly `43` |
| `{"has": "$order"}` | condition: `order` is in the state |
| `{"has": ["$order", "$$API"]}` | condition: all are there; `$$API` is defined (environment or secret) |

`event`, `schedule` and `path` are alternatives, each alone in its
object; conditions must all hold. Any other key is an error naming it.

## `meta`

| Key | Means |
|---|---|
| `description` | text |
| `from`, `until` | a date (`2026-11-01`, a whole day) or an RFC 3339 time: outside them nothing starts it |
| `off` | `true`: nothing starts it |
| `internal` | `true`: it looks after the workspace; its runs are left out of lists unless asked, and outside the limits |
| any other | kept and shown |

`id`, `name`, `created`, `path` and `version` are given by itero; setting
one is an error.

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
