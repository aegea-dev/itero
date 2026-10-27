---
title: HTTP API
description: The daemon's local API, its routes and answers.
---

`itero daemon` serves on `127.0.0.1:<port>` only: `port` in
[`itero.config.json`](/reference/itero-config/), or, when it sets none, one
the OS gives, which `itero info` shows; an app that calls the API sets
one. The examples here use 8787. Answers are JSON;
an error is `{"error": "..."}` with its status.

| Route | Does |
|---|---|
| `GET /health` | the workspace, the daemon, its passes and its connectors |
| `POST /events` | sends an event (a message) |
| `GET /runs` | runs, newest first |
| `GET /runs/{id or name}` | a run, its state and its events |
| `GET /workflows` | each workflow's name and what starts it |

## Only from this machine

A request whose `Host` header is not `127.0.0.1:<port>` or
`localhost:<port>` is refused with 403, so a web page cannot reach the API
by pointing a name of its own at 127.0.0.1 (DNS rebinding). `POST /events`
takes only `application/json` (415 otherwise): a page on another site
cannot post one without a preflight, which is never granted.

## GET /health

```json
{ "ok": true, "pid": 4242,
  "workspace": { "id": "01a0f3…", "name": "quiet-river", "created": "...", "itero": "0.14.0",
                 "path": "/Users/me/home", "meta": { "owner": "ayşe" } },
  "daemon": { "id": "01a15f2c…", "name": "brave-otter", "started": "...", "pid": 4242,
              "host": "mini", "itero": "0.14.0" },
  "started": "...", "passes": 42, "last_pass": "...", "last_error": null, "busy_since": null,
  "connectors": { "mail.received": { "state": "running", "restarts": 0, "last_exit": null } } }
```

`workspace` and `daemon` are their [IDs and names](/guides/log/#ids-and-names).

`busy_since` is when the pass under way began, `null` while the daemon
waits; the service's watchdog restarts a daemon whose pass runs too long.

`connectors` has each [connector](/guides/connectors/): `running`,
`waiting` (to start again) or `gave up`, its restarts, and how it ended
last.

## POST /events

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' \
  -d '{"type": "order.placed", "data": {"order": 43}}'
```

| Field | Is |
|---|---|
| `type` | the event's type, two or more words joined by dots (needed) |
| `data` | a JSON object |

Answers 202 `{"id": "..."}`; 400 for a bad event, or one of the engine's
own types (`itero.*`, `action.*`, `timer.*`, `run.*`, `workflow.*`,
`connector.*`); 422 for a field it does not know. See
[Connectors](/guides/connectors/).

With `?wait=N` (seconds, up to 600) it holds the request until the runs
the event started have ended, and answers 200 with them:

```json
{ "id": "01a1…", "message": "01a1…",
  "runs": [ { "run": "01a1…", "name": "misty-lake", "workflow": "order",
              "status": "done", "state": { "order": 43, "tracking": "TR-43", "output": "" } } ] }
```

When the time runs out first, it answers 202 with the runs so far.

For runs that wait for days, the workflow posts its result itself, to a
URL the app sends in the event's data
([Posting a result](/guides/workflows/#posting-a-result)).

## GET /runs

`?workflow=<name>` keeps one workflow's runs; `?status=<status>` the runs
with one status (`running`, `waiting`, `done`, `failed`, `error`,
`split`); `?limit=N` (50 unless given) caps them. Internal workflows'
runs only with `?all=true`.

```json
[ { "id": "...", "name": "misty-lake", "workflow": "order", "status": "waiting",
    "started": "...", "ended": null } ]
```

## GET /runs/\{id or name\}

The run as in the list, with its `input` (the data it started with), its
`state`, and its `events`: each with `seq`, `type`, `time`, `handled`,
`error` and `data`. A running run also has `progress`: each running step's
`step`, what it `said` last on stderr, and `at` when, newest first. Of two runs with one name, the newest. 404 for a run
there is not.

## GET /workflows

```json
[ { "name": "order", "path": "workflows/shop/order.json", "internal": false,
    "on": { "events": ["order.placed"], "schedules": [] } } ]
```

`path` is from the workspace, and `null` for a built-in workflow.
