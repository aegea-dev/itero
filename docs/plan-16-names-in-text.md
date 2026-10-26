# Plan 16: a `$name` filled inside text, and `\$` for a dollar sign

itero fills a value only when it is exactly `"$name"`; inside other text,
`"$count files in ~/$folder"`, the names stay as written. That is the
first thing people (and models) write, and it does nothing. This plan
fills a name wherever it is in a text, and gives a dollar sign meant as
itself a way to say so.

Status: spec decided with the owner (2026-10-26); building.

## Spec

### A name inside text

```json
{ "#notify": { "args": { "title": "Downloads", "text": "$count files in ~/$folder" } } }
```

With `{"count": 3, "folder": "Downloads"}` in the state, the text is
"3 files in ~/Downloads".

- **A name** is `$`, then a letter or `_`, then letters, digits or `_`,
  as today: `$count`, `$_x`, `$output`. It ends at the first other
  character: `"$count."` is the count and a full stop.
- **A value that is exactly `"$count"` keeps its type**, as today (`3`,
  not `"3"`), so conditions and numbers work as they do.
- **Inside text, a value is written as itself:** text as it is, a number
  or `true` as JSON (`3`, `true`), a list or an object as its JSON.
- **A state variable that is not there, `null` or `""` is `""` inside
  text:** `"answer: $answer"` is "answer: " (the owner, reversing "stays
  as written"). A whole value `"$answer"` is `null` then, as today.
- **`$name.field` inside text reads one field** when the value is an
  object that has it: `"$output.count files"`. Otherwise the dot is text:
  `"$file.txt"` with `file` set to `"report"` is "report.txt".
- **Texts inside lists and objects in args are filled too**, at any depth:
  `"buttons": ["Ship $order", "Cancel"]`; a whole `"$order"` there keeps
  its type as at the top.

### Secrets inside text

- **A secret, `$$NAME`, is never filled inside text:** only as a whole
  value, so it reaches no notification, log or state (decided).
- **`itero check` refuses `$$NAME` inside text**, and so does a run, with
  `why` saying a secret is a whole value only, rather than sending the
  text as written.
- **A state variable that holds a secret's name** (a secret passed in args
  joins the state as `"$$NAME"`) is refused inside text the same way.
- A watched `path` keeps its own rule: `$$NAME` there is an environment
  variable, filled inside the path, never a secret.

### A dollar sign: `\$`

- **`\$` is a dollar sign:** `"costs \$5 for \$HOME"` is "costs $5 for
  $HOME" (decided).
- **In a JSON file it is written `\\$`**, as JSON has no `\$` escape:
  `"text": "\\$count stays"`, said plainly in the docs.
- **A `$` that starts no name is itself already:** `"costs $5"`, `"a $ b"`,
  `"$"`. So `\$` is needed only before a name, and `itero check` does not
  warn of a lone `$`.

### Where

- Every step's args, the end lists' too, `#notify`'s too.
- An action's args in `itero.config.json`.
- A connector's args: it has no run state, so a `$name` inside text is an
  error, as a whole `"$name"` is today; `\$` is a dollar sign there too.
- Not conditions (`on`): they compare whole values.

## Where it touches the code

| Where | What changes |
|---|---|
| `src/flow.rs` | `fill`: a text with names in it filled from the state; `\$`; one name's reading shared by the whole value and the text |
| `src/engine/steps.rs` | a name inside text that holds a secret's name breaks the step |
| `src/engine/daemon.rs` | `resolve` for an action's args; connector args refuse a `$name` inside text |
| `src/cli/top.rs` | `itero check`: `$$NAME` inside text |

## Docs

- Guides: `workflows.md` (a name inside text, `\$`), `secrets.md` (a
  secret is a whole value), `first-workflow.md` (the expressions table).
- Reference: `workflow-file.md`, `glossary.md`.

## Tasks

1. Tests first, each watched failing: names inside text, the whole value
   still typed, a state variable not there, `null` or `""`, a list or an
   object inside text, texts nested in args, `$name.field` and
   `"$file.txt"`, `\$` (next to a name, at the start
   and the end, twice in a row), a secret inside text refused, a secret's
   name inside text refused, connector args.
2. `flow::fill`, then `resolve` and the connector's args.
3. `itero check`.
4. The docs; an example where it reads better (the beacon's `#notify`).
5. `make verify`, `make test-docker`, `make test-linux`.

## Decided (the owner, 2026-10-25 and 2026-10-26)

- A `$name` is filled inside text: `"$count files in ~/$folder"` is "3
  files in ~/Downloads".
- The whole value `"$count"` keeps its type.
- A state variable that is not there, `null` or `""` is `""` inside text
  (2026-10-26, reversing "stays as written" of 2026-10-25).
- A list or an object inside text is its JSON.
- `$name.field` inside text is a field only when the object has it.
- Texts nested in args are filled too.
- A secret is never filled inside text; `$$NAME` inside text, and a state
  variable holding a secret's name, are refused by `itero check` and by a
  run.
- `\$` is a dollar sign, written `\\$` in JSON; no warning for a lone `$`.

## Open
- `${name}`, for a name with a letter right after it (`"${count}x"`), as
  the shell has: not in this plan unless asked.
