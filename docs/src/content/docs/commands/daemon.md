---
title: itero daemon
description: Runs the workspace, pass after pass.
---

```sh
itero daemon [--interval N] [--no-serve]
```

Runs the workspace: starts its [connectors](/guides/messages/#connectors),
watches the [paths](/guides/messages/#watching-files) its workflows name,
takes messages and schedules, and runs steps, pass after pass. It serves
the [HTTP API](/reference/http/) on `127.0.0.1:<port>`. Leave it running;
Ctrl-C stops it.

| Option | Does |
|---|---|
| `--interval N` | seconds between passes (1); a message from `POST /events`, a connector or a watched path wakes it at once |
| `--no-serve` | the passes only, no HTTP API |

```
$ itero daemon
itero: running the workflows in /Users/me/home, a pass every 1s (Ctrl-C stops); log: .itero/itero.log
itero: serving on http://127.0.0.1:8787/
3 events, 1 jobs, 0 lost (run again), 0 waiting
```

In a terminal it prints a line for each pass that did something. What it
does goes to `.itero/itero.log`, one JSON line each
([The log](/guides/log/)).

One daemon per workspace. To have it running from login on, without a
terminal: [`itero service install`](/commands/service/).
