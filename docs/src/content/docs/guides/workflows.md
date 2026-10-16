---
title: Writing a workflow
description: States, transitions, chains of actions, conditions and templates.
---

A workflow is a JSON file in one of the workspace's workflow folders
(`workflows/` unless [`itero.toml`](/reference/itero-toml/) says otherwise).
Every key is checked: an unknown key is an error naming the file, never
ignored. `itero check` reads every workflow and reports the first problem.

## States and transitions

```json
{
  "name": "backup",
  "states": ["start", "copied", "done"],
  "start": "start",
  "end": ["done"],
  "transitions": [
    { "from": "start", "to": "copied",
      "do": [ { "action": "command@1", "with": { "run": ["rsync", "-a", "notes/", "backup/notes/"] } } ] },
    { "from": "copied", "to": "done",
      "do": [ { "action": "notify@1", "with": { "title": "Backup", "text": "Notes copied" } } ] }
  ]
}
```

- `name`, states and ids are lowercase letters, digits and `-`.
- `failed` is the engine's own state: do not list it.
- A transition without `on` is taken as soon as the run enters its `from`
  state; when several could be taken, the first in the file whose `when`
  holds wins.
- A transition with `on` waits for an event of that type sent to the run
  (see [Apps and events](/guides/apps/)).

## A chain of actions

A transition's `do` is a list of actions, run in order. Each is a name, or
an object:

```json
{ "id": "check", "action": "mail.check@1",
  "when": "args.mailbox != ''",
  "with": { "mailbox": "${args.mailbox}" },
  "expect": "output.exit == 0" }
```

| Key | Means |
|---|---|
| `action` | a [built-in or your own action](/guides/actions/), with its version |
| `id` | names its output: `steps.<id>` for the rest of the chain |
| `when` | a condition: false skips the action, and `prev` passes through |
| `with` | its arguments; strings may hold `${...}` templates |
| `expect` | a condition over `output`: false means the action failed |
| `description` | text for people |

The chain moves to the transition's `to` state once every action is done.
If an action fails, the run goes to the transition's `on_fail` state, or
to `failed`.

## Conditions and templates

Conditions (`when`, `expect`) are [CEL](https://cel.dev) expressions. A
string in `with` may hold `${...}` templates, CEL expressions too. A string
that is one template keeps the value's type (a number stays a number);
otherwise the values are written into the text.

What expressions see:

| Name | Is |
|---|---|
| `args` | the arguments the trigger gave the run |
| `input` | what the transition started with: the event's data, or the output that ended the previous transition |
| `prev` | the previous action's output in this chain (at first, `input`) |
| `steps` | the outputs of this chain's actions with an `id` |
| `memory` | the workflow's memory |
| `run` | the run's `id`, `workflow` and `started` |
| `output` | the action's own output, in `expect` only |

`has(memory.seen) ? memory.seen : ''` reads a key that may not be there
yet. `${env.NAME}` is not an expression: it stands for a
[secret](/guides/secrets/), filled in only when the job starts.

## Timeouts

A transition with a `timeout` (`30s`, `5m`, `2h`, `7d`, `1h 30m`) needs an
`on_timeout` state:

- on a transition with `on`, the run moves to `on_timeout` when no such
  event came in time after entering `from`;
- on a transition with a chain, it moves there when the chain did not
  finish in time.

## Memory

`memory.set@1` sets keys in the workflow's memory:

```json
{ "action": "memory.set@1", "with": { "seen": "${steps.check.json.seen}" } }
```

The change is saved only when the whole chain succeeds, in the same
transaction as the rest of the answer. `itero memory <workflow>` and `GET
/workflows/<name>/memory` show it.

## Versions

A workflow's version is a hash of what it does: descriptions and triggers
are left out. A run is
pinned to the version it started with, so editing a workflow never changes
a run already under way. `itero check` prints each workflow's version.

All keys: [Workflow files](/reference/workflow-file/).
