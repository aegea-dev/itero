# itero, simple

This replaces the file formats in `design.md`. The engine underneath
stays: the journal, the daemon, workers, and runs that survive a restart.
What people write becomes as small as it can be.

The rule is KISS:
- as few fixed words as possible;
- tools already on the machine (`sh`, `curl`, `jq`) before programs of
  our own;
- defaults instead of settings.

## A workspace

```
itero.toml                    the daemon's settings (the port)
workflows/order.json          a workflow; its name is the file name
actions/check-payment         an action: an executable; its name is the file name
actions/notify@0.2            a version, in the name
sources/mail.received         a source: an executable; its messages are of this type
args/check-payment.json       data for an action, written by `itero args`
```

## Names at a glance

| Written | Is |
|---|---|
| `#check-payment` | an action |
| `order-process` | a workflow |
| `order.placed` | a message type: two or more words joined by `.` |
| `0 9 * * *` | a schedule, in cron form (it has spaces) |
| `$order` | a value in the run's state |
| `$$API_KEY` | an environment variable or a secret |

## A workflow

```json
{
  "on": ["order.placed"],
  "steps": [
    ["#check-payment", "#check-stock"],
    "order-process",
    "#send-mail"
  ],
  "success": ["#log-result"],
  "failure": ["#log-result"],
  "error": ["#log-result", "#alert-team"]
}
```

Its words: `on`, `steps`, and optionally `success`, `failure`, `error`
and `timeout`.

### `on`: when it runs

A list:
- **message types and schedules** are alternatives: any one of them starts
  a run (`["mail.received", "0 * * * *"]`);
- **`$name`** means that value is in the run's state;
- **`$$NAME`** means that environment variable or secret is defined.

The `$` and `$$` entries are conditions that must all hold:
`["order.received", "$$API"]` starts a run on `order.received`, only if
`API` is defined.

An object means exact values: `{"$order": 43}` holds when the state has
`order` equal to `43`.

A workflow whose `on` has no message type or schedule never starts by
itself: it runs only as a step of another workflow, and only if its `on`
holds there.

### `steps`: what it does

They run in order. A step is:

- **an action**, `"#check-payment"`, or **a workflow**, `"order-process"`.
  A workflow used as a step shares the run's state;
- **a list**, `["#check-payment", "#check-stock"]`: those run side by
  side, and the next step waits for all of them. A list holds actions, not
  workflows, for now;
- **an object**, an action or workflow with data for it and an optional
  condition:

  ```json
  { "#ship": { "carrier": "PTT", "on": { "$paid": true, "$stock": true } } }
  ```

  `on` is the condition, as above; the step runs only if it holds, and is
  skipped otherwise. The other keys are data for the action. A value that
  is exactly `"$name"` or `"$$NAME"` is replaced by that value:
  `{ "#ask-ai": { "key": "$$ANTHROPIC_KEY" } }`.

### `success`, `failure`, `error`: how it ended

Steps run at the end, by the outcome:

- **error**: something broke (an action exited with a code other than 0
  or 75, took too long, or a run waited too long). The steps stop at once
  and `error` runs.
- **failure**: the steps all ran, and the state says `"success": false`.
  That is a business outcome, not a broken workflow: out of stock, payment
  declined. `failure` runs.
- **success**: anything else. `success` runs.

The run ends as `done`, `failed` or `error`. These steps get `$why` and
`$failed_step` in the state. If one of them breaks, the run ends as
`error` and no other list runs.

### `timeout`

How long a run may wait for data (see exit 75), like `"2h"`. Unless
given, 1 day. A run still waiting then ends in `error`.

## An action

An executable file in `actions/`, in any language its first line names
(`#!/bin/sh`, `#!/usr/bin/env python3`), with no extension and no
description file.

- **Its name** is its file name. A version is part of it: `notify@0.2`.
  `#notify` is the highest version; `#notify@0.2` pins one.
- **It reads** its values as environment variables (`$host`, `$user`) and
  the run's whole state as JSON on stdin. A value that is not a string
  arrives as JSON text.
- **It writes** a JSON object on stdout, whose fields join the state. A
  field set to `null` is removed. Printing nothing adds nothing.
- **Its exit code:**
  - `0`: done;
  - `75`: not yet (the standard Unix "try again later"). What it printed
    still joins the state. The run waits, and the step runs again when
    matching data reaches the run, or every hour, with
    `$ITERO_WAITING_SINCE` set so it can decide to give up;
  - anything else: an error; why is the end of what it printed on stderr.
- **What it keeps between runs** (like "the last mail seen", for a
  workflow that polls) goes in the file `$ITERO_MEMORY`, one per workflow.
  Two runs of a polling workflow overlap only if one takes longer than its
  schedule's interval; itero does not guard against that.

`#notify` is built in: it shows `$text` (and `$title`) as a desktop
notification. A workspace action named `notify` replaces it.

Example, `actions/check-payment`:

```sh
#!/bin/sh
# asks the provider; a pending payment comes back later as payment.received
[ -n "$paid" ] && { echo "{\"paid\": $paid}"; exit 0; }
ref=$(curl -fsS --max-time 30 "https://pay.example.com/orders/$order" | jq -r .ref)
echo "{\"payment_ref\": \"$ref\"}"
exit 75
```

### Side by side

Actions in a list run at the same time, each with the same state. A field
that only one of them prints joins the state as it is. A field that more
than one prints is kept per action:

```json
"status": { "#check-payment": "ok", "#check-stock": "low" }
```

## Values

An action's environment variables come from, in this order (the first
found wins):

1. **the step's data** in the workflow;
2. **its args**, `args/<action>.json`;
3. **the run's state.**

Secrets and environment variables enter only by name, with `$$`, in a
step's data or in args. A secret is looked up in the OS secret store (the
macOS Keychain, the Secret Service on Linux), then in the daemon's
environment. It is never written into the state, the journal, a log or a
command line.

```sh
itero args check-payment '{"provider": "pay.example.com"}'
itero args check-payment --file args.json
itero secret API_KEY                    # asks for it, without echo
itero secret API_KEY --file key.txt
```

**A list in args runs the step once per item**, each with its own copy of
the state for the rest of the run:

```sh
itero args imap-check '[{"user": "me@example.com", "password": "$$ME_PASS"},
                        {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

## Messages

A message is a type and its data:

```sh
itero emit order.placed '{"order": 43, "total": 120}'
```
```
POST /events   {"type": "order.placed", "data": {"order": 43, "total": 120}}
```

They come from three kinds of source:
- **push**: an app or a webhook calls `POST /events`, or a source holds a
  connection open and prints a line when told something;
- **pull**: a workflow on a schedule asks, or a source that polls;
- **watch**: a source runs `itero watch <path>`, which prints a line each
  time a file changes, from the OS's own file events.

A **source** is an executable in `sources/` that keeps running; each line
it prints is the data of one message, of its file name's type. The daemon
starts it again when it stops. A source that keeps its own "last seen"
sends one message per new thing, so each run has its own:
`sources/mail.received` prints one line per new mail, with its sender and
subject.

What a message does:
- **It starts a run** of every workflow whose `on` lists its type (and
  whose conditions hold), all in parallel. Its data is the run's first
  state.
- **It reaches waiting runs** that it matches: it shares at least one
  field with the run's state, and agrees with it on every field they
  share. Every run it matches gets it. A run that starts waiting also gets
  the matching messages that arrived since the run began, so none is
  missed by arriving early.
- **It is handled once**: the daemon marks it handled in the same
  transaction as the runs it starts and the copies it sends.
- **It is kept** until its runs end, then 1 day more, then deleted.

## Runs

- Each run has a UUIDv7 id, and keeps the workflow version it started
  with (also a UUIDv7): editing a file never changes a run under way.
- Everything runs in parallel: every run, every workflow.
- A step may take 10 minutes. Past that, or when the daemon stops while it
  runs, it is tried again, up to 3 times; then it is an error.

## Commands

```
itero daemon                 run the workspace (itero service install: at login)
itero emit <type> '<data>'   send a message
itero args <action> '<data>' set an action's data (or --file)
itero secret <NAME>          set a secret (asks; or --file)
itero runs                   runs, newest first, and how each ended
itero errors                 runs that ended in error: which step and why
itero check                  read every file; say what is wrong
itero watch <path>           for sources: a line per change
```

## Examples

**New mail, as it arrives.** `sources/mail.received` watches the mailbox
(`itero watch`) and prints one line per new mail. `workflows/mail-alert.json`:

```json
{ "on": ["mail.received"], "steps": ["#notify"] }
```

(`#notify` shows `$text`; the source prints `{"text": "Ayşe: Toplantı"}`.)

**An IMAP mailbox, every minute** (pull). `workflows/imap-alert.json`:

```json
{
  "on": ["* * * * *"],
  "steps": [
    { "#imap-check": { "host": "imap.example.com", "user": "me@example.com", "password": "$$IMAP_PASS" } },
    { "#notify": { "on": ["$text"] } }
  ]
}
```

`#imap-check` is a few lines of shell around `curl` (it speaks IMAP, POP3
and SMTP), keeps the last mail seen in `$ITERO_MEMORY`, and prints
`{"text": …}` only when there is new mail. The password goes to `curl` on
stdin, never on its command line.

**An order.** `workflows/order.json` as above, and
`workflows/order-process.json`:

```json
{
  "on": ["$paid", "$stock"],
  "steps": [
    { "#ship":           { "on": { "$paid": true,  "$stock": true } } },
    { "#remind-payment": { "on": { "$paid": false, "$stock": true } } },
    { "#remind-stock":   { "on": { "$paid": true,  "$stock": false } } },
    { "#cancel":         { "on": { "$paid": false, "$stock": false } } }
  ]
}
```

1. `order.placed` with `{"order": 43, "total": 120}` starts a run.
2. `#check-payment` and `#check-stock` run side by side. The stock is
   there: `{"stock": true}`. The payment is pending: `#check-payment`
   prints `{"payment_ref": "pay_1"}` and exits 75.
3. The provider calls back: `payment.received` with
   `{"payment_ref": "pay_1", "paid": true}` matches by `payment_ref`;
   `#check-payment` runs again and prints `{"paid": true}`.
4. `order-process` runs `#ship`; `#send-mail` tells the customer;
   `success` runs `#log-result`; the run is `done`.
5. Had the stock been 0, `#remind-stock` would have printed
   `{"success": false}`, and `failure` would run instead.

**A page, summarized by an AI assistant, by mail.**
`workflows/page-digest.json`:

```json
{
  "on": ["0 9 * * *"],
  "steps": [
    { "#fetch": { "url": "https://example.com/news" } },
    { "#ask-ai": { "prompt": "Summarize this page in 5 bullets.", "key": "$$ANTHROPIC_KEY" } },
    { "#send-mail": { "to": "me@example.com", "password": "$$SMTP_PASS" } }
  ]
}
```

`#fetch` prints `{"page": …}`, `#ask-ai` sends `$prompt` and `$page` to
the assistant's API with `curl` and prints `{"text": …}`, and `#send-mail`
sends `$text` with `curl smtps://…`.

## What goes away

- `name`, `description`, `version`, and `action.json` altogether;
- `states`, `start`, `end`, `transitions`, `from`, `to`, `do`, `id`;
- `${...}` expressions, `when`, `expect`;
- `memory.set`, `concurrency`, `pause_after`, `limits`;
- workspace `triggers/`;
- the action types `python@`, `bun@` and `container` (a first line covers
  languages; a script can run `docker` itself);
- built-in actions other than `#notify`;
- `match`, `publish`, `"from": "*"`;
- `itero memory` and `itero resume` (no pausing, for now).

They stay in git history, to come back only when a real workflow needs
one.

## Later, when needed

- **Settings for a side-by-side list**, like
  `["#a", ["#b", "#c", {"wait_all": true}]]`.
- **Pausing a workflow** after errors in a row: a daemon setting.
- **Commands to create things** without knowing the folders:
  `itero workflows new <name> '<json>'`, `itero workflows update …`,
  `itero actions new <name> --file …`, `itero sources new …`. Each writes
  the file a person would.
- **A web page** to see workflows and runs, then to edit them; xyflow for
  the graph once branches are drawn.
- **`itero sub <type>`** for apps to follow messages, and NATS when one
  machine is not enough.
