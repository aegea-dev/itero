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
  (`$$ANTHROPIC_KEY`, [Secrets and environment variables](/guides/secrets/)).
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
    { "#log-result": { "args": { "order": "$event.order", "why": "$steps.order-process.output.why" } } }
  ],
  "error": [
    { "#log-result": { "args": { "order": "$event.order", "failed_step": "$output.failed_step",
                                 "why": "$output.why" } } }
  ]
}
```

- `on`: an `order.placed` event, sent by the shop (`itero ws emit` or
  `POST /events`).
- `#check-payment` and `#check-stock` run side by side, each given the
  order from the event; the next step waits for both.
- `order-process` is another workflow, used as a step: a run of its own
  that ships, reminds or cancels ([below](#what-it-does-steps)), given
  what the two checks printed, read by their names:
  `$steps.check-payment.paid`. What it returns is the next step's
  `$output` ([A workflow used as a step](#a-workflow-used-as-a-step)).
- At the end, `success`, `failure` or `error` runs, by how it went
  ([How it ended](#how-it-ended-success-failure-error)), and mails the
  customer.

## When it runs: `on`

A list of objects, each saying one thing:

| Entry | Means |
|---|---|
| `{ "event": "order.placed" }` | an event of this type starts a run |
| `{ "event": "mail.received", "args": { "MAILBOX": "sales" } }` | the same, and the args its connector is given ([One connector, many sets of args](/guides/connectors/#one-connector-many-sets-of-args)) |
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
exactly: `0` is not `false`. An `event` (with its `args`), a `schedule` or
a `path` stands alone in its object.

A workflow with no `event`, `schedule` or `path` never starts by itself:
it runs only as a step of another.

### Across events: windows

Each event starts a run of its own. A window in `on` looks across them,
at the runs started before, and lets an event start a run or holds it back:

| Window | A run starts |
|---|---|
| `{ "unique": "$event.id" }` | for a value no run started with: each delivery once |
| `{ "unique": "$event.id", "for": "1d" }` | the same, a value remembered for a day |
| `{ "changed": "$event.state" }` | when the value is not the last run's: `up`, `up`, `down`, `up` start three |
| `{ "at_most": "1h" }` | at most once an hour: the first event in the hour starts it, the rest start nothing |
| `{ "batch": { "count": 100, "within": "5m" } }` | one run for 100 events, or for those that came within 5 minutes of the first, whichever comes first |
| `{ "at_least": 5, "within": "10m" }` | one run once 5 events came within 10 minutes: "5 failures in 10 minutes" |

`per` keeps a window for each value: `{ "at_most": "1h", "per": "$event.site" }`
starts a run at most once an hour for each site. The conditions are checked
first, then every window must let the event through; only a run started
counts for the windows, so an event one window held back is not counted by
another. An event without the value counts as `null`.

```json
{ "on": [{ "event": "page.changed" },
         { "changed": "$event.text", "per": "$event.url" },
         { "at_most": "10m", "per": "$event.url" }],
  "steps": [{ "#notify": { "args": { "title": "$event.url", "text": "$event.text" } } }] }
```

tells a page's new text, at most once every ten minutes for each page.
`batch` (with a `count`, a `within` or both) and `at_least` (with its
`within`) keep the events they let through, and start one run of them:
its `$event` is `{"events": [...], "count": 3, "first": "<time>",
"last": "<time>"}`, the events' data in the order they came, and `per`,
the value its window is kept for, when it has one:

```json
{ "on": [{ "event": "order.failed" },
         { "at_least": 5, "within": "10m", "per": "$event.shop" }],
  "steps": [{ "#notify": { "args": { "title": "$event.per",
                                     "text": "$event.count failures in ten minutes" } } }] }
```

`at_least` starts again from none once it has started a run. An `on` has
one `batch` or `at_least` at most; the other windows come first, and an
event kept counts for them as one that starts a run (`unique` before a
batch keeps a value out of it once in). A batch whose time has come starts
its run as a schedule does, outside `limits.runs`; one whose workflow is
gone or off by then is dropped (`batch.dropped` in the log).

What a window let through or keeps is in the journal, so a daemon started
again goes on where it stopped; the log's `message` line says which
workflow a window `held` back, and why, and which `kept` it. A schedule's
run passes no window.

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
| any other | yours: itero keeps it, `itero ws check` shows it, and nothing more |

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
- **a workflow**, `"order-process"`: a run of its own, whose `$event` is
  the args its step gives. The step's result is what it returns
  ([A workflow used as a step](#a-workflow-used-as-a-step)).
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
  it back ([Undo](#undo-taking-steps-back)), `repeat` runs it in rounds
  ([Repeat](#repeat-rounds)). It has no other keys.

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
| `$count` | the round of a repeated step or workflow, from 1 ([Repeat](#repeat-rounds)) |
| `$$NAME` | a secret, a value in the workspace's `.env`, or an environment variable ([Secrets and environment variables](/guides/secrets/)) |

After `$event`, `$output` or `$steps.<name>`, fields go as deep as the
value does: `$event.order.id`. Any other `$name` is no value: `itero ws
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
`#count-files` is read as `$steps.count-files`. A workflow `daily` used
as a step is kept as all it returned: `$steps.daily.output`,
`$steps.daily.steps.<name>` and `$steps.daily.status`
([A workflow used as a step](#a-workflow-used-as-a-step)).

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

- One action (or workflow) used more than once in a workflow is told
  apart by its number: `$steps.ask-mirror/1`, `$steps.ask-mirror/2`,
  counted over the whole file (`steps`, then `success`, `failure`,
  `error`; a step side by side where it stands). The name alone is the
  first: `$steps.ask-mirror` is `$steps.ask-mirror/1`.
- `$steps.x` naming no step of the workflow is an error of `itero ws check`.
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
  ([Secrets and environment variables](/guides/secrets/)), for an action: a workflow used as
  a step writes it in its own step that needs it, as its `$event` is data.
- Inside text, each value is filled ([below](#a-value-inside-text)):
  `"$output.count files in ~/$event.folder"`.
- `"args": "$output"` passes a whole object on: each field is an arg. A
  result that is not an object is an error there.
- **A file** is an object with only the key `file`, and its text is the arg,
  exactly as written; nothing in it is filled:

  ```json
  { "#ai": { "args": { "system": { "file": "prompts/sort.md" }, "message": "$event.body" } } }
  ```

  The path is from the workspace's folder. The file is inside the workspace,
  and is not `.env` or in `.itero/`. It is read with the workflow, so a run
  keeps the text it started with, and `itero ws check` says when one cannot
  be used.
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
- A field's name is letters, digits or `_`; it ends at anything else:
  `"$output.count."` is the count and a full stop, `"$event.from-$event.to"`
  two values with a dash between.
- A `_` also finds a field with a `-` there, when there is none by the name as
  written: `$event.headers.x_github_event` reads a webhook's `x-github-event`.
- A dot reads a field when the value is an object that has it; otherwise
  the dot is text: `"$event.file.txt"`, with `file` set to `report`, is
  "report.txt".
- Texts inside lists and objects in args are filled too:
  `"buttons": ["Ship $event.order", "Cancel"]`.
- `\$` is a dollar sign, written `\\$` in a JSON file, as JSON has no `\$`:
  `"text": "\\$count is not filled"`. A `$` that starts no name is itself
  already: `"costs $5"`.
- A secret is a whole value only, never part of a text: `"Bearer $$TOKEN"`
  is refused by `itero ws check` and by a run.
- Only a `$$NAME` written in a step's args is a secret. In a value that came
  from an event or a step's result it is plain text, so nothing from outside
  can name a secret.

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
others ([The daemon's own workflows](/guides/maintenance/)).

A run can also be stopped by a person,
[`itero ws wf stop`](/commands/workflow/#stop): it ends `stopped` where it
is, its running step killed, with no undo and no list at the end.

## A workflow used as a step

A workflow is the same however it starts. Used as a step, it is a run of
its own, linked to the run whose step started it: its own id and name,
its `$event` (the args its step gives), its `$steps`, its end lists, its
undos, its `timeout` and its steps that wait. The step waits until that
run ends, then goes on with what it returned:

- With a `success` list, once it ended well: that list's result.
- With an `error` or `failure` list, once it broke or failed: that
  list's result.
- Without the list for how it ended: its last step's result, or, when it
  broke, why (`why`, `failed_step`, `undo_failed`, as an `error` list
  starts with).

What it returned is the step's `$output`, and, with its `$steps` and how
it ended, kept by its name: `$steps.<workflow>` is `{ "output", "steps",
"status" }`, `status` being `done`, `failed`, `error` or `stopped`.

A run that broke undoes its own finished steps first, then runs its
`error` list. A workflow used as a step that broke or failed is no error
for its parent: the parent reads what came back and does with it what it
wants, by its `status`:

```json
{ "steps": [
    "infrastructure",
    { "#notify": { "on": [{ "$steps.infrastructure.status": "error" }],
                   "args": { "title": "Setup", "text": "$output.why" } } }
] }
```

Parent and child are independent: the parent never sees the child's
steps but through what it returned, and the child's undos are its own.
Nothing the parent does stops its child: a child left behind by its
group (a `race` it lost) or by its parent breaking goes on to its end.
The parent's undo of the step (`{ "infrastructure": { "undo": "#teardown" } }`)
is the parent's, run when the parent breaks later.

`itero ws wf runs` lists the child's run under its own workflow,
`from <parent run>`; the parent's story shows it at its step:
`infrastructure  done  {...}  run misty-lake`
([`itero ws wf runs`](/commands/workflow/#runs)).

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
daemon's environment: the first found. `.env` may hold secrets too: it is
yours, local, and `itero ws add` keeps it out of git
([Secrets and environment variables](/guides/secrets/)). A value for one run only
comes with its event: `itero ws emit sites.check '{"timeout": 5}'`, read as
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
- A workflow used as a step is a run of its own: its steps' undos run
  when it breaks, before its own `error` list. The step that uses it can
  have an undo too, the parent's, run when the parent breaks; the child's
  steps are not undone then
  ([A workflow used as a step](#a-workflow-used-as-a-step)).
- An undo that breaks is noted in the log (`undo.failed`) and in the
  `error` list's `$output.undo_failed`, a list of `"#step: why"` lines,
  so it can say what may be left; the others still run, and the run ends
  as `error` with the first why.
- A `failure` (`"success": false`) undoes nothing: the steps all worked.
- `itero ws wf runs <run>` marks them: `#cluster-delete (undo)  done`.

## Repeat: rounds

`repeat` on a step runs it in rounds, one as soon as the one before has
ended; on a workflow (beside `on` and `steps`), each round is a run of
its own. Every output is input for the next round: each gets the round
before's `$output` and `$steps`, the run's `$event`, and `$count`, its
number from 1.

```json
{ "steps": [
    { "#fetch-page": { "args": { "page": "$count", "after": "$output.last" },
                       "repeat": "$output.pages" } },
    "#report"
] }
```

| `repeat` | Rounds |
|---|---|
| `true` | until one stops it (below) |
| `3` | three |
| `"$output.count"` | as many as the value says, read as the first round starts |
| `"<= 2026-12-31"` | while it is not past that day |
| `"> 2026-12-31"` | while it is past that day: before it, the first round only |
| `"0 9 * * *"` | on a step, at each time the cron line names: the run waits between rounds (a workflow's schedule is in its `on`) |

A round that fails (`"success": false`) or ends in error stops the repeat,
as does the workflow being turned `off` or being out of its `from` and
`until`. A round that breaks breaks the run, as any step. A repeated step
gives its last round's result. A workflow used as a step with a `repeat`
of its own answers its step after its last round; a repeated step that
is a workflow starts a run per round, its state seeded with the round
before's. A step side by side cannot repeat: put it in a workflow of its
own, repeated, used there by its name.

## Waiting: `timeout`

A step that exits 75 is not ready yet: the run waits for an event that
agrees with what the step printed ([What an event does](/guides/connectors/#what-an-event-does)),
then the step runs again. `timeout` is how long a run may wait, like
`"2h"`; unless given, a day.

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
workflow by its own name). A workflow among them is a run of its own: it
goes on to its end, and what it returns reaches nothing. itero undoes
nothing for them; to clean up after them, write the next step to do it.

`all` and `settled` wait for every step, even once one broke them: the
steps still running run to their end, then the group breaks (and the run
takes its undos and its `error` list). What those steps print is not
kept, a workflow among them runs to its end, and a step waiting for an
event waits no more, shown `stopped`.

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
as a step in `steps` is: with `args` and `on` (not `undo`). One action
asked twice side by side is told apart by its number, counted over the
whole file as for `$steps`:

```json
{ "any": [
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:9/price" } } },
    { "#ask-mirror": { "args": { "url": "http://127.0.0.1:8765/price" } } }
], "fail": "#ask-mirror/2" }
```

`"fail"` and `"names"` write a step as names are, with its number
(`#ask-mirror/2` for an action's step, the name alone for a workflow's);
`$steps.ask-mirror/2` and `$output.winner` (`"ask-mirror/2"`) without
the `#`.

All keys: [Workflow files](/reference/workflow-file/).
