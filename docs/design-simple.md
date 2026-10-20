# itero, simple

This replaces the file formats in `design.md`. The engine underneath
stays: the journal, the daemon, workers, and runs that survive a restart.
What people write becomes as small as it can be.

The rule is KISS:
- as few fixed words as possible;
- tools that are already on the machine (`sh`, `curl`, `jq`) before
  programs of our own;
- defaults instead of settings.

## A workspace

```
itero.toml                    the port
workflows/mail-alert.json     a workflow; its name is the file name
actions/imap-check            an action; its name is the file name
actions/notify@0.2            a version, in the name
sources/mail.changed          a source; the messages it sends are of this type
args/imap-check.json          data for an action, written by `itero args`
```

## A workflow

Two fixed words: `on` and `steps`.

```json
{
  "on": ["mail.changed", "0 * * * *"],
  "steps": ["mail-check", "notify"]
}
```

- **`on`**: what starts a run. Each entry is either a message type
  (`mail.changed`) or a schedule in cron form (`0 * * * *`); a schedule is
  told apart by its spaces.
- **`steps`**: the actions, run in order. A step is an action's name, or a
  name with data for it:

  ```json
  { "imap-check": { "host": "imap.example.com" } }
  ```

- **Versions**: `imap-check` is its latest version; `imap-check@0.5` pins
  one. Data is never keyed by version: `imap-check` is `imap-check` at
  every version.

## An action

An executable file in `actions/`, in any language its first line names
(`#!/bin/sh`, `#!/usr/bin/env python3`). There is no description file.

- **Its name** is its file name, with no extension: its first line says
  what runs it. A version, if any, is part of the name: `notify@0.2`. With
  several versions, the highest is the latest.
- **It reads** each value it needs as an environment variable named after
  it (`$host`, `$user`, `$password`), and the run's whole state as JSON on
  stdin. A value that is not a string arrives as JSON text.
- **It writes** a JSON object on stdout, whose fields join the run's state.
  A field set to `null` is removed.
- **Its exit code** says how it went:
  - `0` with output: done, and the run goes on to the next step;
  - `0` with no output: done, and the run ends here, quietly (for example,
    no new mail, so nothing to notify);
  - `75`: not yet. The run waits, and the step runs again when new data
    reaches the run (see Messages). 75 is the standard Unix code for "try
    again later" (`EX_TEMPFAIL`);
  - anything else: the run fails, and why is the end of what the action
    printed on stderr.
- **What it keeps between runs** (like "the last mail seen") goes in the
  file `$ITERO_MEMORY`, which it may read and write. itero keeps one per
  workflow and action.

Example, `actions/check-payment`:

```sh
#!/bin/sh
[ -n "$amount" ] || exit 75           # no payment yet: run me again when data comes
[ "$amount" -ge "$total" ] || exit 1  # paid too little: the run fails
echo '{"paid": true}'
```

## Where an action's values come from

itero looks a name up in four places, in this order, and takes the first
it finds:

1. **The step's data** in the workflow: `{ "imap-check": { "host": "..." } }`.
2. **The action's args**: `args/imap-check.json`.
3. **The run's state**: the message that started the run, messages that
   reached it since, and what earlier steps printed. Newer wins over older.
4. **Secrets**, in the OS secret store (the macOS Keychain, the Secret
   Service on Linux), named `<action>.<name>`: `imap-check.password`.

Secrets are bound when a step starts and never written into the state, the
journal, a log or a command line.

### Args

```sh
itero args imap-check --json '{"host": "imap.example.com"}'
itero args imap-check --file accounts.json
```

Either writes `args/imap-check.json`, which can also be edited by hand.

**A list runs the step once per item**, each with its own copy of the
state for the rest of the run:

```sh
itero args imap-check --json '[{"user": "me@example.com"}, {"user": "you@example.com"}]'
```

`imap-check` runs twice, `$user` set to each in turn, and each goes on to
`notify` with its own result.

### Secrets

```sh
itero secret imap-check password                       # asks for it, without echo
itero secret imap-check password --for me@example.com  # one account's, asked for
itero secret imap-check password --file passwords.json
itero secret imap-check password --json - < passwords.json
```

With a file or stdin, the value is one secret, or an object with one per
account:

```json
{ "me@example.com": "…", "you@example.com": "…" }
```

A step whose `$user` is `me@example.com` gets that account's password; a
secret without accounts is shared. Values never go on the command line,
where `ps` and the shell's history would show them.

## Messages

A message has a type and its data:

```json
{ "type": "payment.received", "data": { "order": 43, "amount": 120 } }
```

Sending one:

```sh
itero emit payment.received --json '{"order": 43, "amount": 120}'
```
```
POST /events   {"type": "payment.received", "data": {"order": 43, "amount": 120}}
```

A source is an executable in `sources/` that keeps running and prints the
data of one message per line; the messages' type is its file name.
`sources/mail.changed` could be:

```sh
#!/bin/sh
itero watch "/var/mail/$USER"
```

(`itero watch` prints a line each time a file changes, using the OS's own
file events.)

What a message does:

- **It starts a run** of every workflow with its type in `on`. Its data is
  the run's first state.
- **It reaches waiting runs** (a step that exited 75) that it matches: it
  shares at least one field with the run's state, and agrees with it on
  every field they share. `{"order": 43, "amount": 120}` matches the run
  whose state has `"order": 43`, not the one with `"order": 42`. Its data
  joins the state, and the waiting step runs again.
- Messages wait in the journal until the daemon takes them: none is lost
  when the daemon stops.

## Defaults

None of these are written in files:

- **One run at a time per workflow.** Others wait their turn.
- **A step may take 10 minutes**, then the run fails.
- **A lost step** (the daemon or the machine stopped) runs again, up to 3
  times.
- **5 failed runs in a row pause a workflow** until `itero resume
  <workflow>`, or a new version of its file.

## Commands

```
itero daemon            run the workspace (itero service install: at login)
itero emit <type>       send a message
itero args <action>     set an action's data
itero secret <action> <name>   set a secret
itero runs              runs, newest first
itero errors            failed runs: which step and why
itero resume <workflow> after a pause
itero check             read every file; say what is wrong
itero watch <path>      for sources: a line per change
```

## Examples

**New local mail, as it arrives.** `workflows/mail-alert.json`:

```json
{ "on": ["mail.changed", "0 * * * *"], "steps": ["mail-check", "notify"] }
```

`mail-check` reads the mailbox and remembers the last mail seen in
`$ITERO_MEMORY`. With nothing new it prints nothing, and the run ends.
Otherwise it prints `{"text": "2 new: …"}`, which `notify` shows.

**New mail on an IMAP server, every minute.** `workflows/imap-alert.json`:

```json
{
  "on": ["* * * * *"],
  "steps": [{ "imap-check": { "host": "imap.example.com" } }, "notify"]
}
```

`actions/imap-check` is a few lines of shell around `curl`, which speaks
IMAP, POP3 and SMTP: no Docker, no Python. It asks the server for mail
newer than the last one in `$ITERO_MEMORY`, and passes the password to
`curl` on stdin, never on its command line. A POP3 check is the same with
`pop3s://`.

**An order.** `workflows/order.json`:

```json
{
  "on": ["order.placed"],
  "steps": ["check-payment", "reserve-stock", "ship", "send-mail"]
}
```

1. `order.placed` with `{"order": 43, "email": "ayse@example.com", "total": 120}`
   starts a run.
2. `check-payment` finds no `$amount` and exits 75: the run waits.
3. `payment.received` with `{"order": 43, "amount": 120}` matches it by
   `order`; `check-payment` runs again and prints `{"paid": true}`.
4. `reserve-stock` calls the warehouse's API with `curl`, `ship` prints
   `{"tracking": "TR123"}`, and `send-mail` uses `$email` and `$tracking`.

**A page, summarized by an AI assistant, by mail.**
`workflows/page-digest.json`:

```json
{ "on": ["0 9 * * *"], "steps": ["fetch", "ask-ai", "send-mail"] }
```

```sh
itero args fetch --json '{"url": "https://example.com/news"}'
itero args ask-ai --json '{"prompt": "Summarize this page in 5 bullets."}'
itero args send-mail --json '{"host": "smtp.example.com", "user": "me@example.com", "to": "me@example.com"}'
itero secret ask-ai api_key
itero secret send-mail password
```

`fetch` prints `{"page": …}`, `ask-ai` sends `$prompt` and `$page` to the
assistant's API with `curl` and prints `{"text": …}`, and `send-mail` sends
`$text` with `curl smtps://…`.

## What goes away

From the files people write:
- `name`, `description`, `version`, and `action.json` altogether;
- `states`, `start`, `end`, `transitions`, `from`, `to`, `do`, `id`;
- `${...}` expressions, `when`, `expect`;
- `memory.set`, `concurrency`, `pause_after`, `limits`;
- workspace `triggers/`;
- the action types `python@`, `bun@` and `container` (a shebang line covers
  languages; a script can run `docker` itself);
- built-in actions other than `notify`;
- `match`, `publish` and `"from": "*"`.

They stay in git history, to come back only when a real workflow needs
one.

## Open questions

- **Waiting forever.** Should a run that waits (exit 75) give up after some
  time, by default?
- **One message, several waiting runs.** If a message matches more than
  one waiting run, does each get it, or only the oldest?
- **Memory with a list of args.** One `$ITERO_MEMORY` per item (per
  account), or one shared by all?
