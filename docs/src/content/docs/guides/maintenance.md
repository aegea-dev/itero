---
title: The daemon's own workflows
description: The messages the daemon sends about itself, internal workflows, the built-in keep and logs, and the workspace's limits.
---

The daemon looks after itself with the same parts as everything else:
messages about what it notices, and workflows that handle them. They show
in `itero check`, `itero runs --all` and the log like any other, and a
workspace changes or turns them off the same way.

## Messages from the daemon

Their types start with `itero.`, which only the daemon may send:

| Type | When | Data |
|---|---|---|
| `itero.daemon.started` | the daemon starts | `daemon`, `name`, `itero` |
| `itero.run.error` | a run ended in error | `workflow`, `run`, `failed_step`, `why` |
| `itero.run.failed` | a run ended as failed | `workflow`, `run`, `why` |
| `itero.source.gave-up` | a source kept ending and was given up on | `source`, `why` |
| `itero.space.full` | `.itero/` is past its `space` limit | `used`, `limit` |
| `itero.log.full` | the log is past its `log` limit | `path`, `size`, `limit` |

A workflow on one is written like any other:

```json
{ "on": [{ "event": "itero.run.error" }],
  "steps": [{ "#notify": { "title": "itero", "text": "$workflow: $why" } }] }
```

A message about a run never starts its own workflow, so a broken workflow
cannot set itself off again and again. No run waits for these messages:
they only start workflows.

## Internal workflows

A workflow whose entry in `itero.config.json` says `"internal": true` looks after the workspace
rather than doing its work. Nothing in its name says so. Being internal:

- `itero check` lists it apart, last;
- `itero runs` and `GET /runs` leave its runs out unless asked
  (`itero runs --all`, `?all=true`); `itero errors` shows its errors;
- its runs do not count against `runs`, nor it against `workflows`;
- its lines in the log carry `"internal": true`.

## Built in: `keep` and `logs`

itero carries two internal workflows and runs them as if they were in
`workflows/`:

| Workflow | On | Does |
|---|---|---|
| `keep` | every hour, and `itero.space.full` | `#forget`: forgets ended runs and their messages past `keep`; all ended ones while space is short |
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

- past `runs` under way, a message that would start a run waits its turn;
- past `workflows`, `itero check` and each of the daemon's passes stop
  with an error naming the limit;
- past `space`, the daemon sends `itero.space.full` and logs an `error`;
  `keep` forgets what it can, and until there is room no new run starts
  but internal ones;
- past `log`, the daemon sends `itero.log.full` once, and again after the
  log is rolled and fills up;
- a step quiet past `quiet`, or running past `most`, is stopped and runs
  again, up to 3 times; then its run ends in error.
