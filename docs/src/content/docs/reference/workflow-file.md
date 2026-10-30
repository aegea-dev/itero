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
| `failure` | steps run when the last step's result says `"success": false` | |
| `error` | steps run when something broke | |
| `timeout` | how long a run may wait for data, like `"2h"` | a day |

## `on`

| Entry | Means |
|---|---|
| `{"event": "order.placed"}` | an event of this type starts a run |
| `{"schedule": "0 9 * * *"}` | a schedule starts a run (cron, local time) |
| `{"path": "inbox"}` | a change to this file or folder starts a run; `~/` is home, `$$NAME` a value from `.env` or the environment, a relative path is in the workspace |
| `{"$event.order": 43}` | condition: the event's `order` is exactly `43` |
| `{"$output.paid": true}` | condition: a field of the step before's result (in a step's `on`) |
| `{"has": "$event.order"}` | condition: the event has an `order` |
| `{"has": ["$event.order", "$$API"]}` | condition: all are there; `$$API` is defined (secret, `.env` or environment) |

`event`, `schedule` and `path` are alternatives, each alone in its
object; conditions must all hold. Any other key is an error naming it.

What a workflow is (its description, `off`, `from`, `until`, `internal`)
is in its entry in
[`itero.config.json`](/reference/itero-config/), not in its file.

## A step

| Form | Is |
|---|---|
| `"#name"` | an action, its latest version |
| `"#name@0.2"` | an action at a version |
| `"name"` | a workflow, run here; its `$event` is the args its step gives, its result its last step's |
| `["#a", "b"]` | side by side; the next step waits for all, and their results merge into one; a step in it may be an object (below) |
| `{"race": ["#a", "b"]}` | side by side, waiting as its key says (below) |
| `{"#name": {...}}` | with keys of its own (below) |

| A step's key | Is |
|---|---|
| `args` | what its action gets, and nothing else: an object, a list of objects read in order (an item with exactly `title` and `text` gives the arg `title` names its text, as text), or a value that is one (`"$output"`) |
| `on` | when it runs, conditions only; otherwise it is skipped |
| `undo` | what takes it back when a later step breaks: `"#name"`, or `{"#name": {"args": {...}}}`; given this step's result as `$output` |
| `as` | its name for `$steps`, `"eu"`, read as `$steps.eu`; in a group, `#eu` for an action's step |

| A value | Is |
|---|---|
| `$event`, `$event.order` | the event that started the run |
| `$output`, `$output.total` | the step before's result; in the `error` list's first step, `why`, `failed_step`, `undo_failed` |
| `$steps.fetch-rates`, `$steps.fetch-rates.price` | an earlier step's result, by its name or `as` |
| `$config`, `$config.description` | `itero.config.json`, read-only |
| `$$NAME` | a secret, a value in `.env`, or an environment variable; a whole value only |

Fields go to any depth. A whole value keeps its type; inside text, and in
lists and objects, each is filled (`""` when it is not there); `\\$` is a
dollar sign. Any other `$name` is an error of `itero check`, which says
what to write. `output` and `event` are no arg's names; `strict` is gone
(args reach their action only). See [Workflows](/guides/workflows/).

## Steps side by side

| A group's key | Is |
|---|---|
| `all` | its steps; it ends when every one ended well (a plain list) |
| `race` | its steps; it ends with the first to end, `$output.winner` its name (no `#`) |
| `any` | its steps; it ends with the first to end well, `$output.winner` its name (no `#`) |
| `some` | its steps; it ends when `count` of them, or those in `names`, ended well |
| `settled` | its steps; it ends when every one ended, and never breaks |
| `count` | with `some`: how many, 1 to the number of steps |
| `names` | with `some`: which, by name |
| `fail` | when it breaks instead: `"any"`, `"all"`, `"never"`, a step's name, or a list of names |

An object has one of the five keys, with two or more steps, each a name
or an object with `args`, `on` and `as` (not `undo`), each known
by one name in the group (its `as`, else its name), and its result kept
as `$steps.<name>`. `$output.failed` and `$output.why` name the steps that broke and why,
whenever one did (always with `settled` and `"fail": "never"`). The steps
that lose are stopped, never undone. A group of `all` or `settled` that
breaks waits for its running steps to end first. See
[How steps side by side wait](/guides/workflows/#how-steps-side-by-side-wait).
