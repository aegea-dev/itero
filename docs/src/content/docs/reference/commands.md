---
title: Commands
description: Every itero command and its options.
---

Every command works in the workspace of the folder it runs in: the nearest
folder upwards with an `itero.toml`. `itero help <command>` shows a
command's options.

## itero daemon

Runs the workspace: starts its [sources](/guides/messages/#sources), takes
messages and schedules, runs steps, pass after pass, and serves the
[HTTP API](/reference/http/) on `127.0.0.1:<port>`. Ctrl-C stops it.

| Option | Does |
|---|---|
| `--interval N` | seconds between passes (1) |
| `--no-serve` | the passes only, no HTTP API |
| `--once` | one pass, then stop (sources do not run) |
| `--now TIME` | with `--once`: the time to run at, RFC 3339 (for tests) |

It prints a line for each pass that did something.

## itero emit

```sh
itero emit <type> ['<data>']
```

Sends a message: `itero emit order.placed '{"order": 43}'`. The type is two
or more words joined by dots; the data, a JSON object. It prints the
message's id.

## itero args

```sh
itero args <action> '<data>'
itero args <action> --file <path>
```

Writes `args/<action>.json`: an object, or a list of objects (the step
runs once per item). See [Args and secrets](/guides/secrets/).

## itero secret

```sh
itero secret <NAME>
itero secret <NAME> --file <path>
```

Keeps a secret in the OS secret store, for `$$NAME`. Without `--file` it
asks for it, without echo; the value is never an argument.

## itero check

Reads every workflow, action and source, and prints each workflow with
the actions and workflows its steps name, and each source. Anything wrong
is an error naming the file: a key it does not know, a message type of one
word, an action or a workflow that is not there.

## itero runs

The runs, newest first: id, workflow, status (`running`, `waiting`,
`done`, `failed`, `error`, `split`), started, ended.

## itero errors

The runs that ended in error, newest first: when, the workflow, the run,
the step and why. `--limit N` (20).

## itero watch

```sh
itero watch <path>
```

For sources: prints a JSON line each time the file or folder changes. It
ends when its stdin closes.

## itero service

`itero service install` runs the workspace's daemon at login, kept up by a
watchdog; `status` shows it and the daemon's health; `uninstall` removes
it. See [Start at login](/guides/service/).
