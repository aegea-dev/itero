# Plan 24: values by where they come from

## Why

A workflow reads every value from one flat state today: the starting
event's fields (`$order`), a step's args (they join the state unless the
step is `strict`), `$output` (the step before), itero's own facts
(`$why`, `$failed_step`). An action gets a value as the first found in
its step's args, its own args, then the whole state, which it also gets
on stdin. A reader cannot tell where `$order` comes from, and an event
field named `output` or `why` clashes with itero's own.

A probe on Qwen3.5-4B (2026-10-30, 78 answers) found the models guessing
exactly here: `$output.x` in 61 of 71 workflows, the starting event's
fields included; `$input.x`, `$data.x`, `$settings.x` invented; `"$1"`
for an action's args in 39 of 78 answers.

## What changes

A workflow reads a value by where it comes from, with three words:

| Write | Means |
|---|---|
| `$event`, `$event.foo` | the event that started the run |
| `$output`, `$output.bar` | what the step just before gave (or a group's merged result) |
| `$steps.fetch-rates`, `$steps.fetch-rates.price` | what an earlier step gave, by its name |

And an action gets only its args.

### `$event`

- The data of the event that started the run, the same in every step and
  every list (`success`, `failure`, `error`) of the run.
- A schedule's run: `$event` is `{}`; a path's: `{"path", "kind"}`.
- A workflow used as a step: its `$event` is the args its caller's step
  gives it.
- Conditions read it: `{"has": "$event.url"}`, `{"$event.kind": "created"}`.

### `$output`

- What the step just before printed; after steps side by side, the
  group's merged result (`winner`, `failed`, `why` as today).
- In the `failure` and `error` lists, for the list's first step: why
  the run ended, `{"why", "failed_step", "undo_failed"}` (`$output.why`,
  `$output.failed_step`), as a group that breaks gives `$output.why` today;
  a later step of the list sees the step before, as anywhere.
  (Suggestion, to approve: the same object stays readable as
  `$steps.ended` in every step of the lists, `ended` being itero's, so a
  later step can still say why.)
- An undo step: the result of the step it takes back, as today.

### `$steps`

- Every step's result is kept for the rest of the run under the step's
  name: `#fetch-rates` is read as `$steps.fetch-rates`, a workflow
  `daily` used as a step as `$steps.daily`.
- `as` names a step for this, anywhere (today only inside a group): the
  same action twice, `"as": "eu"` and `"as": "us"`, are `$steps.eu` and
  `$steps.us`.
- A step name used twice in one workflow (its steps and its lists
  together) without `as` is an error of `itero check`, naming both.
- Steps side by side: each one's result under its own name
  (`$steps.size`, `$steps.type`); this replaces `$output.#size`.
- A step that has not run (yet, or at all): its name gives nothing
  (`null`, or empty inside text), as a missing field does today.
- A step that waited: its result is what it printed once done.

### Fields and text

- After `$event`, `$output` or `$steps.<name>`, a path of fields at any
  depth: `$event.order.id`. A dot followed by no field is text, as today
  (`$event.file.txt` with `file` a text is the file's name and `.txt`).
- A step's name runs to the first character that cannot be in an
  action's name (letters, digits, `_`, `-`), as `#name` does.
- `\$` writes a dollar sign; `$$NAME` stays a secret, a whole value only.
- Any other `$name` is no value: `itero check` says so, naming what to
  write (`$order` → `$event.order`; `$output.#size` → `$steps.size`;
  `$why` → `$output.why`).

### An action gets only its args

- Its environment: each arg as a variable (`"args": {"order":
  "$event.order"}` gives `$order`), from its step's args over its own
  args in `itero.config.json` (and a `repeat` item, as today); secrets
  resolved; plus itero's own `$ITERO_MEMORY` and `$ITERO_WAITING_SINCE`.
  Nothing from the run's state.
- Its stdin: one JSON object, `{"args": {...}}`, the same args (with
  values too big for a variable, or names a variable cannot have).
- An arg named `event` is an error: the name is itero's (below).
- Built-in steps (`#notify`, `#ai`) read their args as today.

### A waiting step

- An action that exits 75 waits, as today; what it printed is what an
  event is matched on, as today.
- When an event matches, only that step sees it: run again, its action
  gets the event's data as `$event` (JSON text) and in its stdin
  (`{"args": {...}, "event": {...}}`). On its first run there is none.
- Its args are filled as on its first run: in the workflow, `$event` is
  still the event that started the run.
- Later steps see only what it printed once done: `$output`,
  `$steps.<name>`.

### Gone

- The flat state: `$name` for an event's field, a step's args joining
  the state, `strict` (args join nothing now; `strict` in a workflow is an
  error naming why).
- `$output.#name`, `$why`, `$failed_step`, `$undo_failed` (now
  `$output.why` and so on in the lists).
- The whole state on an action's stdin.

## Where in the code

- `src/flow.rs`: names and fields (`pieces`, `lookup`, `fill`,
  `names_in`, `holds`); `as` on any step; the checks of unknown roots,
  `strict`, an arg named `event`, a name used twice.
- `src/engine/steps.rs`: the run keeps `event`, `output` and `steps`
  instead of a flat state; a step's result kept by name; the lists'
  `$output`; args no longer join anything.
- `src/engine/daemon.rs`, `src/engine/worker.rs`: an action's
  environment and stdin from its args alone; a waiting step's event as
  `$event`.
- `src/engine/events.rs`: conditions over `$event`.
- `src/cli/top.rs` (`itero check`): the errors above, each naming what
  to write.
- The examples: rewritten for the new forms, from scratch where that is
  simpler than moving them; each keeps a test that runs it.
- Outside this repo: the model harness's answers and guide are rewritten
  in the new forms too, from scratch if simpler.
- The docs: workflows, actions, connectors, secrets guides; the workflow
  file and glossary references.

## Tests

Unit tests in `flow.rs` for the names (roots, fields at depth, step
names with `-`, text, `\$`, secrets) and for each `itero check` error;
`steps.rs` for the run's `event`, `output`, `steps`, the lists'
`$output`, a group's members by name, `as`; the worker for an action's
environment and stdin (args only, `ITERO_*`, a waiting step's `$event`).
Every existing test moves to the new forms. `make verify`, `make
test-linux`, `make test-docker` (the mail examples' connectors and
workflows change); the examples' `bun test`.

## Shipping

One PR, commits split by topic (engine and its tests; examples; docs),
each under 30 files. A minor release, with a Breaking paragraph: the
three words, an action gets only its args (environment and stdin),
`strict` gone, the lists' `$output`. The model harness's rebuilt guide
teaches the new form from the start.
