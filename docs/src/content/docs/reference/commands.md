---
title: Commands
description: Every itero command and its options.
---

Every command works in the workspace of the folder it runs in: the nearest
folder upwards with an `itero.toml`. `itero help <command>` shows a
command's options.

## itero daemon

Runs the workspace: starts its [sources](/guides/messages/#sources),
watches the [paths](/guides/messages/#watching-files) its workflows name,
takes messages and schedules, runs steps, pass after pass, and serves the
[HTTP API](/reference/http/) on `127.0.0.1:<port>`. Ctrl-C stops it.
What it does goes to `.itero/itero.log` ([The log](/guides/log/)).

| Option | Does |
|---|---|
| `--interval N` | seconds between passes (1) |
| `--no-serve` | the passes only, no HTTP API |

In a terminal, it prints a line for each pass that did something.

## itero emit

```sh
itero emit <type> ['<data>']
```

Sends a message: `itero emit order.placed '{"order": 43}'`. The type is two
or more words joined by dots; the data, a JSON object. It prints the
message's id, then on stderr what the message will do: the workflows it
starts, the runs it may reach, or that nothing starts on its type with the
nearest type that does; and that no daemon is running, when none answers
on the port.

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
its id, its `meta`, its runs so far and its last run, the actions and
workflows its steps name, and each source. Internal workflows come last,
apart. Anything wrong is an error naming the file: a key it does not
know, a message type of one word, an action or a workflow that is not
there.

## itero workflows

Each workflow on a line: what starts it, its runs so far, how the last
one went and when, and whether it is `off` or internal. The last line
says whether this workspace's daemon is running, and how many messages
wait for one:

```
$ itero workflows
page-digest   on website.requested   runs 0   last: none
order         on order.placed        runs 3   last: done 2m ago
keep          at 0 * * * *           runs 1   last: done 40m ago   internal
daemon: not running, 1 message waiting
```

## itero info

The workspace: its id, name, when it was made, its folder, the itero that
made it, its `[meta]` and its limits; and the daemon running it, when one
answers on the port: its id, name, start, pid, host and itero version.

## itero runs

```sh
itero runs [--all]
itero runs <id or name>
```

The runs, newest first: id, name, workflow, status (`running`, `waiting`,
`done`, `failed`, `error`, `split`), started, ended. Internal workflows'
runs only with `--all`.

With a run's id or name, its story: how it started, each step and how it
went, the messages it took, how it ended, and its state.

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
