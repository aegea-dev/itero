---
title: Commands
description: Every itero command and its options.
---

Every command works in the workspace of the folder it runs in: the nearest
folder upwards with an `itero.toml`. `itero help <command>` shows a
command's options.

## itero daemon

Runs the workspace's workflows: their schedules, events and jobs, pass
after pass, and serves the [HTTP API](/reference/http/) on
`127.0.0.1:<port>`. Ctrl-C stops it.

| Option | Does |
|---|---|
| `--interval N` | seconds between passes (1) |
| `--no-serve` | the passes only, no HTTP API |
| `--once` | one pass, then stop |
| `--now TIME` | with `--once`: the time to run at, RFC 3339 (for tests) |

It prints a line for each pass that did something: events delivered, jobs
run, jobs lost (run again), and events waiting.

## itero check

Reads every workflow and action, and prints each with its version:

```
mail-check  e734495fe858
mail.check@1
```

The first problem found is an error naming the file and the transition. A
folder `itero.toml` lists that is not there is noted.

## itero emit

```bash
itero emit <type> [--data JSON] [--run ID]
```

Sends an event: to a run with `--run`, or to the workspace, where it may
start runs. `--data` is a JSON object. Prints the event's id. It writes the
journal directly, so it works while the daemon is stopped. See
[Apps and events](/guides/apps/).

## itero runs

The runs, newest first: id, workflow, state, started, ended.

## itero memory

```bash
itero memory <workflow>
```

A workflow's memory, as JSON.

## itero service

| Command | Does |
|---|---|
| `itero service install [--interval N]` | runs the daemon as a per-user service at login, kept up by a watchdog |
| `itero service status` | the service's state and the daemon's health |
| `itero service uninstall` | stops the service and removes it |

See [Start at login](/guides/service/).
