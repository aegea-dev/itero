---
title: Workflows
description: on, steps, where a value comes from ($event, $output, $steps), args, side by side and how it waits, conditions, workflows as steps, and the lists run at the end.
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
    { "#check-sites": { "args": { "sites": ["https://example.com", "http://localhost:3000"] } } },
    { "#changed": { "args": { "now": "$output" } } },
    { "#notify": { "on": [{ "has": "$output.text" }],
                   "args": { "title": "beacon: sites", "text": "$output.text" } } }
  ]
}
```

- `on`: every minute.
- `#check-sites` fetches each site its args name and says which are
  down: its result.
- `#changed` is given that result, `$output`, as its arg `now`, and keeps
  only what changed since the last run, as `{"text": ...}`.
- `#notify` shows that text, but only when there is one: no news, no
  notification.

**A notification for each new mail** (`examples/mail-alert`):

```json
{
  "on": [{ "event": "mail.received" }],
  "steps": [{ "#notify": { "args": { "title": "Mail", "text": "$event.from: $event.subject" } } }]
}
```

- `on`: a `mail.received` event, which a connector sends for each new
  mail ([Connectors](/guides/connectors/)).
- `#notify` shows it: `$event` is the event's data, and its `from` and
  `subject` fields are written into the text.

**A daily digest by mail** (`examples/page-digest`):

```json
{
  "on": [{ "schedule": "0 9 * * *" }],
  "steps": [
    { "#fetch": { "args": { "url": "https://example.com/news" } } },
    { "#ask-ai": { "args": { "prompt": "Summarize this page in 5 bullets.",
                             "key": "$$ANTHROPIC_KEY", "page": "$output" } } },
    { "#send-mail": { "args": { "host": "smtp.example.com", "user": "me@example.com",
                                "password": "$$SMTP_PASS", "to": "me@example.com",
                                "subject": "Daily digest", "text": "$output" } } }
  ]
}
```

- `on`: every day at 09:00.
- `#fetch` gets the page: its result, `$output`, for the next step.
- `#ask-ai` summarizes it, with the API key kept as a secret
  (`$$ANTHROPIC_KEY`, [Args and secrets](/guides/secrets/)).
- `#send-mail` mails the summary, its password a secret too. A step's
  args reach its action and nothing else: no later step sees the key or
  the password.

**An order** (`examples/order`, `workflows/order.json`):

```json
{
  "on": [{ "event": "order.placed" }],
  "steps": [
    [
      { "#check-payment": { "args": { "order": "$event.order" } } },
      { "#check-stock": { "args": { "order": "$event.order" } } }
    ],
    { "order-process": { "args": { "order": "$event.order",
                                   "paid": "$steps.check-payment.paid",
                                   "stock": "$steps.check-stock.stock" } } }
  ],
  "success": [
    { "#send-mail": { "args": { "email": "$event.email", "order": "$event.order",
                                "tracking": "$output.tracking" } } },
    { "#log-result": { "args": { "order": "$event.order" } } }
  ],
  "failure": [
    { "#send-mail": { "args": { "email": "$event.email", "order": "$event.order" } } },
    { "#log-result": { "args": { "order": "$event.order", "why": "$steps.order-process.why" } } }
  ],
  "error": [
    { "#log-result": { "args": { "order": "$event.order", "failed_step": "$output.failed_step",
                                 "why": "$output.why" } } }
  ]
}
```

- `on`: an `order.placed` event, sent by the shop (`itero emit` or
  `POST /events`).
- `#check-payment` and `#check-stock` run side by side, each given the
  order from the event; the next step waits for both.
- `order-process` is another workflow, used as a step: it ships, reminds
  or cancels ([below](#what-it-does-steps)), given what the two checks
  printed, read by their names: `$steps.check-payment.paid`.
- At the end, `success`, `failure` or `error` runs, by how it went
  ([How it ended](#how-it-ended-success-failure-error)), and mails the
  customer.

## When it runs: `on`

A list of objects, each saying one thing:

| Entry | Means |
|---|---|
| `{ "event": "order.placed" }` | an event of this type starts a run |
| `{ "schedule": "0 9 * * *" }` | a schedule starts a run (cron, the machine's local time) |
| `{ "path": "~/Pictures/inbox" }` | a change to this file or folder starts a run ([Watching files](/guides/connectors/#watching-files)) |
| `{ "$event.paid": true }` | condition: the event's `paid` is exactly `true` |
| `{ "$output.answer": "Yes" }` | condition: a field of the step before's result (in a step's `on`) |
| `{ "has": "$event.order" }` | condition: the event has an `order` |
| `{ "has": ["$event.order", "$$API"] }` | condition: both are there; `$$API` is a secret, a value in `.env` or an environment variable |

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
| any other | yours: itero keeps it, `itero check` shows it, and nothing more |

The entry says what the workflow is, never values to work on: those come
from the event, a step's args or `.env` ([Values that change from one
machine to another](#values-that-change-from-one-machine-to-another)).

`id`, `name`, `created`, `path` and `version` are itero's and cannot be
set ([The log, IDs and names](/guides/log/)). A `meta` in a workflow file
is an error saying where it goes.

## What it does: `steps`

Steps run in order. A step is:

- **an action**, `"#check-payment"`: an executable in `actions/`. `#name`
  is its latest version, `#name@0.2` a pinned one.
- **a workflow**, `"order-process"`: its steps run there; its `$event` is
  the args its step gives. Its result is its last step's.
- **a list**, `["#check-payment", "stock-check"]`: those run side by side,
  actions or workflows, and the next step waits for all of them.
- **an object**: a name with its args, and when it runs:

  ```json
  { "#ship": { "args": { "carrier": "PTT Kargo" }, "on": [{ "$event.paid": true }] } }
  ```

  The step runs only if its `on` holds; otherwise it is skipped. A step's
  `on` holds conditions only: an event, a schedule or a path starts a
  workflow, never a step. `args` are the values its action gets
  ([below](#args-what-a-step-gives-its-action)), `undo` names what takes
  it back ([Undo](#undo-taking-steps-back)), and `as` names it for
  `$steps` ([below](#steps-every-step-by-its-name)). It has no other keys.

So branches are steps with conditions. `order-process` picks one of four,
by the args its caller gave it, its `$event`:

```json
{
  "on": [{ "has": ["$event.paid", "$event.stock"] }],
  "steps": [
    { "#ship":           { "on": [{ "$event.paid": true,  "$event.stock": true }],
                           "args": { "order": "$event.order" } } },
    { "#remind-payment": { "on": [{ "$event.paid": false, "$event.stock": true }] } },
    { "#remind-stock":   { "on": [{ "$event.paid": true,  "$event.stock": false }] } },
    { "#cancel":         { "on": [{ "$event.paid": false, "$event.stock": false }] } }
  ]
}
```

## Where a value comes from

A workflow reads a value by where it comes from:

| Write | Means |
|---|---|
| `$event`, `$event.order` | the event that started the run |
| `$output`, `$output.count` | what the step just before gave |
| `$steps.count-files`, `$steps.count-files.count` | what an earlier step gave, by its name |
| `$config`, `$config.description` | the workspace's `itero.config.json`, read-only |
| `$$NAME` | a secret, a value in the workspace's `.env`, or an environment variable ([Args and secrets](/guides/secrets/)) |

After `$event`, `$output` or `$steps.<name>`, fields go as deep as the
value does: `$event.order.id`. Any other `$name` is no value: `itero
check` says what to write instead (`$order` is `$event.order`).

### `$event`: what started the run

The data of the event that started the run, the same in every step and
in the lists at the end. A schedule's run has `{}`; a path's has
`{"path", "kind"}` ([Watching files](/guides/connectors/#watching-files)).
A workflow used as a step has the args its step gives it.

### `$output`: the step just before

What a step prints is its result, `$output` to the next step: a number,
`true`, a list, an object or text ([Actions](/guides/actions/#what-it-says)).
The next step's result takes its place. A step skipped by its `on` leaves
it as it was.

### `$steps`: every step by its name

Every step's result is kept for the rest of the run, under its name:
`#count-files` is read as `$steps.count-files`, a workflow `daily` used
as a step as `$steps.daily`.

```json
{ "steps": [
    "#count-files",
    { "#archive": { "args": { "folder": "$output.folder" } } },
    { "#notify": { "args": { "title": "Downloads",
                             "text": "$steps.count-files.count files archived" } } }
] }
```

`#count-files` prints `{"count": 3, "folder": "Downloads"}`. `#archive`
is given its folder; `#notify` comes after `#archive`, whose result is
now `$output`, and reads the count by the step's name.

- `as` gives a step another name: one action twice, `"as": "eu"` and
  `"as": "us"`, is read as `$steps.eu` and `$steps.us`. Two steps with
  one name are fine until a `$steps.<name>` reads it: then `itero check`
  asks for an `as`.
- `$steps.x` naming no step of the workflow is an error of `itero check`.
- A step that has not run (yet, or at all) gives nothing: `null`, or
  empty inside text.

## Args: what a step gives its action

An action gets only its args, written in the step that runs it: as
environment variables, and on its stdin ([What it gets](/guides/actions/#what-it-gets)).
Nothing else of the run reaches it.

```json
{ "#archive": { "args": { "folder": "$output.folder", "keep": 30, "to": "$$ARCHIVE" } } }
```

- A value that is exactly `"$event.x"`, `"$output"` and so on takes that
  value, with its type: `"$output"` is `524` when the step before printed
  `524`. A field that is not there is `null`, and an action gets no
  variable for it.
- `"$$NAME"` is a secret, a value in `.env` or an environment variable
  ([Args and secrets](/guides/secrets/)).
- Inside text, each value is filled ([below](#a-value-inside-text)):
  `"$output.count files in ~/$event.folder"`.
- `"args": "$output"` passes a whole object on: each field is an arg. A
  result that is not an object is an error there.
- **Args may be a list** of objects, read in order as one set of values. An
  item gives each of its keys as it is, or, with exactly the keys `title`
  and `text`, gives the arg `title` names its `text` formatted as text:

  ```json
  { "#ask": { "args": [{ "url": "$event.url", "tries": 3 }, { "title": "label", "text": "mirror $event.n" }] } }
  ```

  With `{"bar": 42}` as the event, `{"foo": "$event.bar"}` gives `foo` the
  number `42`, and `{"title": "foo", "text": "$event.bar"}` gives it the
  text `"42"`. A name may be given once in a list. As an object,
  `{"title": ..., "text": ...}` is the two args `title` and `text`, as
  `#notify` takes them.
- `output` and `event` are itero's: no arg has those names.
- Conditions read values the same way:
  `{ "#apply": { "on": [{ "$output": true }] } }` runs when the step before
  printed `true`.

### A value inside text

```json
{ "#notify": { "args": { "text": "$output.count files in ~/$event.folder" } } }
```

With `{"count": 3}` as the step before's result and `{"folder":
"Downloads"}` as the event, the text is "3 files in ~/Downloads".

- A value inside text is written as itself when it is text, as JSON
  otherwise (`3`, `true`, `[1,2]`). One that is not there, `null` or
  `""` is nothing: `"answer: $output.answer"` is "answer: ".
- A field's name is letters, digits, `_` or `-`; it ends at anything
  else: `"$output.count."` is the count and a full stop.
- A dot reads a field when the value is an object that has it; otherwise
  the dot is text: `"$event.file.txt"`, with `file` set to `report`, is
  "report.txt".
- Texts inside lists and objects in args are filled too:
  `"buttons": ["Ship $event.order", "Cancel"]`.
- `\$` is a dollar sign, written `\\$` in a JSON file, as JSON has no `\$`:
  `"text": "\\$count is not filled"`. A `$` that starts no name is itself
  already: `"costs $5"`.
- A secret is a whole value only, never part of a text: `"Bearer $$TOKEN"`
  is refused by `itero check` and by a run, and so is an event's value
  holding a secret, inside text.

## How it ended: `success`, `failure`, `error`

Steps run at the end, by the outcome:

- **error**: something broke: an action exited with a code other than 0
  or 75, a step was lost three times, or a run waited too long. The steps
  stop at once and `error` runs.
- **failure**: the steps all ran, and the last step's result says
  `"success": false`: a business outcome such as out of stock. `failure`
  runs.
- **success**: anything else.

The `error` list's first step gets why as its `$output`: `$output.why`,
`$output.failed_step`, and `$output.undo_failed` (a list, when an undo
broke). The `success` and `failure` lists start with the last step's
result as `$output`. Later steps of a list see the step before, as
anywhere, and `$event` and `$steps` as the steps did. Before `error`, the
steps that finished are undone ([Undo](#undo-taking-steps-back)). The run ends as `done`,
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
{ "on": [{ "event": "app.notify" }],
  "steps": [{ "#notify": { "args": { "title": "$event.title", "text": "$event.text",
                                     "buttons": "$event.buttons" } } }],
  "timeout": "1h",
  "success": [{ "#post-answer": { "args": { "reply_url": "$event.reply_url", "asked": "$event",
                                            "answer": "$output.answer" } } }],
  "error": [{ "#post-answer": { "args": { "reply_url": "$event.reply_url", "asked": "$event",
                                          "why": "$output.why" } } }] }
```

`#notify`'s result is the click; `success` gives `actions/post-answer`
what the app asked and the answer, and it posts its args (on its stdin)
but the URL:

```sh
#!/bin/sh
jq -c '.args | del(.reply_url)' |
  curl -fsS --retry 5 -H 'content-type: application/json' --data-binary @- "$reply_url"
```

The app sends `{"type": "app.notify", "data": {..., "reply_url":
"http://localhost:3000/notified"}}` and gets its request and the answer
back when the notification is answered, or `why` when the run broke.

## Values that change from one machine to another

A workflow you share, or take from someone, is used as it is. A value
that changes from one person to another, such as a folder or an address,
goes in the workspace's `.env`, and the workflow reads it as `$$NAME`:

```sh
# .env
SITES_TIMEOUT=10
ARCHIVE=~/Archive
```

```json
{ "#archive": { "args": { "to": "$$ARCHIVE" } } }
```

`$$NAME` is looked up in the OS secret store, then `.env`, then the
daemon's environment: the first found. Secrets stay in the store, never in
`.env` ([Args and secrets](/guides/secrets/)). A value for one run only
comes with its event: `itero emit sites.check '{"timeout": 5}'`, read as
`$event.timeout`.

## Undo: taking steps back

A step that makes something (a cluster, an order, a booking) can name
what takes it back, with `undo`:

```json
{
  "on": [{ "event": "cluster.requested" }],
  "steps": [
    { "#cluster-create": { "args": { "name": "$event.name" },
                           "undo": { "#cluster-delete": { "args": { "name": "$event.name" } } } } },
    { "#tf-plan": { "args": { "name": "$event.name" } } },
    { "#check-plan": { "args": { "plan": "$output" } } },
    { "#tf-apply": { "args": { "name": "$event.name" } } },
    { "#web-check": { "args": { "url": "$steps.cluster-create.url" } } }
  ],
  "error": [
    { "#notify": { "args": { "title": "$event.name", "text": "$output.why" } } }
  ]
}
```

When a step breaks, the undos of the steps that finished run, newest
first, then the `error` list. If `#check-plan` refuses the plan, or
`#web-check` gets no answer: `#cluster-delete`, which takes the app with
it, then the notification. If `#cluster-create` breaks: nothing to undo.
This is `examples/terraform`'s `cluster` workflow.

- An undo is an action or a workflow: `"undo": "#cluster-delete"`, or
  with args, `"undo": { "#cluster-delete": { "args": {...} } }`. Its args
  read `$event` and `$steps` as any step's do, and the result of the step
  it takes back as `$output` (`#cluster-create`'s cluster, whatever ran
  after it).
- Only steps that finished are undone: a step that broke half way cleans
  up after itself, or the `error` list does.
- A workflow used as a step can have an undo, and its own steps can too:
  each finished step counts.
- An undo that breaks is noted in the log (`undo.failed`) and in the
  `error` list's `$output.undo_failed`, a list of `"#step: why"` lines,
  so it can say what may be left; the others still run, and the run ends
  as `error` with the first why.
- A `failure` (`"success": false`) undoes nothing: the steps all worked.
- `itero wf runs <run>` marks them: `#cluster-delete (undo)  done`.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for an event that
agrees with what the step printed ([What an event does](/guides/connectors/#what-an-event-does)).
That event is then the step's `$output`, and it runs again. `timeout` is
how long a run may wait, like `"2h"`; unless given, a day.

What the step prints is what an event is matched on. When one agrees,
only that step sees it: run again, its action gets the event's data as
`$event` (JSON text) and on its stdin. Its args are as on its first run,
and later steps see only what it printed once done:

```json
{ "on": [{ "event": "order.placed" }],
  "steps": [{ "#wait-payment": { "args": { "order": "$event.order" } } },
            { "#notify": { "args": { "title": "Paid",
                                     "text": "Order $event.order: $output.amount" } } }] }
```

`actions/wait-payment`:

```sh
#!/bin/sh
# waits for the payment.received event of its order
if printf '%s' "${event:-}" | jq -e '.amount' >/dev/null 2>&1; then
  echo "$event"; exit 0          # it came: its data is the result
fi
jq -c '{order: .args.order}'     # the field to match, from its args on stdin
exit 75
```

`order.placed` with `{"order": 1042}` starts a run that waits;
`payment.received` with `{"order": 1042, "amount": 25}` reaches it, and
it tells "Order 1042: 25". An event agrees only with the same JSON value,
so `"1042"` is not `1042`: read the arg from stdin, where it keeps its
type, rather than from the environment, where it is text.

## The result of steps side by side

Each step side by side keeps its result under its own name, `$steps.size`,
`$steps.type`. After them, `$output` is their results merged: the objects
they print, a field more than one prints kept by step, and a result that
is not an object kept under its step's name:

```json
{ "paid": true, "stock": false, "status": { "check-payment": "ok", "stock-check": "low" } }
```

Two steps that print plain values, `#size` printing `1064` and `#type`
printing `image/png`, are read by name:

```json
{ "steps": [["#size", "#type"],
            { "#notify": { "args": { "text": "$steps.size bytes, $steps.type" } } }] }
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
step does: `failed_step` is its first step that broke, `why` its
reason. When `fail` names steps, `failed_step` is the first of those
that broke: with `"fail": "#b"`, when `#a` breaks before `#b`,
`failed_step` is `#b`. A group whose key can no longer be met and that
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
  (`"mirror-eu"`).
- `$output.failed`, the names of the steps that broke, and `$output.why`,
  each one's reason (`{"mirror-eu": "exit 7: connection refused"}`),
  whenever a step broke; with `settled` or `"fail": "never"` always, `[]`
  when none broke.

`winner`, `failed` and `why` are itero's: a step's own field of that name
is not kept. `{"race": {...}}`, with an object, is a workflow named `race`
used as a step.

### A step side by side, with its args

A step in a group, or in a list side by side, may be written as an object,
as a step in `steps` is: with `args`, `on` and `as` (not `undo`). To ask
one action twice side by side, give each step `as`, a name of its own:

```json
{ "any": [
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:9/price" }, "as": "mirror-1" } },
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:8765/price" }, "as": "mirror-2" } }
] }
```

The group knows a step by its `as`, or else by its name: `"fail"` and
`"names"` write it as names are (`#mirror-1` for an action's step, the
name alone for a workflow's); `$steps.mirror-2` and `$output.winner`
without the `#`. Two steps the group would know by one name are an error
that says to give each `as`.

All keys: [Workflow files](/reference/workflow-file/).
