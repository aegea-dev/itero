---
title: The daemon's own workflows
description: The events the daemon sends about itself, internal workflows, the built-in ones, measuring the machine and itero, and the workspace's limits.
---

The daemon looks after itself with the same parts as everything else:
events about what it notices, and workflows that handle them. They show
in `itero ws check`, `itero ws wf runs --all` and the log like any other, and a
workspace changes or turns them off the same way.

## Events from the daemon

Their types start with `itero.`, which only itero may send (a click on a
notification comes through Itero.app, a webhook through `itero webhook`):

| Type | When | Data |
|---|---|---|
| `itero.daemon.started` | the daemon starts | `daemon`, `name`, `itero` |
| `itero.run.error` | a run ended in error | `workflow`, `run`, `failed_step`, `why` |
| `itero.run.failed` | a run ended as failed | `workflow`, `run`, `why` |
| `itero.run.done` | a run ended done (not one started by an `itero.run.` event) | `workflow`, `run`, `output` (its last result) |
| `itero.connector.gave-up` | a connector kept ending and was given up on | `connector`, `why` |
| `itero.space.full` | `.itero/` is past its `space` limit | `used`, `limit` |
| `itero.log.full` | the log is past its `log` limit | `path`, `size`, `limit` |
| `itero.notification.clicked` | a notification from `#notify` was clicked, on macOS ([`#notify`](/guides/actions/#built-in)) | `notification`, `answer`, `workflow`, `data` |
| `itero.webhook.received` | a webhook passed its check; starts workflows, reaches no waiting step ([Webhooks](/guides/webhooks/)) | `path`, `headers`, `body` |
| `itero.ai.started` | itero-ai, the local model's server, started for an `#ai` step ([Running and troubleshooting](/ai/running/)) | `socket`, `seconds` |
| `itero.ai.start.failed` | a start of itero-ai never answered | `why`, `seconds` |
| `itero.ai.stopped` | it exited after its idle time | `after` |
| `itero.ai.failed` | it ended any other way | `exit`, `why` |
| `itero.ai.model.loaded` | it loaded a model | `model`, `seconds` |
| `itero.ai.model.missing` | an `#ai` step named a model this machine does not have | `model`, `workflow` |
| `itero.workspace.stuck` | on the machine's daemon, a workspace's pass runs past its limit; told to every listed workspace ([One itero per machine](/guides/machine/)) | `workspace`, `since` |
| `itero.workspace.failed` | on the machine's daemon, a workspace's loop kept failing and was given up on; told to every listed workspace | `workspace`, `why` |

A workflow on one is written like any other:

```json
{ "on": [{ "event": "itero.run.error" }],
  "steps": [{ "#notify": { "args": { "title": "itero", "text": "$event.workflow: $event.why" } } }] }
```

An event about a run never starts its own workflow, so a broken workflow
cannot set itself off again and again. A run that an `itero.run.` event
started sends no `itero.run.done`: two workflows that watch every run's
end would otherwise start each other forever. To start a workflow when
another one is done, name it in the event's condition:

```json
{ "on": [{ "event": "itero.run.done" }, { "$event.workflow": "nightly-backup" }],
  "steps": ["#upload-report"] }
```

No run waits for these events: they only start workflows.

## Internal workflows

A workflow whose entry in `itero.config.json` says `"internal": true` looks after the workspace
rather than doing its work. Nothing in its name says so. Being internal:

- `itero ws check` lists it apart, last;
- `itero ws wf runs` and `GET /runs` leave its runs out unless asked
  (`itero ws wf runs --all`, `?all=true`); `itero ws ac errors` counts its
  errors;
- its runs do not count against `runs`, nor it against `workflows`;
- its lines in the log carry `"internal": true`.

## Built-in workflows

itero carries five internal workflows and runs them as if they were in
`workflows/`:

| Workflow | On | Does |
|---|---|---|
| `keep` | every hour, and `itero.space.full` | `#forget`: forgets ended runs and their events past `keep`; all ended ones while space is short |
| `load` | every minute, once its entry turns it on: `"workflow": {"load": true}` | `#load`: measures the machine and itero ([below](#measuring-the-machine-and-itero)) |
| `load-record` | `itero.run.done` of `load` | `#record-load`: keeps each result of `load` in a file of the day |
| `logs` | `itero.log.full` | `#roll-log`: `itero.log` becomes `itero.log.1`, `.1` becomes `.2`, keeping `logs` of them |
| `models` | the daemon's start, `itero.ai.model.missing`, and every hour | `#find-models`: the models in LM Studio's folder, Hugging Face's cache and `~/.cache/itero/models`, into the list `#ai` steps pick from ([`itero ai`](/commands/ai/)); it reads only new or changed files, and does nothing without itero-ai. A workspace on the machine's list has none: the machine's daemon looks once for all ([One itero per machine](/guides/machine/)) |

`#forget`, `#roll-log`, `#find-models`, `#load` and `#record-load` are
built-in actions, like `#notify`.

- **Changing one:** a file of its name in `workflows/` is used instead,
  such as `workflows/logs.json` rolling the log and then sending the
  oldest away:

  ```json
  { "on": [{ "event": "itero.log.full" }],
    "steps": ["#roll-log", "#upload-old-logs"] }
  ```

  with `"workflow": {"logs": {"internal": true}}` in `itero.config.json`,
  so it stays internal.

- **Turning one off:** its entry, `"workflow": {"logs": {"off": true}}`.
  `load` is off until its entry is `true` (or says `"off": false`).

## Measuring the machine and itero

`load` measures the machine and itero every minute, once it is on:

```json
{ "workflow": { "load": true } }
```

Its result is `#load`'s: the OS, the CPUs and their load, memory, the
workspace's disk, the daemon and its workers, the queue, the last
minute's runs and the journal's size ([`#load`](/guides/actions/#load)).

### The files

`load-record` keeps each result as a line of `.itero/load/<UTC date>.jsonl`.
A day's file is gzipped once the day is over (`.jsonl.gz`), and a file
whose day ended more than `limits.history` ago (30 days unless set) is
deleted. The files count toward `space`. Read them with `jq`, an old day
through `zcat`:

```
$ jq -c '{at, used: .memory.used, load: .cpu.load[0]}' .itero/load/2026-11-26.jsonl
$ zcat .itero/load/2026-11-25.jsonl.gz | jq -c .runs
```

`"workflow": {"load-record": false}` keeps no files; each result is still
its run's output, and a workflow of your own can take it on
`itero.run.done`, as `load-record` does.

### How often, and what

A workspace's own `workflows/load.json` is used instead of the built-in:
its own schedule, only the parts it needs, steps of its own after it.

```json
{ "on": [{ "schedule": "*/5 * * * *" }],
  "steps": [{ "#load": { "args": { "measure": ["memory", "disk", "queue"] } } }] }
```

with `"workflow": {"load": {"internal": true}}`, so it stays internal.
`load-record` keeps its results as it does the built-in's.

### In a step

`#load` is a step like any other, in any workflow: it measures at once.
Twice in a workflow, each is told by its number
([`$steps`](/guides/workflows/#steps-every-step-by-its-name)):

```json
{ "steps": [
    { "#load": { "args": { "measure": ["memory"] } } },
    "#resize-photos",
    { "#load": { "args": { "measure": ["memory"] } } },
    { "#report": { "args": { "before": "$steps.load/1.memory.used",
                             "after":  "$steps.load/2.memory.used" } } } ] }
```

Only the `load` workflow's results go to the files: a `#load` step
elsewhere is its run's.

### By the machine

`os.name` is `macos` or `linux` and `os.arch` `aarch64` or `x86_64`, so a
step can run on one kind of machine alone:

```json
{ "steps": [
    { "#load": { "args": { "measure": ["os"] } } },
    { "#build-metal": { "on": [{ "$steps.load.os.name": "macos" }] } },
    { "#build-cuda":  { "on": [{ "$steps.load.os.name": "linux" }] } } ] }
```

### What it costs

`load` and `load-record` are two internal runs a minute, each one step,
forgotten by `keep` after `limits.keep`. `#load` reads only what it is
asked for and never waits: each result says what measuring took, in
`took.ms` and `took.cpu_ms`. CPU use is given as CPU time
(`daemon.cpu_ms`), not a percentage: two results in a row give the
percentage over the minute between them.

## Limits

`limits` in [`itero.config.json`](/reference/itero-config/#limits):

- past `runs` under way, an event that would start a run waits its turn;
- past `workflows`, `itero ws check` and each of the daemon's passes stop
  with an error naming the limit;
- past `space`, the daemon sends `itero.space.full` and logs an `error`
  (it measures `.itero/` at most once a second);
  `keep` forgets what it can, and until there is room no new run starts
  but internal ones. The log and its rolled copies count too, up to
  `log` times `logs + 1`, and forgetting does not make them smaller:
  limits that let them take all of `space` keep it full, and
  `itero ws check` and the daemon's start warn of them;
- past `log`, the daemon sends `itero.log.full` once, and again after the
  log is rolled and fills up;
- a step quiet past `quiet`, or running past `most`, is stopped and runs
  again, up to 3 times; then its run ends in error.
- past `steps` running at once, a step waits for one to end. So it does
  past what the daemon's open files allow, all its workspaces together:
  it raises its limit on open files to the most the system lets it have
  as it starts, keeps a quarter of them (64 at least) for the rest, and
  counts 7 for each running step. Under macOS's default of 256, which a
  daemon raises, that would be 27. The log says it once a pass, as a
  `warning`.
