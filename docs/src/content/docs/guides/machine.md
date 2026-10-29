---
title: One itero per machine
description: One daemon runs every workspace on the machine's list, on one port.
---

One `itero daemon --all` runs every workspace on this machine's list:
each with its own journal, log, connectors and schedules, all served on
one port. Add a workspace once, and it runs.

```sh
cd ~/projects/home && itero workspace add
cd ~/projects/mail && itero workspace add
itero daemon --all
```

## The list

[`itero workspace add`](/commands/workspace/), in a workspace's folder,
puts it on the machine's list; `itero workspace remove` takes it off, and
its folder stays as it is. Nothing runs that is not on the list.

A workspace on the list is named by its journal's two words, as runs
are (`quiet-river`); `itero info` shows it. Two workspaces on one
machine never share a name: `add` gives the newer one another, and says
so.

The list is kept in `~/.local/state/itero/workspaces.json`
(`$XDG_STATE_HOME/itero/` when that is set). A folder that moves is
added again from its new place: its journal is the same, so its entry
moves with it.

## What runs

`itero daemon --all` starts each listed workspace, and follows the list
while it runs: a workspace added is started at once, one removed is
stopped, and the others run on. Each workspace has its own threads; its
steps and connectors run as processes of their own, as with
`itero daemon` in a folder.

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
`itero daemon` in a folder on the list while `--all` runs it, or the
other way around (`--all` then shows it as `stopped`, with why).

A workspace `stopped` because another daemon held it is started when the
list changes or the machine's daemon starts again
(`itero service restart`).

[`itero service install`](/commands/service/) runs the machine's daemon
from login on, and moves the services from before, one per workspace,
over to the list.

## One API

The daemon serves one port, `--port` when given, else the one it kept
from its last start (in `~/.local/state/itero/port`):

| Route | Does |
|---|---|
| `GET /health` | the daemon, each workspace's passes and connectors, itero-ai |
| `GET /workspaces` | the list, with each one's state |
| `/w/<name>/...` | that workspace's own routes: `/w/quiet-river/events`, `/w/quiet-river/runs` |

See the [HTTP API](/reference/http/#one-itero-per-machine). Commands run
in a workspace's folder find the daemon running it, whichever it is;
`itero info` shows its address.
