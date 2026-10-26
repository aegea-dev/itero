# Plan 15: a step's result is `$output`, and only that

Today an action prints one JSON object and its fields join the run's
state, where any later step may read them. That wiring is invisible: a
field one step prints is read three steps later, and nothing in the
workflow says so; an action that prints a plain `3` fails. This plan makes
every step's result one field, `$output`, overwritten by the next step,
and the workflow passes on what it keeps, by name, as a step's args.

Status: spec for the owner's review. Points marked **(suggestion)** are
mine and wait for a yes; the rest the owner decided (Decided, below).

## Spec

### A step's result

- **Every step's result is `$output`.** itero reads what the step printed,
  trimmed: JSON when it is JSON (a number, `true` or `false`, a list, an
  object, a string in quotes), else the text itself. `524` and `"524"`
  are both fine. A step that prints nothing has `""`, never the step
  before's.
- **Nothing else joins the state.** A printed object's fields no longer
  do; it is `$output` as a whole. A field set to `null` no longer removes
  one.
- **`$output` is overwritten at every step's end**, so a step reads only
  the result of the step just before it. To keep a result for later, the
  workflow passes it on as args (below).
- A step skipped by its `on` leaves `$output` as it was, since nothing ran
  **(suggestion)**.
- **`output` is reserved:** args may not name it, and `itero check` says
  so.
- **Conditions read `$output` as any field:** `{"#ship": {"on":
  [{"$output": true}]}}` runs when the step before printed `true`;
  `{"has": "$text"}` holds only when `text` was passed on as args.
  An action that prints `true` or `false` is all a condition needs.

### Args, and how they join the state

```json
{"steps": [
  {"#count-files": {"args": {"folder": "~/Downloads"}}},
  {"#notify":      {"args": {"title": "Downloads", "text": "$output"}}}
]}
```

- **A step's values go under `args`,** next to `on` and `undo`; a step has
  these three keys and no others **(suggestion: an object, as an entry's
  `args` in `itero.config.json` already is, rather than a list of `name`
  and `value`; today the values sit straight in the step object)**.
- **A value `"$name"` is the state's, with its type**: `"file_count":
  "$output"` is the number `524`. `"$$NAME"` stays a secret.
- **`"args": "$output"` passes a whole object:** each of its fields
  becomes an arg (an error when `$output` is not an object).
- **A step's args join the run's state before it runs**, the newer over
  the older, so later steps see them too **(suggestion: the run's state,
  not only that step's; with `$output` overwritten every step, a value
  could otherwise travel only one step)**.
- The action gets the state as today: as environment variables and as
  JSON on stdin; the entry's args in `itero.config.json` come under the
  step's.
- **A workflow run as a step** takes args the same way; its `$output` is
  its last step's **(suggestion)**.
- **One field of an object**, `"$output.count"` **(question: a new term;
  without it, keeping one field means passing the whole object)**.

### Steps side by side

`["#check-payment", "#check-stock"]` gives one `$output`. Two ways
**(to settle)**:

- **A, by step name:** `{"#check-payment": {"paid": true},
  "#check-stock": {"stock": false}}`. Plain, but reading one needs
  `"$output.#check-payment"`, or passing the whole and then names with a
  `#` in them.
- **B, objects merged (suggestion):** objects printed side by side merge
  into one `$output` (`{"paid": true, "stock": false}`); a field two of
  them print is kept by step name, as itero does today; a plain value is
  kept under its step's name. Then `{"order-process": {"args":
  "$output"}}` passes both.

### Waiting, and messages

- A step that says "not yet" (exit 75) has its printed result as
  `$output` too.
- **A message is the waiting run's** when it agrees with the waiting
  step's `$output` object on the fields they share, and shares at least
  one (today it is checked against the whole state) **(suggestion)**.
- **The message becomes that step's `$output`** when it comes; the step
  runs again and finishes with it **(suggestion)**.
- **`#notify` with `$wait` or `$buttons`:** prints `{"notification":
  "<id>"}`; the click (`itero.notification.clicked`, with `notification`
  and `answer`) is taken by that step; `#notify` finishes with the click
  as its `$output`, so the next step reads `$output`, or is passed
  `"args": "$output"` to have `$answer`.
- A run started by a message starts its state with the message's data, as
  today; so do a workflow's settings.

### Undo, failure, the fields itero writes

- **An undo gets the result of the step it takes back as `$output`**
  (`#cluster-create` prints the cluster's id; `#cluster-delete` reads it
  as `$output`, whatever ran in between) **(suggestion)**.
- **The `failure` list** runs when the steps ended with `"success":
  false` in the state, or in the last step's `$output` object
  **(suggestion: today an action's printed `success` joins the state; the
  last step has no next step to pass it on)**.
- itero's own fields stay as they are: `why`, `failed_step`,
  `undo_failed`.

### Logs and commands

- `step.finished` and `step.waiting` log `output` as whatever the step
  printed (today always an object); `itero wf runs RUN` shows it.
- A breaking change for every workflow and every action that prints an
  object: a minor version, with a Breaking note. No old form is kept:
  nobody depends on it yet. Runs still in a journal when the daemon is
  upgraded go on under the new rules.

## Where it touches the code

| Where | What changes |
|---|---|
| `src/engine/worker.rs` | `answer()` returns any JSON value or text, not only an object; `Outcome` carries a `Value`; `#notify` prints only its id and finishes with the click |
| `src/engine/steps.rs` | `finished`, `waiting` and a message set `output` instead of `merge`; `find` and `keep_per_entry` build the side-by-side `$output`; `ours` checks the waiting step's `$output`; `Runtime.undo` keeps each undo with its step's `$output`; the failure route reads `$output` too |
| `src/engine/daemon.rs` | `bind` and `resolve`: args joined into the state before a step; `"args": "$output"`; the log's `output` |
| `src/flow.rs` | a step object has `args`, `on`, `undo`; `output` reserved; `$output.field` if agreed |
| `src/cli/top.rs` | `itero check`: an old-style step (values straight in it) is an error naming `args`; args naming `output` |
| `src/cli/workflow.rs` | `wf runs RUN` shows any `output` |

## Docs

- Guides: `actions.md` (what an action prints), `workflows.md` (the
  state, `$output`, args, side by side, waiting, undo, failure),
  `first-workflow.md`, `connectors.md` (a message's data starts a run's
  state), `log.md`.
- Start: `concepts.md`, `quickstart.md`.
- Reference: `workflow-file.md`, `action-file.md`, `glossary.md`
  (`$output`, args), `http.md`.
- Commands: `workflow.md`, `emit.md`, `help.md` where they show a printed
  object.

## Examples

Every example whose steps read what an earlier step printed changes, with
its `bun test`:

| Example | What it relies on today |
|---|---|
| beacon | `#check-sites` and `#check-disk` print `text`; `#changed` and `#notify` read it |
| languages | `summarize`: `#fetch`, `#to-markdown`, `#ask-ai`, `#print` hand fields down the chain |
| page-summary, page-digest | the same chain, then `#print` or `#send-mail` |
| mail-servers | `#imap-check` and `#pop3-check` print `text` for `#notify` |
| order | `paid` and `stock` from two steps side by side, read by `order-process`'s conditions |
| terraform | `#cluster-create`'s output read by later steps, and by `#cluster-delete` as its undo |
| notify-api | `#post-answer` reads the click's `answer` |
| gmail, mail-alert, photos, terraform's `tools` and `remove` | the message's data only: check, and keep |

## Tasks

1. Tests first, each watched failing: a plain value as `$output`; an
   object not merged; `""` after a silent step; args into the state;
   `"args": "$output"`; side by side; a waiting step and its message;
   `#notify` with a click; an undo with its step's `$output`; the failure
   route; `itero check` on an old-style step and on `output`.
2. The worker's `answer()` and `Outcome`.
3. `steps.rs`: `output` instead of `merge`, side by side, messages, undo,
   failure.
4. `flow.rs` and `daemon.rs`: `args`, the whole-object pass, the reserved
   name, `$output.field` if agreed.
5. `itero check` and `wf runs`.
6. The examples, then their tests.
7. The docs.
8. `make verify`, `make test-docker`, `make test-linux`.

## Decided (the owner, 2026-10-25 and 2026-10-26)

- A step's result is the state's field `output`, `$output` to the next
  step, typed: a number stays a number, text stays text.
- `$output` changes at every step; what is not passed on as args is gone
  after the next step.
- A printed object no longer merges into the state; it is `$output`, and
  the workflow passes it on as args to keep it.
- A step's args join the state.
- `output` is reserved.
- Conditions read `$output` as any field: `{"on": [{"$output": true}]}`;
  another name holds only when it was passed on as args.

## Open

- The suggestions above: args under `args` as an object, args for the
  run, a skipped step's `$output`, a workflow step's `$output`, side by
  side (A or B), messages matched by the waiting step's `$output`, an
  undo's `$output`, the failure route.
- `"$output.count"`: one field of an object.
- A `$name` filled inside text (`"$output files in ~/$folder"`) and `\$`
  for a dollar sign: their own plan, after this one.
