---
title: itero daemon
description: Runs the workspace, each thing as it comes due.
---

```sh
itero daemon [--no-serve]
```

Runs the workspace: starts its [connectors](/guides/messages/#connectors),
watches the [paths](/guides/messages/#watching-files) its workflows name,
takes messages and schedules, and runs steps. It serves the
[HTTP API](/reference/http/) on `127.0.0.1:<port>`. Leave it running;
Ctrl-C stops it.

It works in passes, and makes one only when something wakes it:

- a message: `POST /events`, `itero emit`, a connector, a click on a
  notification;
- a watched path that changed, or an edit to a workflow or
  `itero.config.json`;
- the next due time: a schedule's next slot, a waiting run's deadline or
  retry, a reply to send again. It is set in the kernel on the wall clock,
  so a time that passes while the computer sleeps comes due as soon as it
  wakes.

In between it waits and uses no CPU.

| Option | Does |
|---|---|
| `--no-serve` | the passes only, no HTTP API |

```
$ itero daemon
itero: running the workflows in /Users/me/home, each as it comes due (Ctrl-C stops); log: .itero/itero.log
itero: serving on http://127.0.0.1:8787/
3 events, 1 jobs, 0 lost (run again), 0 waiting
```

In a terminal it prints a line for each pass that did something. What it
does goes to `.itero/itero.log`, one JSON line each
([The log](/guides/log/)).

One daemon per workspace. To have it running from login on, without a
terminal: [`itero service install`](/commands/service/).
