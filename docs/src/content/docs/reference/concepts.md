---
title: Concepts
description: The six parts itero is made of, each in a few lines.
---

itero has six main parts. Without any of them, itero cannot run:

- [the daemon](#daemon)
- [the journal](#journal)
- [the workspace](#workspace)
- [the workflow](#workflow-)
- [the action](#action-)
- [the connector](#connector-)

Each has parts of its own, listed under it.

## daemon

- One daemon per machine (`itero daemon --all`): it runs every workspace
  on the machine's list ([One itero per machine](/guides/machine/)).
- Starts what it needs: a worker per step, the connectors, itero-ai.
- A watchdog (its own process, run by the service) keeps it running: it
  restarts the daemon when it exits or stops answering.
- One API on one port: `/health`, `/workspaces`, `/ws/<name>/...`
  ([HTTP API](/reference/http/)).
- Runs workflows.
- Logs everything: each workspace's `.itero/itero.log`, and its own
  `~/.local/state/itero/daemon.log` ([The log, IDs and names](/guides/log/)).

## journal

- One SQLite file per workspace: `.itero/journal.sqlite`, local, never
  synced.
- The message queue: every event that comes in is kept here, and runs
  start from it.
- Keeps each run's place (state, step, waiting), so a run survives a
  restart.
- Pins each run to the workflow version it started with.
- Holds timers and schedules.
- Gives the workspace its name: two random words (`pale-otter`), unique
  on the machine.
- Forgets on its own: ended runs and their events after `keep` (a day).

## workspace

- A folder with `itero.config.json`: the one manifest (what it is, how
  it runs, an entry per workflow, action and connector), like
  `package.json`, never values to work on
  ([itero.config.json](/reference/itero-config/)).
- Holds `workflows/`, `actions/`, `connectors/`.
- `.env`: the user's own values (`NAME=value`), read as `$$NAME`; secrets
  stay in the OS secret store ([Args and secrets](/guides/secrets/)).
- Its own state in `.itero/`: journal, log (`itero.log`), port, memory,
  lock.
- Run by one daemon at a time (a lock on `.itero/daemon.lock`).
- Lives anywhere on disk; `itero ws add` puts it on the machine's list.

## workflow ●

- A JSON file: `workflows/<name>.json` ([Workflow files](/reference/workflow-file/)).
- `on`: what starts it (an event, a schedule, a path) and conditions
  (`has`, `$event.x` values).
- `steps`: actions (`#name`), built-ins (`#notify`, `#ai`), other
  workflows.
- Steps side by side: a list waits for all; `race`, `any`, `some`,
  `settled`, `fail` say how else.
- Values by where they come from: `$event` (what started the run),
  `$output` (the step before), `$steps.<name>` (any earlier step, by its
  name or `as`), `$config` (the manifest), `$$NAME` (a secret, `.env`, the
  environment).
- Can wait for an event (a step exits 75), with a `timeout`.
- `undo` for steps that finished (with args when needed); `success`,
  `failure`, `error` lists at the end; the `error` list starts with
  `$output.why`.
- Each run has an id and a two-word name: `itero ws wf runs`.

See [Workflows](/guides/workflows/).

## action ■

- An executable file in `actions/`, any language (`#!/bin/sh`); its name
  is its file name.
- Gets only its step's args: as environment variables (`$order`), and on
  stdin as `{"args": {...}}`.
- A waiting action gets the event that matched it as `$event`.
- Secrets and `.env` values as `$$NAME` in its args.
- Prints its result: JSON or plain text, the next step's `$output`.
- Exit code: `0` done, `75` not yet (the run waits), anything else an
  error (stderr says why).
- `$ITERO_MEMORY`: a file kept between runs.
- Time limits: quiet 10 minutes, at most a day; run again up to 3 times
  when lost.
- Can send an event itself: `itero ws emit`.

See [Actions](/guides/actions/).

## connector ▲

- An executable file in `connectors/`, named after the event it sends
  (`mail.received`: two or more words joined by `.`).
- Keeps running: the daemon starts it, and starts it again when it ends
  (giving up on one that keeps ending).
- Each line it prints is one event: a JSON object.
- Its stdin closes when the daemon goes: then it stops.
- Its args in `itero.config.json`; its stderr goes to the daemon's log.
- `itero watch <path>` for one that watches files.

See [Connectors](/guides/connectors/).

## Addresses

Any part of a workspace is named by one address, from the workspace's
name down, by kind, then the workflow file's own keys. `itero ws check`
says where each problem is with one.

| Address | Names |
|---|---|
| `home` | the workspace |
| `home/workflow/posta` | a workflow, and its entry in `itero.config.json` |
| `home/workflow/posta/on/2` | the second entry of its `on` |
| `home/workflow/posta/steps/#ai` | a step running the action `ai` |
| `home/workflow/posta/steps/#ai/2` | the second `#ai` step in `steps` |
| `home/workflow/posta/steps/daily` | a step running the workflow `daily` |
| `home/workflow/posta/steps/all/#check-stock` | a step side by side, in a plain list or an `all` group |
| `home/workflow/posta/steps/race/2/#a` | in the second `race` group of `steps` |
| `home/workflow/posta/steps/#ship/undo` | a step's undo |
| `home/workflow/posta/error/#notify` | a step in `success`, `failure` or `error` |
| `home/action/check-stock` | an action (`check-stock@2` pinned, `ai` built in) |
| `home/connector/mail.received` | a connector, by the event it sends |

A step is named by its action (with `#`) or its workflow, as the step
writes it. When one is in its list more than once, `/1`, `/2` tell them
apart; `#ai` and `#ai/1` are the same step, so a step keeps its address
when a second one is added. A workflow used as a step has its own
address: `home/workflow/daily/...`.
