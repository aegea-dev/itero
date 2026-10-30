---
title: Workflows
description: on, meta, steps, a step's result and args, side by side and how it waits, conditions, workflows as steps, and the lists run at the end.
---

A workflow says when something should happen, and what to do then,
step by step. It is a JSON file in `workflows/`, and its name is its file
name. Its steps are actions (small programs) and other workflows.

## Examples

These are from the `examples/` folder, each a workspace you can run.

**Check websites every minute** (`examples/beacon`, `workflows/sites.json`):

```json
{
  "on": [{ "schedule": "* * * * *" }],
  "steps": [
    { "#check-sites": { "args": { "sites": "$sites" } } },
    "#changed",
    { "#notify": { "on": [{ "has": "$output.text" }],
                   "args": { "title": "beacon: sites", "text": "$output.text" } } }
  ]
}
```

- `on`: every minute.
- `#check-sites` fetches each site in `$sites` (the workflow's
  [settings](#settings)) and says which are down: its result.
- `#changed` reads that result as `$output`, and keeps only what changed
  since the last run, as `{"text": ...}`.
- `#notify` shows that text, but only when there is one: no news, no
  notification.

**A notification for each new mail** (`examples/mail-alert`):

```json
{ "on": [{ "event": "mail.received" }], "steps": ["#notify"] }
```

- `on`: a `mail.received` event, which a connector sends for each new
  mail ([Connectors](/guides/connectors/)).
- `#notify` shows it: the event's data (sender, subject) is the run's
  state, and `#notify` takes its title and text from there.

**A daily digest by mail** (`examples/page-digest`):

```json
{
  "on": [{ "schedule": "0 9 * * *" }],
  "steps": [
    { "#fetch": { "args": { "url": "https://example.com/news" } } },
    { "#ask-ai": { "args": { "prompt": "Summarize this page in 5 bullets.",
                             "key": "$$ANTHROPIC_KEY", "page": "$output" },
                   "strict": true } },
    { "#send-mail": { "args": { "host": "smtp.example.com", "user": "me@example.com",
                                "password": "$$SMTP_PASS", "to": "me@example.com",
                                "subject": "Daily digest", "text": "$output" },
                      "strict": true } }
  ]
}
```

- `on`: every day at 09:00.
- `#fetch` gets the page: its result, `$output`, for the next step.
- `#ask-ai` summarizes it, with the API key kept as a secret
  (`$$ANTHROPIC_KEY`, [Args and secrets](/guides/secrets/)).
- `#send-mail` mails the summary, its password a secret too. Both steps
  are `strict`: the page, the key and the password reach no later step.

**An order** (`examples/order`, `workflows/order.json`):

```json
{
  "on": [{ "event": "order.placed" }],
  "steps": [
    ["#check-payment", "#check-stock"],
    { "order-process": { "args": "$output" } },
    { "#send-mail": { "args": "$output" } }
  ],
  "success": ["#log-result"],
  "failure": ["#log-result"],
  "error": ["#log-result", "#alert-team"]
}
```

- `on`: an `order.placed` event, sent by the shop (`itero emit` or
  `POST /events`).
- `["#check-payment", "#check-stock"]` run side by side; the next step
  waits for both.
- `order-process` is another workflow, used as a step: it ships, reminds
  or cancels, depending on payment and stock ([below](#what-it-does-steps)),
  which the two checks printed and the step passes on as its args.
- `#send-mail` tells the customer, with what `order-process` gave.
- At the end, `success`, `failure` or `error` runs, by how it went
  ([How it ended](#how-it-ended-success-failure-error)).

## When it runs: `on`

A list of objects, each saying one thing:

| Entry | Means |
|---|---|
| `{ "event": "order.placed" }` | an event of this type starts a run |
| `{ "schedule": "0 9 * * *" }` | a schedule starts a run (cron, the machine's local time) |
| `{ "path": "~/Pictures/inbox" }` | a change to this file or folder starts a run ([Watching files](/guides/connectors/#watching-files)) |
| `{ "$paid": true }` | condition: the state's `paid` is exactly `true` |
| `{ "$output.answer": "Yes" }` | condition: one field of a value, here the step before's result |
| `{ "has": "$order" }` | condition: `order` is in the state |
| `{ "has": ["$order", "$$API"] }` | condition: both are there; `$$API` is an environment variable or a secret |

`event`, `schedule` and `path` are alternatives: any of them starts a
run. The others are conditions that must all hold:

```json
"on": [{ "event": "order.received" }, { "has": "$$API" }]
```

starts a run on `order.received` only if `API` is defined. Values compare
exactly: `0` is not `false`. An `event`, `schedule` or `path` stands alone
in its object.

A workflow with no `event`, `schedule` or `path` never starts by itself:
it runs only as a step of another.

## About it: its entry

What a workflow is, apart from its work, is in the workspace's
`itero.config.json`, under its name, not in its file:

```json
{
  "workflow": {
    "order": {
      "description": "Orders from the shop",
      "from": "2026-11-01", "until": "2026-12-31",
      "owner": "ayşe", "tags": ["orders", "payments"]
    }
  }
}
```

| Key | Means |
|---|---|
| `description` | text for people |
| `from`, `until` | a date or an RFC 3339 time: outside them nothing starts it; runs under way finish. A date is a whole day. |
| `off` | `true`: nothing starts it, without deleting it |
| `internal` | `true`: it looks after the workspace ([The daemon's own workflows](/guides/maintenance/)) |
| `settings` | values its runs start with ([Settings](#settings)) |
| any other | yours: itero keeps it, `itero check` shows it, and nothing more |

`id`, `name`, `created`, `path` and `version` are itero's and cannot be
set ([The log, IDs and names](/guides/log/)). A `meta` in a workflow file
is an error saying where it goes.

## What it does: `steps`

Steps run in order. A step is:

- **an action**, `"#check-payment"`: an executable in `actions/`. `#name`
  is its latest version, `#name@0.2` a pinned one.
- **a workflow**, `"order-process"`: its steps run there, with the same
  state. Its result is its last step's.
- **a list**, `["#check-payment", "stock-check"]`: those run side by side,
  actions or workflows, and the next step waits for all of them.
- **an object**: a name with its args, and when it runs:

  ```json
  { "#ship": { "args": { "carrier": "PTT Kargo" }, "on": [{ "$paid": true, "$stock": true }] } }
  ```

  The step runs only if its `on` holds; otherwise it is skipped. A step's
  `on` holds conditions only: an event, a schedule or a path starts a
  workflow, never a step. `args` are values for it
  ([below](#a-steps-result-output)), `undo` names what takes it back
  ([Undo](#undo-taking-steps-back)), and `strict` keeps its args to
  itself. A step side by side may also have `as`
  ([below](#a-step-side-by-side-with-its-args)). It has no other keys.

So branches are steps with conditions. `order-process` picks one of four:

```json
{
  "on": [{ "has": ["$paid", "$stock"] }],
  "steps": [
    { "#ship":           { "on": [{ "$paid": true,  "$stock": true }] } },
    { "#remind-payment": { "on": [{ "$paid": false, "$stock": true }] } },
    { "#remind-stock":   { "on": [{ "$paid": true,  "$stock": false }] } },
    { "#cancel":         { "on": [{ "$paid": false, "$stock": false }] } }
  ]
}
```

## A step's result: `$output`

What a step prints is its result, `$output` to the next step: a number,
`true`, a list, an object or text ([Actions](/guides/actions/#what-it-says)).
The next step's result takes its place, so a step reads only the result
of the one just before it. A step skipped by its `on` leaves it as it was.

To keep a result, pass it on as a step's **args**. A step's args join the
run's state when it starts, so every later step sees them too:

```json
{ "steps": [
    "#count-files",
    { "#archive": { "args": { "count": "$output.count" } } },
    { "#notify": { "args": { "title": "Downloads", "text": "$count" } } }
] }
```

`#count-files` prints `{"count": 3, "folder": "Downloads"}`. `#archive`
is given its count, which joins the state as `count`; `#notify` comes after
`#archive`, whose result is now `$output`, and still has `$count`.

- A value that is exactly `"$name"` takes the state's value, with its type:
  `"$output"` is `524` when the step before printed `524`. `"$output.count"`
  is one field of an object (one level; `null` when it is not there).
  `"$$NAME"` is a secret ([Args and secrets](/guides/secrets/)).
- Inside text, each `$name` is filled ([below](#a-state-variable-inside-text)):
  `"$count files in ~/$folder"`.
- `"args": "$output"` passes a whole object on: each field is an arg. A
  result that is not an object is an error there.
- **Args may be a list** of objects, read in order as one set of values. An
  item gives each of its keys as it is, or, with exactly the keys `title`
  and `text`, gives the arg `title` names its `text` formatted as text:

  ```json
  { "#ask": { "args": [{ "url": "$url1", "tries": 3 }, { "title": "label", "text": "mirror $n" }] } }
  ```

  With `{"name": "Ayşe", "bar": 42}` in the state, `{"foo": "$bar"}` gives
  `foo` the number `42`, and `{"title": "foo", "text": "$bar"}` gives it
  the text `"42"`; `{"title": "foo", "text": "$name is bar"}` gives
  `"Ayşe is bar"`. A name may be given once in a list. As an object,
  `{"title": ..., "text": ...}` is the two args `title` and `text`, as
  `#notify` takes them.
- `output` is itero's: no arg has that name.
- `"strict": true` keeps a step's args to that step: its action gets
  them, and the state does not. For values no later step should see,
  such as a password: a secret in args otherwise joins the state as its
  name, `$$NAME`, and later actions get its value.
- Conditions read `$output` as any value:
  `{ "#apply": { "on": [{ "$output": true }] } }` runs when the step before
  printed `true`.

### A state variable inside text

```json
{ "#notify": { "args": { "text": "$output.count files in ~/$folder" } } }
```

With `{"count": 3}` as the step before's result and `folder` set to
`Downloads`, the text is "3 files in ~/Downloads".

- A state variable inside text is written as itself when it is text, as
  JSON otherwise (`3`, `true`, `[1,2]`). One that is not there, `null` or
  `""` is nothing: `"answer: $answer"` is "answer: ".
- A name is a letter or `_`, then letters, digits or `_`; it ends at
  anything else: `"$count."` is the count and a full stop.
- A dot reads a field when the value is an object that has it; otherwise
  the dot is text: `"$file.txt"`, with `file` set to `report`, is
  "report.txt".
- Texts inside lists and objects in args are filled too:
  `"buttons": ["Ship $order", "Cancel"]`.
- `\$` is a dollar sign, written `\\$` in a JSON file, as JSON has no `\$`:
  `"text": "\\$count is not filled"`. A `$` that starts no name is itself
  already: `"costs $5"`.
- A secret is a whole value only, never part of a text: `"Bearer $$TOKEN"`
  is refused by `itero check` and by a run, and so is a state variable
  holding a secret, passed in args, inside text.

## How it ended: `success`, `failure`, `error`

Steps run at the end, by the outcome:

- **error**: something broke: an action exited with a code other than 0
  or 75, a step was lost three times, or a run waited too long. The steps
  stop at once and `error` runs.
- **failure**: the steps all ran, and the state says `"success": false`
  (passed on as an arg), or the last step's result does: a business
  outcome such as out of stock. `failure` runs.
- **success**: anything else.

These steps get `$why` and `$failed_step` (and `$undo_failed` when an
undo broke). Before `error`, the steps that finished are undone ([Undo](#undo-taking-steps-back)). The run ends as `done`,
`failed` or `error`; if one of these steps breaks, it ends as `error` and
no other list runs. A run ending in error or failed also sends
`itero.run.error` or `itero.run.failed`, for workflows that watch over
others ([The daemon's own workflows](/guides/maintenance/)). A workflow used as a step has no end lists of its own:
the run's own workflow has them.

## Posting a result

An app that sends an event can get the result back when the run ends, even
days later: it sends its URL in the event's data, and the workflow's end
steps post the result there. From `examples/notify-api`:

```json
{ "on": [{ "event": "app.notify" }], "steps": ["#notify"], "timeout": "1h",
  "success": [{ "#post-answer": { "args": "$output" } }],
  "failure": ["#post-answer"], "error": ["#post-answer"] }
```

`#notify`'s result is the click; `success` passes it on, so it joins the
state, and `actions/post-answer` posts the run's state, which an action
gets on its stdin:

```sh
#!/bin/sh
curl -fsS --retry 5 -H 'content-type: application/json' --data-binary @- "$reply_url"
```

The app sends `{"type": "app.notify", "data": {..., "reply_url":
"http://localhost:3000/notified"}}` and gets the state back when the
notification is answered, or `$why` when the run broke.

## Settings

A workflow you share, or take from someone, is used as it is. What
changes from one person to another goes apart from it: its settings, in
its entry in `itero.config.json`, set with
[`itero wf config set`](/commands/workflow/#config).

```json
{ "on": [{ "schedule": "* * * * *" }],
  "steps": [{ "#check-sites": { "args": { "sites": "$sites" } } }, "#changed", "#notify"] }
```

```sh
itero wf config set sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
```

writes:

```json
{ "workflow": { "sites": { "settings": { "sites": ["https://example.com", "http://localhost:3000"] } } } }
```

- A run's state starts with the settings, then the event's data on top:
  `itero emit` can override a setting for one run. A step names one as
  `"$sites"`, an action gets it as `$sites`, and `on` conditions see it.
- Only the run's own workflow's settings: a workflow used as a step runs
  with the run's state.
- They are read when a run starts: editing them changes the next runs.

Args are an action's settings; settings are a workflow's.

## Undo: taking steps back

A step that makes something (a cluster, an order, a booking) can name
what takes it back, with `undo`:

```json
{
  "on": [{ "event": "cluster.requested" }],
  "steps": [
    { "#cluster-create": { "undo": "#cluster-delete" } },
    { "#tf-plan": { "args": "$output" } },
    "#check-plan",
    "#tf-apply",
    "#web-check"
  ],
  "error": [
    { "#notify": { "args": { "title": "$name", "text": "$why" } } },
    { "#notify": { "on": [{ "has": "$undo_failed" }],
                   "args": { "title": "$name is left", "text": "$undo_failed" } } }
  ]
}
```

When a step breaks, the undos of the steps that finished run, newest
first, then the `error` list. If `#check-plan` refuses the plan, or
`#web-check` gets no answer: `#cluster-delete`, which takes the app with
it, then the notification. If `#cluster-create` breaks: nothing to undo.
This is `examples/terraform`'s `cluster` workflow.

- An undo is an action or a workflow. It gets the state, with `$why` and
  `$failed_step`, and the result of the step it takes back as `$output`
  (`#cluster-create`'s cluster, whatever ran after it).
- Only steps that finished are undone: a step that broke half way cleans
  up after itself, or the `error` list does.
- A workflow used as a step can have an undo, and its own steps can too:
  each finished step counts.
- An undo that breaks is noted in the log (`undo.failed`) and in
  `$undo_failed`, a list of `"#step: why"` lines, so the `error` list can
  say what may be left; the others still run, and the run ends as
  `error` with the first `$why`.
- A `failure` (`"success": false`) undoes nothing: the steps all worked.
- `itero wf runs <run>` marks them: `#cluster-delete (undo)  done`.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for an event that
agrees with what the step printed ([What an event does](/guides/connectors/#what-an-event-does)).
That event is then the step's `$output`, and it runs again. `timeout` is
how long a run may wait, like `"2h"`; unless given, a day.

What the step prints is what an event is matched on. The fields of the
event that started the run are in the state from the first step on, as
`$order`, and in the state on the action's stdin, so the step can print
the one to match as it came:

```json
{ "on": [{ "event": "order.placed" }],
  "steps": ["#wait-payment",
            { "#notify": { "args": { "title": "Paid", "text": "Order $order: $output.amount" } } }] }
```

`actions/wait-payment`:

```sh
#!/bin/sh
# waits for the payment.received event of this run's order
if printf '%s' "$output" | jq -e '.amount' >/dev/null 2>&1; then
  echo "$output"; exit 0   # it came: its data is $output
fi
jq -c '{order}'            # the field to match, from the state on stdin
exit 75
```

`order.placed` with `{"order": 1042}` starts a run that waits;
`payment.received` with `{"order": 1042, "amount": 25}` reaches it, and
it tells "Order 1042: 25". A value from elsewhere, such as an earlier
step's result, is given to the waiting step through its args
(`{ "#wait-payment": { "args": { "order": "$output.id" } } }`) and
arrives as text, `$order`: an event agrees only with the same JSON value,
so `"1042"` is not `1042`.

## The result of steps side by side

Steps side by side start from the same state, and give one `$output`: the
objects they print, merged. A field more than one prints is kept by step,
and a result that is not an object is kept under its step's name:

```json
{ "paid": true, "stock": false, "status": { "#check-payment": "ok", "stock-check": "low" } }
```

A step after them reads a field as usual, `$output.paid`, and a result
kept under a step's name by that name: two steps that print plain values,
`#size` printing `1064` and `#type` printing `image/png`, give
`{"#size": 1064, "#type": "image/png"}`, read as `$output.#size` and
`$output.#type`:

```json
{ "steps": [["#size", "#type"],
            { "#notify": { "args": { "text": "$output.#size bytes, $output.#type" } } }] }
```

## How steps side by side wait

A list waits for all its steps, and one that breaks breaks the run once
the others have ended. An object says how else to wait, with one of five
keys and a list of two or more steps:

```json
"steps": [
  { "race":    ["#mirror-eu", "#mirror-us"] },
  { "any":     ["#mirror-eu", "#mirror-us"] },
  { "some":    ["#replica-a", "#replica-b", "#replica-c"], "count": 2 },
  { "some":    ["#replica-a", "#replica-b", "#replica-c"], "names": ["#replica-a", "#replica-c"] },
  { "settled": ["#check-web", "#check-mail", "#check-db"] }
]
```

| Key | Ends when | Breaks when |
|---|---|---|
| `all` | every step ended well (a plain list is `all`) | a step breaks |
| `race` | the first step ends | that first step broke |
| `any` | the first step ends well | every step broke |
| `some` with `count` | that many ended well | too many broke to reach `count` |
| `some` with `names` | the named steps ended well | a named step broke |
| `settled` | every step ended | never |

`fail` says when the group breaks in place of its key's rule: `"any"`
(a step broke), `"all"` (every step broke), `"never"`, a step's name, or
a list of names (one of them broke). `{ "all": [...], "fail": "never" }`
waits for all and never breaks. A group that breaks breaks the run, as a
step does: `$failed_step` is its first step that broke, `$why` its
reason. When `fail` names steps, `$failed_step` is the first of those
that broke: with `"fail": "#b"`, when `#a` breaks before `#b`,
`$failed_step` is `#b`. A group whose key can no longer be met and that
does not break (`any` with `"fail": "never"`, every step broke) ends when
all have.

When a group ends or breaks before all its steps have (`race`, `any`,
`some`), the others are stopped: a running one's process is ended, a
waiting one waits no more, and the run's story shows them `stopped` (a
workflow by its own name). itero undoes nothing for them; to clean up
after them, write the next step to do it.

`all` and `settled` wait for every step, even once one broke them: the
steps still running run to their end, then the group breaks (and the run
takes its undos and its `error` list). What those steps print is not
kept, a workflow among them goes on to no next step, and a step waiting
for an event waits no more, shown `stopped`.

A workflow in a group whose `on` does not hold did not run, and nor does
a step side by side whose own `on` does not hold: it is out of the group,
as if not written. It wins no race and counts for nothing.

After the group, `$output` is:

- `all`, `some`: the results merged, as above.
- `race`, `any`: the winner's result, and `$output.winner`, its name
  (`"#mirror-eu"`).
- `$output.failed`, the names of the steps that broke, and `$output.why`,
  each one's reason (`{"#mirror-eu": "exit 7: connection refused"}`),
  whenever a step broke; with `settled` or `"fail": "never"` always, `[]`
  when none broke.

`winner`, `failed` and `why` are itero's: a step's own field of that name
is not kept. `{"race": {...}}`, with an object, is a workflow named `race`
used as a step.

### A step side by side, with its args

A step in a group, or in a list side by side, may be written as an object,
as a step in `steps` is: with `args`, `on` and `strict` (not `undo`). To
ask one action twice side by side, give each step `as`, a name of its own:

```json
{ "any": [
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:9/price" }, "as": "mirror-1" } },
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:8765/price" }, "as": "mirror-2" } }
] }
```

The group knows a step by its `as`, written as names are (`#mirror-1` for
an action's step, the name alone for a workflow's), or else by its name:
`$output.#mirror-2`, `"fail": "#mirror-1"`, `"names"` and
`$output.winner` use it. Two steps the group would know by one name are
an error that says to give each `as`. A step's args side by side join the
state when the group starts, in the group's order, unless it is
`strict`.

All keys: [Workflow files](/reference/workflow-file/).
