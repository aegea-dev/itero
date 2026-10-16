---
title: Workspaces, workflows and runs
description: The few things itero is made of, and how a run moves.
---

## A workspace

A folder with an `itero.toml` is a workspace. One daemon runs one
workspace; keep work and home apart as two workspaces, each with its own
daemon and port. itero finds the workspace from the folder it runs in: the
nearest folder upwards with an `itero.toml`.

```
itero.toml               port, and where workflows and actions are
workflows/<name>.json    workflows
actions/<name>/          your own actions: action.json and a program
triggers/<name>.json     this workspace's triggers for a workflow
sources/<name>.json      sources: what tells itero that something happened
.itero/journal.sqlite    every event, run and job, and each workflow's memory
.itero/daemon.log        the service's log
```

`.itero/` is local to the machine and never synced.

## A workflow

A workflow is a template: its **states**, where a run **starts**, the states
where it **ends**, and the **transitions** between states. A transition can
run a chain of **actions** (`do`), one after the other; the output of one is
`prev` for the next, and one with an `id` is `steps.<id>` for the rest of
the chain.

A workflow's **triggers** say when a run starts: a cron **schedule**, or
**on** an app's event.

## A run

A run is one short instance of a workflow. It starts in the `start` state,
moves along its transitions, and ends in an `end` state or in `failed`. A
recurring job is many runs: the mail check is one run every 5 minutes, not
one run that waits.

A transition without `on` is taken as soon as the run enters its `from`
state (and its `when` holds). A transition with `on` waits for an event of
that type sent to the run. A transition with a `timeout` moves to its
`on_timeout` state when nothing else happened in time.

When an action fails, the run goes to the transition's `on_fail` state, or
to `failed`.

## Memory

Each workflow has a small store in the journal, keys and JSON values, for
its own bookkeeping ("seen up to", "sent this year"). Runs read it as
`memory`; the built-in `memory.set@1` changes it, saved only when the chain
succeeds. An app's own records belong in the app, not here.

## The pieces

```
  app or person --events-->  +--------------------+
  (HTTP API, itero emit)     |      daemon        |  saves every event and
                             | (one per workspace)|  answer in the journal
  schedules (cron) --------> +--+--------------+--+  decides nothing
                                |              |
                       +--------v---+  +-------v------+
                       |  manager   |  |    worker    |  one process per
                       | pure: JSON |  | runs one     |  action, any
                       | in and out |  | action, exits|  language
                       +------------+  +--------------+
```

- **The daemon** saves events, fires schedules and timers, asks the manager
  what to do, saves each answer in one transaction, and runs jobs in
  workers. With no workflow it does nothing.
- **The manager** decides what a run does next. It is pure: the same input
  always gives the same answer.
- **A worker** runs one action in its own process and exits.

Every job carries an idempotency key, the same when it runs again. A job
that is lost (killed, its lease ran out, the daemon stopped) runs again, up
to 3 times; an error fails the action at once.
