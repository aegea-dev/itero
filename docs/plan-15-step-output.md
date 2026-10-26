# Plan 15: a step's result is `$output`, and only that

Today an action prints one JSON object and its fields join the run's
state, where any later step may read them. That wiring is invisible: a
field one step prints is read three steps later, and nothing in the
workflow says so; an action that prints a plain `3` fails. This plan makes
every step's result one field, `$output`, overwritten by the next step,
and the workflow passes on what it keeps, by name, as a step's args.

Status: spec decided with the owner (2026-10-26); building.

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
- **A step skipped by its `on` leaves `$output` as it was:** nothing ran.
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

- **A step's values go under `args`, an object,** as an entry's `args` in
  `itero.config.json` already is. A step object has `args`, `on`, `undo`
  and `strict`, and no other key; today's values straight in the step
  object are an error that names `args`.
- **A value `"$name"` is the state's, with its type**: `"file_count":
  "$output"` is the number `524`. `"$$NAME"` stays a secret.
- **`"$output.count"` is one field of an object**, typed; one level only;
  a field that is not there is `null`. Any `"$name.field"` reads the same
  way.
- **`"args": "$output"` passes a whole object:** each of its fields
  becomes an arg (an error when `$output` is not an object).
- **A step's args join the run's state before it runs**, the newer over
  the older, so every later step sees them.
- **`"strict": true` keeps a step's args to that step:** its action gets
  them, and the state does not. For values later steps should not see.
- **Secrets:** an arg `"$$NAME"` joins the state as any arg does, unless
  the step is strict. (Today a secret reaches only its own step's
  environment, never the state.)
- The action gets the state as today, as environment variables and as
  JSON on stdin, with its args over it; the entry's args in
  `itero.config.json` come under the step's.
- **A workflow run as a step** takes args the same way; its `$output` is
  its last step's.

### Steps side by side

`["#check-payment", "#check-stock"]` gives one `$output`: **the objects
they print, merged** (`{"paid": true, "stock": false}`). A field two of
them print is kept by step name (`{"#check-payment": ..., "#check-stock":
...}`), as itero does today; a plain value is kept under its step's name.
Then `{"order-process": {"args": "$output"}}` passes both.

### Waiting, and messages

- A step that says "not yet" (exit 75) has its printed result as
  `$output` too.
- **A message is the waiting run's** when it agrees with the waiting
  step's `$output` object on the fields they share, and shares at least
  one (today it is checked against the whole state).
- **The message becomes that step's `$output`** when it comes, and the
  step runs again with it.
- **`#notify` with `$wait` or `$buttons`** prints `{"notification":
  "<id>"}`; the click (`itero.notification.clicked`, with `notification`
  and `answer`) is taken by that step, and `#notify` finishes with the
  click as its `$output`: the next step reads `"$output.answer"`.
- A run started by a message starts its state with the message's data, as
  today; so do a workflow's settings.

### Undo, failure, the fields itero writes

- **An undo gets the result of the step it takes back as `$output`**
  (`#cluster-create` prints the cluster's id; `#cluster-delete` reads it
  as `$output`, whatever ran in between).
- **The `failure` list** runs when the steps ended with `"success":
  false` in the state, or in the last step's `$output` object.
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
| `src/engine/steps.rs` | `finished`, `waiting` and a message set `output` instead of `merge`; `find` and `keep_per_entry` build the side-by-side `$output`; `ours` checks the waiting step's `$output`; `Runtime.undo` keeps each undo with its step's `$output`; the failure route reads `$output` too; args join the state when a step starts |
| `src/engine/daemon.rs` | `bind` and `resolve`: args, `"$name.field"`, `"args": "$output"`; the log's `output` |
| `src/flow.rs` | a step object has `args`, `on`, `undo`, `strict`; `output` reserved |
| `src/cli/top.rs` | `itero check`: an old-style step is an error naming `args`; args naming `output` |
| `src/cli/workflow.rs` | `wf runs RUN` shows any `output` |

## Docs

- Guides: `actions.md` (what an action prints), `workflows.md` (the
  state, `$output`, args, strict, side by side, waiting, undo, failure),
  `first-workflow.md`, `connectors.md` (a message's data starts a run's
  state), `log.md`, `secrets.md` (a secret in args joins the state unless
  strict).
- Start: `concepts.md`, `quickstart.md`.
- Reference: `workflow-file.md`, `action-file.md`, `glossary.md`
  (`$output`, args, strict), `http.md`.
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
   object not merged; `""` after a silent step; a skipped step; args into
   the state, and kept out with `strict`; `"$output.count"`; `"args":
   "$output"`; side by side; a workflow step's `$output`; a waiting step
   and its message; `#notify` with a click; an undo with its step's
   `$output`; the failure route; `itero check` on an old-style step and on
   `output`.
2. The worker's `answer()` and `Outcome`, and `#notify`.
3. `steps.rs`: `output` instead of `merge`, args into the state, side by
   side, messages, undo, failure.
4. `flow.rs` and `daemon.rs`: `args`, `strict`, `"$name.field"`, the
   whole-object pass, the reserved name.
5. `itero check` and `wf runs`.
6. The examples, then their tests.
7. The docs.
8. `make verify`, `make test-docker`, `make test-linux`.

## Decided (the owner, 2026-10-25 and 2026-10-26)

- A step's result is the state's field `output`, `$output` to the next
  step, typed: a number stays a number, text stays text.
- `$output` changes at every step; what is not passed on as args is gone
  after the next step. A skipped step leaves it as it was.
- A printed object no longer merges into the state; it is `$output`, and
  the workflow passes it on as args to keep it.
- A step's values go under `args`, an object; args join the run's state,
  unless the step says `"strict": true` (the owner's idea, so values such
  as secrets reach no later step).
- `"$output.count"` reads one field, one level.
- `output` is reserved.
- Conditions read `$output` as any field: `{"on": [{"$output": true}]}`;
  another name holds only when it was passed on as args.
- Side by side: the printed objects merged into one `$output`, a field
  two print kept by step name.
- A workflow step's `$output` is its last step's.
- A message is matched against the waiting step's `$output` and becomes
  it.
- An undo gets its own step's `$output`.
- The failure list runs on `"success": false` in the state or in the
  last step's `$output`.

## Open

- A `$name` filled inside text (`"$output files in ~/$folder"`) and `\$`
  for a dollar sign: their own plan, after this one.
