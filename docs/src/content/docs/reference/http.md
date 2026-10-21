---
title: HTTP API
description: The daemon's local API, its routes and answers.
---

`itero daemon` serves on `127.0.0.1:<port>` only (`port` in
[`itero.toml`](/reference/itero-toml/), 8787 by default). Answers are JSON;
an error is `{"error": "..."}` with its status.

| Route | Does |
|---|---|
| `GET /health` | the daemon, its passes and its sources |
| `POST /events` | sends a message |
| `GET /runs` | runs, newest first |
| `GET /runs/{id}` | a run, its state and its events |
| `GET /workflows` | each workflow's name and what starts it |

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
  "sources": { "mail.received": { "state": "running", "restarts": 0, "last_exit": null } } }
```

`sources` has each [source](/guides/messages/#sources): `running`,
`waiting` (to start again) or `gave up`, its restarts, and how it ended
last.

## POST /events

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' \
  -d '{"type": "order.placed", "data": {"order": 43}}'
```

| Field | Is |
|---|---|
| `type` | the message's type, two or more words joined by dots (needed) |
| `data` | a JSON object |

Answers 202 `{"id": "..."}`; 400 for a bad message, or one of the
engine's own types (`action.*`, `timer.*`, `run.*`, `workflow.*`,
`source.*`). See [Messages and sources](/guides/messages/).

## GET /runs

`?workflow=<name>` keeps one workflow's runs; `?status=<status>` the runs
with one status (`running`, `waiting`, `done`, `failed`, `error`,
`split`); `?limit=N` (50 unless given) caps them.

```json
[ { "id": "...", "workflow": "order", "status": "waiting",
    "started": "...", "ended": null } ]
```

## GET /runs/\{id\}

The run as in the list, with its `input` (the data it started with), its
`state`, and its `events`: each with `seq`, `type`, `time`, `handled`,
`error` and `data`. 404 for a run there is not.

## GET /workflows

```json
[ { "name": "order", "on": { "messages": ["order.placed"], "schedules": [] } } ]
```
