---
title: Workflow files
description: Every key of a workflow file.
---

A workflow is a JSON file ending in `.json` in a workflows folder. An
unknown key is an error naming the file. [Writing a workflow](/guides/workflows/)
explains how the pieces fit.

## The workflow

| Key | Is | |
|---|---|---|
| `name` | lowercase letters, digits and `-`; unique in the workspace | needed |
| `description` | text for people | |
| `states` | every state a run can be in; not `failed`, the engine's own | needed |
| `start` | the state a run begins in | needed |
| `end` | the states where a run ends | |
| `triggers` | what starts a run | |
| `transitions` | the moves between states | |
| `concurrency` | `{"max": n, "then": "queue" \| "skip"}`: at most `n` runs under way; a start beyond waits its turn (`queue`, the default) or is dropped (`skip`) | |
| `pause_after` | failed runs in a row that pause the workflow (5 unless given; 0 never) | |

`concurrency` and `pause_after` are not part of the workflow's version:
they say how often it runs, not what a run does.

## Paused

When `pause_after` runs in a row have failed, the workflow is paused:
nothing starts it, the daemon's log says so, and the engine sends
`workflow.paused` to the workspace, with data `{workflow, version, failed,
run}` (the last failed run's id), so a maintenance workflow can act on it.
`itero resume <workflow>` starts it again, and so does a new version of its
file: the fix. `GET /workflows` shows a workflow's pause.

## A trigger

| Key | Is |
|---|---|
| `schedule` | a cron line, in the machine's local time |
| `on` | an event type, like `photo.added` |
| `args` | an object: the run's `args` |

One of `schedule` and `on`. See [Triggers](/guides/triggers/).

## A transition

| Key | Is | |
|---|---|---|
| `from` | the state it leaves | needed |
| `to` | the state it enters | needed |
| `on` | the event type that takes it; without, it is taken on entering `from` | |
| `when` | a condition that must hold for it to be taken | |
| `do` | the chain of actions, run in order | |
| `timeout` | a duration: `30s`, `5m`, `2h`, `7d`, `1h 30m` | with `on_timeout` |
| `on_timeout` | the state to go to when time runs out | with `timeout` |
| `on_fail` | the state to go to when an action fails, instead of `failed` | |
| `description` | text for people | |

A transition to its own state needs `on`, `do`, or both.

## An action in `do`

A name, like `"notify@1"`, or an object:

| Key | Is | |
|---|---|---|
| `action` | a built-in, or `<name>.<target>@<version>` from an actions folder | needed |
| `id` | names its output `steps.<id>`; unique in the workflow | |
| `when` | a condition: false skips it, and `prev` passes through | |
| `with` | its arguments; strings may hold `${...}` templates | |
| `expect` | a condition over `output`: false means it failed | |
| `description` | text for people | |

## Expressions

Conditions and templates are [CEL](https://cel.dev). They see `args`,
`input`, `prev`, `steps`, `memory` and `run`, and `expect` also sees
`output`. `${env.NAME}` is a [secret](/guides/secrets/), not an expression.
