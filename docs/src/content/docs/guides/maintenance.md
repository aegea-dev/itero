---
title: The daemon's own workflows
description: The events the daemon sends about itself, internal workflows, the built-in keep and logs, and the workspace's limits.
---

The daemon looks after itself with the same parts as everything else:
events about what it notices, and workflows that handle them. They show
in `itero check`, `itero wf runs --all` and the log like any other, and a
workspace changes or turns them off the same way.

## Events from the daemon

Their types start with `itero.`, which only itero may send (a click on a
notification comes through Itero.app):

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

A workflow on one is written like any other:

```json
{ "on": [{ "event": "itero.run.error" }],
  "steps": [{ "#notify": { "args": { "title": "itero", "text": "$workflow: $why" } } }] }
```

An event about a run never starts its own workflow, so a broken workflow
cannot set itself off again and again. A run that an `itero.run.` event
started sends no `itero.run.done`: two workflows that watch every run's
end would otherwise start each other forever. To start a workflow when
another one is done, name it in the event's condition:

```json
{ "on": [{ "event": "itero.run.done" }, { "$workflow": "nightly-backup" }],
  "steps": ["#upload-report"] }
``` No run waits for these events:
they only start workflows.

## Internal workflows

A workflow whose entry in `itero.config.json` says `"internal": true` looks after the workspace
rather than doing its work. Nothing in its name says so. Being internal:

- `itero check` lists it apart, last;
- `itero wf runs` and `GET /runs` leave its runs out unless asked
  (`itero wf runs --all`, `?all=true`); `itero action errors` counts its
  errors;
- its runs do not count against `runs`, nor it against `workflows`;
- its lines in the log carry `"internal": true`.

## Built in: `keep` and `logs`

itero carries two internal workflows and runs them as if they were in
`workflows/`:

| Workflow | On | Does |
|---|---|---|
| `keep` | every hour, and `itero.space.full` | `#forget`: forgets ended runs and their events past `keep`; all ended ones while space is short |
| `logs` | `itero.log.full` | `#roll-log`: `itero.log` becomes `itero.log.1`, `.1` becomes `.2`, keeping `logs` of them |

`#forget` and `#roll-log` are built-in actions, like `#notify`.

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

## Limits

`limits` in [`itero.config.json`](/reference/itero-config/#limits):

- past `runs` under way, an event that would start a run waits its turn;
- past `workflows`, `itero check` and each of the daemon's passes stop
  with an error naming the limit;
- past `space`, the daemon sends `itero.space.full` and logs an `error`;
  `keep` forgets what it can, and until there is room no new run starts
  but internal ones;
- past `log`, the daemon sends `itero.log.full` once, and again after the
  log is rolled and fills up;
- a step quiet past `quiet`, or running past `most`, is stopped and runs
  again, up to 3 times; then its run ends in error.
