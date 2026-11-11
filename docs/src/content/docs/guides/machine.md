---
title: One itero per machine
description: One daemon runs every workspace on the machine's list, on one port.
---

One `itero daemon` runs every workspace on this machine's list:
each with its own journal, log, connectors and schedules, all served on
one port. Add a workspace once, and it runs.

```sh
cd ~/projects/home && itero workspace add
cd ~/projects/mail && itero workspace add
itero daemon
```

## The list

[`itero workspace add`](/commands/workspace/), in a workspace's folder,
puts it on the machine's list; `itero workspace remove` takes it off, and
its folder stays as it is. Nothing runs that is not on the list.

A workspace on the list is named by its journal's two words, as runs
are (`quiet-river`); `itero ws info` shows it. Two workspaces on one
machine never share a name: `add` gives the newer one another, and says
so.

The list is kept in `~/.local/state/itero/workspaces.json`
(`$XDG_STATE_HOME/itero/` when that is set). A folder that moves is
added again from its new place: its journal is the same, so its entry
moves with it.

## What runs

`itero daemon` starts each listed workspace, and follows the list
while it runs: a workspace added is started at once, one removed is
stopped, and the others run on. Each workspace has its own threads; its
steps and connectors run as processes of their own, as with
`itero ws daemon`, which runs one workspace alone.

The daemon also keeps what the workspaces share:

- **one [itero-ai](/ai/)**, so a model is in memory once; its events go
  to every workspace, `itero.ai.model.missing` to the one that asked;
- **one look for models**, at its start, every hour, and when a step
  names a model the list has not, in place of each workspace's `models`
  workflow.

`itero workspace list` shows each one:

| State | Means |
|---|---|
| `running` | the daemon runs it |
| `stuck` | a pass has run past its limit (ten minutes); the others run on |
| `failed` | its loop kept failing, and was given up on |
| `stopped` | no daemon runs it: none is running, or another holds it |
| `missing` | its folder is gone, or holds another workspace |

A workspace that gets stuck or fails is told to every workspace on the
list, as the events `itero.workspace.stuck` and `itero.workspace.failed`
([The daemon's own workflows](/guides/maintenance/)), so a workflow can
send a notification about it.

## One daemon per workspace

Whichever daemon runs a workspace holds a lock on its
`.itero/daemon.lock`. A second one is refused, naming the first: an
`itero ws daemon` for a workspace on the list while `itero daemon` runs
it, or the other way around (`itero daemon` then shows it as `stopped`,
with why).

A workspace `stopped` because another daemon held it is started when the
list changes or the machine's daemon starts again
(`itero service restart`).

[`itero service install`](/commands/service/) runs the machine's daemon
from login on, and removes the services from before, one per workspace.
The service follows an upgrade by itself: its watchdog runs the new itero
([The watchdog](/guides/service/#the-watchdog)), so a workspace whose
journal a newer itero made is not left `missing`.

## One API

The daemon serves one port: the `port` in
[`daemon.json`](/reference/daemon-json/) when it gives one, else the one
it kept from its last start (in `~/.local/state/itero/port`):

| Route | Does |
|---|---|
| `GET /health` | the daemon, each workspace's passes and connectors, itero-ai |
| `GET /workspaces` | the list, with each one's state |
| `POST /events` | an event for every workspace on the list |
| `/ws/<name>/...` | that workspace's own routes: `/ws/quiet-river/events`, `/ws/quiet-river/runs` |

See the [HTTP API](/reference/http/#one-itero-per-machine). A `ws` command
finds the daemon running its workspace, whichever it is;
`itero ws info` shows its address. [`itero info`](/commands/info/), from any
folder, shows the machine's daemon: the itero it runs, and each workspace.

## From another machine

A [context](/commands/context/) names a machine's daemon by its address,
so the same `itero ws` commands reach it from elsewhere:

```sh
itero context add office http://127.0.0.1:8787 --ws quiet-river
itero ws wf runs --context office
```

The daemon's API answers on `127.0.0.1` only, unless its
[`daemon.json`](/reference/daemon-json/#opening-the-api) opens it. On the
server, make a token and open the API:

```sh
itero token create laptop   # prints the token once
```

add `"listen": "0.0.0.0:8443"` to `~/.config/itero/daemon.json`, and
`itero service restart`;

with a `tls` certificate beside it, or behind a proxy that adds TLS; then,
on your machine, `itero context add office https://office.example.com:8443
--ws quiet-river` with that token.

## Deploying a workspace

Made and tried here, a workspace goes to a server with one command, its
folder kept the same there:

```sh
itero context add prod https://prod.example.com:8443 --ws shop
itero ws apply --context prod
```

The first apply makes it on the server; each one after makes the
server's copy match this folder. See [`itero ws apply`](/commands/workspace/#apply).

