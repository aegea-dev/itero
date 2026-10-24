---
title: The log, IDs and names
description: What happened, line by line; the IDs and names itero gives; itero info and itero runs.
---

## The log

`.itero/itero.log` holds everything the daemon does, one JSON object per
line:

```json
{"time":"2026-10-21T13:43:24Z","workspace":"01a0f3…","daemon":"01a15f2c…","event":"run.started","workflow":"order","run":"01a15eb3…","run_name":"misty-lake","message":"01a15eb2…"}
{"time":"2026-10-21T13:43:25Z","workspace":"01a0f3…","daemon":"01a15f2c…","event":"step.waiting","workflow":"order","run":"01a15eb3…","run_name":"misty-lake","step":"#check-payment","output":{"payment_ref":"pay-1"}}
```

Every line has `time`, `workspace`, `daemon` and `event`. A line about a
run has `workflow`, `run` and `run_name`, and `"internal": true` when its
workflow is [internal](/guides/maintenance/).

| `event` | Also |
|---|---|
| `daemon.started` | every field of the workspace and the daemon |
| `daemon.stopped` | |
| `message` | `message`, `type`, `data`, and what it did: `started` and `reached` (runs), or `nothing` |
| `run.started` | `message`, or `schedule` |
| `step.started` | `step` |
| `step.done`, `step.waiting` | `step`, `output` |
| `step.failed` | `step`, `why` |
| `step.skipped` | `step`, `why` (its `on` did not hold) |
| `run.note` | `note`: what the run did with a message or a timer |
| `run.ended` | `status` (`done`, `failed`, `error`, `split`), `why` |
| `run.skipped` | a schedule came due and the workflow's `on` did not hold |
| `connector.started`, `connector.ended`, `connector.gave-up` | `connector`, `why` |
| `paths.watched` | `paths`: every path watched now |
| `reply.sent`, `reply.failed` | `url`, `run`, and for `reply.failed` `why`: a callback taken, or to be tried again |
| `error` | `why`: what went wrong in the daemon itself |

It holds what steps printed, never their environment, so no secret.
Reading it is `jq`'s job:

```sh
jq 'select(.run_name == "misty-lake")' .itero/itero.log      # one run
jq 'select(.event == "step.failed")' .itero/itero.log        # what broke
jq -r '"\(.time) \(.event) \(.workflow // "") \(.step // "")"' .itero/itero.log
tail -f .itero/itero.log | jq .                             # live
```

Past its `log` limit (10 MB) the log is rolled: `itero.log` becomes
`itero.log.1`, and so on, keeping `logs` of them (3). See
[The daemon's own workflows](/guides/maintenance/).

## IDs and names

itero gives each of these an identity nobody edits, kept in the journal:

| | `id` | `name` | also |
|---|---|---|---|
| the workspace | a UUIDv7, made with its journal | two random words: `quiet-river` | when it was made, its folder, the itero that made it |
| a daemon's life | a UUIDv7, at each start | two random words | when it started, its pid, host and itero version |
| a workflow | a UUIDv7, the first time its file is seen | its file name | its runs so far, its last run |
| a run | a UUIDv7, at its start | two random words: `misty-lake` | its workflow and status |

A name is to say aloud and type; the id stays the identity, and two with
one name are told apart by it. A workflow keeps its id while its file
name stays, through any edit. Deleting `.itero/` starts a new history,
with a new workspace id.

## Seeing them

```sh
itero workflows            # each workflow: what starts it, its runs, the last; the daemon
itero info                 # the workspace: id, name, what it is, limits; and its daemon
itero runs                 # every run: id, name, workflow, status, started, ended
itero runs misty-lake      # one run, by name or id, step by step
itero check                # each workflow: its id, meta, runs so far, last run
```

`itero runs <run>` tells a run's story from the journal: how it started,
each step and how it went, the messages it took, where it waits, and its
state at the end. Of two runs with one name it shows the newest, and says
so.
