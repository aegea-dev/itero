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
| `GET /health` | the workspace, the daemon, its passes and its sources |
| `POST /events` | sends a message |
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
  "workspace": { "id": "01a0f3…", "name": "quiet-river", "created": "...", "itero": "0.2.0",
                 "path": "/Users/me/home", "meta": { "owner": "ayşe" } },
  "daemon": { "id": "01a15f2c…", "name": "brave-otter", "started": "...", "pid": 4242,
              "host": "mini", "itero": "0.2.0" },
  "started": "...", "passes": 3600, "last_pass": "...", "last_error": null,
  "sources": { "mail.received": { "state": "running", "restarts": 0, "last_exit": null } } }
```

`workspace` and `daemon` are their [IDs and names](/guides/log/#ids-and-names).

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
| `reply` | an `http://` or `https://` URL: each run it starts is posted there when it ends |

Answers 202 `{"id": "..."}`; 400 for a bad message, one of the engine's
own types (`itero.*`, `action.*`, `timer.*`, `run.*`, `workflow.*`,
`source.*`), or a `reply` that is not an http(s) URL. See
[Messages and sources](/guides/messages/).

With `?wait=N` (seconds, up to 600) it holds the request until the runs
the message started have ended, and answers 200 with them:

```json
{ "id": "01a1…", "message": "01a1…",
  "runs": [ { "run": "01a1…", "name": "misty-lake", "workflow": "order",
              "status": "done", "state": { "order": 43, "tracking": "TR-43" } } ] }
```

When the time runs out first, it answers 202 with the runs so far.

## Callbacks

A message sent with `reply` has each run it starts posted to that URL when
the run ends, as `application/json`:

```json
{ "message": "01a1…", "run": "01a1…", "name": "misty-lake", "workflow": "order",
  "status": "done", "state": { "order": 43, "tracking": "TR-43" } }
```

An answer of 2xx takes it. Anything else, or no answer within 10 seconds,
is tried again on a later pass, waiting twice as long each time (10
seconds to an hour); a day after the first try it is given up on, with an
`error` line in the log. A run that waits for days is posted days later.

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
`error` and `data`. Of two runs with one name, the newest. 404 for a run
there is not.

## GET /workflows

```json
[ { "name": "order", "path": "workflows/shop/order.json", "internal": false,
    "on": { "events": ["order.placed"], "schedules": [] } } ]
```

`path` is from the workspace, and `null` for a built-in workflow.
