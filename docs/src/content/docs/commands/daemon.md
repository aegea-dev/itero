---
title: daemon
description: Runs your workflows; leave it running.
---

`itero daemon` runs your workflows: every workspace on the machine's
list, each on its own, on one port. While it runs, events start them,
schedules come due and steps run; leave it running in a terminal, or use
[`itero service`](/commands/service/) to start it at login.

```sh
itero daemon [--config <path>] [--no-serve]
itero ws daemon [--no-serve]
```

A workspace is on the list once [`itero ws add`](/commands/workspace/)
put it there: see [One itero per machine](/guides/machine/). The daemon's
settings, its port among them, are in
[`daemon.json`](/reference/daemon-json/). Run in a workspace's folder that
is not on the list, it says so when it starts.

`itero ws daemon` runs one workspace alone, in the foreground: the
folder's, or the one named after `ws` (`itero ws quiet-river daemon`).
It is for a workspace you are making, before it goes on the list; its
port is the one in its `itero.config.json`, else one the OS gives.

Each workspace's daemon starts the workspace's [connectors](/guides/connectors/), watches the
[paths](/guides/connectors/#watching-files) its workflows name, takes
events and schedules, and runs steps. It serves the
[HTTP API](/reference/http/) on `127.0.0.1:<port>`. Leave it running;
Ctrl-C stops it.

It works in passes, and makes one only when something wakes it:

- an event: `POST /events`, `itero ws emit`, a connector, a click on a
  notification;
- a watched path that changed, or an edit to a workflow or
  `itero.config.json`;
- the next due time: a schedule's next slot, a waiting run's deadline or
  retry, a reply to send again. It is set in the kernel on the wall clock,
  so a time that passes while the computer sleeps comes due as soon as it
  wakes.

A pass lasts while the steps it started run. Meanwhile it still takes the
events that come, at once, and the times that come due, so an event a waiting step
needs can end a [race](/guides/workflows/#how-steps-side-by-side-wait)
while a slow step still runs. Between passes it waits and uses no CPU.

| Option | Does |
|---|---|
| `--config <path>` | its settings from this file, not the config folder's [`daemon.json`](/reference/daemon-json/) |
| `--no-serve` | the passes only, no HTTP API |

`itero daemon --all`, from before, is refused and says the new form.

```
$ itero ws daemon
itero: running the workflows in /Users/me/home, each as it comes due (Ctrl-C stops); log: .itero/itero.log
itero: serving on http://127.0.0.1:8787/
3 events, 1 jobs, 0 lost (run again), 0 stopped, 0 waiting
```

In a terminal it prints a line for each pass that did something. What it
does goes to `.itero/itero.log`, one JSON line each
([The log](/guides/log/)).

One daemon per workspace: whichever runs it holds a lock on its
`.itero/daemon.lock`, and a second one is refused, naming the first. To
have your workspaces running from login on, without a terminal:
`itero workspace add` in each, then
[`itero service install`](/commands/service/), which runs
`itero daemon`.
