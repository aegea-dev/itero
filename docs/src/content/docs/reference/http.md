---
title: HTTP API
description: The daemon's local API, its routes and answers.
---

`itero daemon` serves on `127.0.0.1:<port>` only (`port` in
[`itero.toml`](/reference/itero-toml/), 8787 by default). Answers are JSON;
an error is `{"error": "..."}` with its status.

| Route | Does |
|---|---|
| `GET /health` | the daemon and its passes |
| `POST /events` | sends an event |
| `GET /runs` | runs, newest first |
| `GET /runs/{id}` | a run and its events |
| `GET /workflows` | each workflow's name, version and triggers |
| `GET /workflows/{name}/memory` | a workflow's memory |

## Only from this machine

A request whose `Host` header is not `127.0.0.1:<port>` or
`localhost:<port>` is refused with 403, so a web page cannot reach the API
by pointing a name of its own at 127.0.0.1 (DNS rebinding). `POST /events`
takes only `application/json` (415 otherwise): a page on another site
cannot post one without a preflight, which is never granted.

## GET /health

```json
{ "ok": true, "pid": 4242, "workspace": "/Users/me/home",
  "started": "...", "passes": 3600, "last_pass": "...", "last_error": null,
  "sources": { "mailbox": { "state": "running", "restarts": 0, "last_exit": null } } }
```

`sources` has each [source](/guides/sources/): `running`, `waiting` (to
start again) or `gave up`, its restarts, and how it ended last.

## POST /events

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' \
  -d '{"type": "photo.added", "data": {"path": "trip/market.jpg"}}'
```

| Field | Is |
|---|---|
| `type` | the event's type, lowercase words joined by dots (needed) |
| `data` | a JSON object, the run's or transition's `input` |
| `run` | the run it is for; without, it is for the workspace |

Answers 202 `{"id": "..."}`. 400 for a bad event (or one of the engine's
own types, `action.*`, `timer.*`, `run.*`, `workflow.*`, `source.*`), 404 for a run there is not, 409
for a run that has ended. See [Apps and events](/guides/apps/).

## GET /runs

`?workflow=<name>` keeps one workflow's runs, `?state=<state>` the runs in
one state (`?state=failed` the failed ones); `?limit=N` (50 unless given)
caps them.

```json
[ { "id": "...", "workflow": "mail-check", "state": "done",
    "started": "...", "ended": "..." } ]
```

## GET /runs/\{id\}

The run as in the list, with its `args` and its `events`: each with `seq`,
`type`, `time`, `handled`, `error` and `data`. 404 for a run there is not.

## GET /workflows

```json
[ { "name": "mail-check", "version": "3f9a1c0b...",
    "triggers": [ { "on": "mail.changed", "args": { "mailbox": "" } } ],
    "paused": null } ]
```

The triggers are the workflow's own merged with the workspace's
`triggers/`. `paused` is null, or the pause: `{workflow, pin, at, why}`
(see [Workflow files](/reference/workflow-file/#paused)).

## GET /workflows/\{name\}/memory

The workflow's memory as an object, `{"seen": "..."}`. 404 for a workflow
there is not.
