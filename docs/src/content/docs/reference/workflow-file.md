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
| `repeat` | its runs go on in rounds, each with the one before's results ([Repeat](/guides/workflows/#repeat-rounds)) | |

## `on`

| Entry | Means |
|---|---|
| `{"event": "order.placed"}` | an event of this type starts a run |
| `{"event": "mail.received", "args": {...}}` | the same, and the args its connector is given, as environment variables: values or `$$NAME` ([A connector's args](/guides/secrets/#a-connectors-args)) |
| `{"schedule": "0 9 * * *"}` | a schedule starts a run (cron, local time) |
| `{"path": "inbox"}` | a change to this file or folder starts a run; `~/` is home, `$$NAME` a value from `.env` or the environment, a relative path is in the workspace |
| `{"$event.order": 43}` | condition: the event's `order` is exactly `43` |
| `{"$output.paid": true}` | condition: a field of the step before's result (in a step's `on`) |
| `{"has": "$event.order"}` | condition: the event has an `order` |
| `{"has": ["$event.order", "$$API"]}` | condition: all are there; `$$API` is defined (secret, `.env` or environment) |
| `{"unique": "$event.id", "for": "1d"}` | window: a run starts for a value no run started with within `for` (for good without it) |
| `{"changed": "$event.state"}` | window: a run starts when the value is not the last run's |
| `{"at_most": "1h"}` | window: at most one run per period; the first event in it starts it |
| `{"batch": {"count": 100, "within": "5m"}}` | window: one run of the events it kept, at `count` or `within` after the first (`$event.events`, `count`, `first`, `last`, and `per`) |
| `{"at_least": 5, "within": "10m"}` | window: one run of the events it kept, once `at_least` came within `within` |
| `"per": "$event.site"` | beside a window: one window for each value |

`event` (with its `args`), `schedule` and `path` are alternatives, each
alone in its object; conditions must all hold, then every window must let
the event through ([Across events: windows](/guides/workflows/#across-events-windows)).
A window is in a workflow's `on`, not a step's, and a schedule's run passes
none; an `on` has one `batch` or `at_least` at most. Any other key is an error naming it.

What a workflow is (its description, `off`, `from`, `until`, `internal`)
is in its entry in
[`itero.config.json`](/reference/itero-config/), not in its file.

## A step

| Form | Is |
|---|---|
| `"#name"` | an action, its latest version |
| `"#name@0.2"` | an action at a version |
| `"name"` | a workflow, a run of its own; its `$event` is the args its step gives, its result what it returns ([A workflow used as a step](/guides/workflows/#a-workflow-used-as-a-step)) |
| `["#a", "b"]` | side by side; the next step waits for all, and their results merge into one; a step in it may be an object (below) |
| `{"race": ["#a", "b"]}` | side by side, waiting as its key says (below) |
| `{"#name": {...}}` | with keys of its own (below) |

| A step's key | Is |
|---|---|
| `args` | what its action gets, and nothing else: an object, a list of objects read in order (an item with exactly `title` and `text` gives the arg `title` names its text, as text), or a value that is one (`"$output"`) |
| `on` | when it runs, conditions only; otherwise it is skipped |
| `undo` | what takes it back when a later step breaks: `"#name"`, or `{"#name": {"args": {...}}}`; given this step's result as `$output` |
| `repeat` | it runs in rounds, each with the one before's `$output`, `$steps` and `$count`: `true`, a number, `"$output.count"`, `"<= 2026-12-31"`, `"> 2026-12-31"` or a cron line ([Repeat](/guides/workflows/#repeat-rounds)); not side by side |
| `unique` (with `for`), `changed`, `at_most`, and `per` | a window: the step runs only when it lets it through, by the runs of the workflow it let through before; otherwise it is skipped and the run goes on ([A window on a step](/guides/workflows/#a-window-on-a-step)); an action's step, not side by side, not with `repeat` |

| A value | Is |
|---|---|
| `$event`, `$event.order` | the event that started the run |
| `$output`, `$output.total` | the step before's result; in the `error` list's first step, `why`, `failed_step`, `undo_failed` |
| `$steps.fetch-rates`, `$steps.fetch-rates.price` | an earlier step's result, by its name; `$steps.fetch-rates/2` for the second of that name in the file |
| `$config`, `$config.description` | `itero.config.json`, read-only |
| `$count` | the round of a repeated step or workflow, from 1 |
| `$$NAME` | a secret, a value in `.env`, or an environment variable; a whole value only |
| `{"file": "prompts/sort.md"}` | as an arg: that file's text, as written; its path from the workspace's folder, inside it, not `.env` or in `.itero/` |

Fields go to any depth. A whole value keeps its type; inside text, and in
lists and objects, each is filled (`""` when it is not there); `\\$` is a
dollar sign. Any other `$name` is an error of `itero ws check`, which says
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
or an object with `args` and `on` (not `undo`), each known by its name
(with its number when the name is in the file more than once,
`#ask-mirror/2`), and its result kept as `$steps.<name>`. `$output.failed` and `$output.why` name the steps that broke and why,
whenever one did (always with `settled` and `"fail": "never"`). The steps
that lose are stopped, never undone. A group of `all` or `settled` that
breaks waits for its running steps to end first. See
[How steps side by side wait](/guides/workflows/#how-steps-side-by-side-wait).
