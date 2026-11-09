---
title: HTTP API
description: The daemon's local API, its routes and answers.
---

`itero daemon` serves on `127.0.0.1:<port>` only: `port` in
[`itero.config.json`](/reference/itero-config/), or, when it sets none, one
the OS gives, which `itero ws info` shows; an app that calls the API sets
one. The examples here use 8787. Answers are JSON;
an error is `{"error": "..."}` with its status.

| Route | Does |
|---|---|
| `GET /health` | the workspace, the daemon, its passes and its connectors |
| `POST /events` | sends an event (a message) |
| `GET /runs` | runs, newest first |
| `GET /events` | the events not yet handled |
| `GET /events/stream` | each event that comes in, as it comes |
| `GET /log/stream` | each new line of the log, as it is written |
| `DELETE /events/{id}` | cancels an event not yet handled |
| `GET /runs/{id or name}` | a run, its state and its events |
| `POST /runs/{id or name}/stop` | stops a run where it is |
| `GET /workflows` | each workflow's name and what starts it |

## Only from this machine

A request whose `Host` header is not `127.0.0.1:<port>` or
`localhost:<port>` is refused with 403, so a web page cannot reach the API
by pointing a name of its own at 127.0.0.1 (DNS rebinding). `POST /events`
takes only `application/json` (415 otherwise): a page on another site
cannot post one without a preflight, which is never granted.

A service outside the machine comes in through
[`itero webhook`](/guides/webhooks/), on a port of its own. Never point a
tunnel at this API: it can stop runs.

## GET /health

```json
{ "ok": true, "pid": 4242,
  "workspace": { "id": "01a0f3…", "name": "quiet-river", "created": "...", "itero": "0.15.0",
                 "path": "/Users/me/home", "meta": { "owner": "ayşe" } },
  "daemon": { "id": "01a15f2c…", "name": "brave-otter", "started": "...", "pid": 4242,
              "host": "mini", "itero": "0.15.0" },
  "started": "...", "passes": 42, "last_pass": "...", "last_error": null,
  "connectors": { "mail.received": { "state": "running", "restarts": 0, "last_exit": null } } }
```

`workspace` and `daemon` are their [IDs and names](/guides/log/#ids-and-names).

`busy_since`, there while a pass is under way, is when it began.

`connectors` has each [connector](/guides/connectors/): `running`,
`waiting` (to start again) or `gave up`, its restarts, and how it ended
last.

`streams` is how many follow the workspace now:
`{"events": 1, "log": 0}` ([GET /events/stream](#get-eventsstream),
[GET /log/stream](#get-logstream)).

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
              "status": "done",
              "state": { "event": { "order": 43 }, "output": { "tracking": "TR-43" },
                         "steps": { "ship": { "tracking": "TR-43" } } } } ] }
```

When the time runs out first, it answers 202 with the runs so far.

For runs that wait for days, the workflow posts its result itself, to a
URL the app sends in the event's data
([Posting a result](/guides/workflows/#posting-a-result)).

## GET /runs

`?workflow=<name>` keeps one workflow's runs; `?status=<status>` the runs
with one status (`running`, `waiting`, `done`, `failed`, `error`,
`stopped`);
`?limit=N` (50 unless given) caps them. Internal
workflows' runs only with `?all=true`. `parent` is the id of the run
whose step started it, for a workflow used as a step, else `null`.

```json
[ { "id": "...", "name": "misty-lake", "workflow": "order", "status": "waiting",
    "started": "...", "ended": null, "parent": null } ]
```

## POST /runs/\{id or name\}/stop

Stops a run where it is, as [`itero ws wf stop`](/commands/workflow/#stop)
does: `202` once asked, `409` when it has ended, `404` when there is
none.

## GET /events, DELETE /events/\{id\}

`GET /events` lists the events not yet handled, as
[`itero ws ev list`](/commands/event/#list) does: `[{ "id", "type",
"time", "data" }]`. `DELETE /events/{id}` cancels one: `200` with
`{"cancelled": id}`, `409` when a daemon has handled it (with the runs it
`started`) or runs waiting for it have it already (those runs, as
`reached`), `404` when there is none.

## GET /events/stream

Each event that comes into the workspace, as it comes, one JSON line each
(`application/x-ndjson`): from connectors, webhooks, `POST /events`,
`itero ws emit` and itero's own (`itero.run.done`,
`itero.notification.clicked`, ...). They are the events a workflow's `on`
sees, whether or not they start a run.

```bash
curl -sN http://127.0.0.1:8787/events/stream | jq .
```

```json
{"id": "01a1…", "type": "order.placed", "time": "2026-11-09T10:00:00Z",
 "source": "outside", "data": {"order": 43}}
```

`source` says where it came from: `outside` (posted or emitted),
`webhook`, `connector/<type>`, or `daemon` (itero's own).

It starts with the events saved after the request.

| Param | Does |
|---|---|
| `after=<id>` | starts after that event instead: every one since, then on |
| `type=<type>` | only events of that type; give it again for more (`?type=order.placed&type=order.paid`) |
| `count=N` | ends after N events: `?count=1` waits for the next one |
| `every=<duration>` | how often it looks: `100ms` unless given, from `10ms` to `1m` |

An `after` the journal no longer has (forgotten by
[`keep`](/guides/maintenance/), or never sent) is 410, so a listener knows
it missed some. A bad parameter is 400, every problem in one `error`.

The stream ends when the daemon stops. A listener that keeps the last
`id` it got starts again with `?after=` it and misses nothing:

```bash
curl -sN "http://127.0.0.1:8787/events/stream?type=order.placed&count=1"   # the next order
```

## GET /log/stream

Each new line of [the log](/guides/log/), as the daemon writes it, the
same JSON as the file (`application/x-ndjson`), from the request on. It
follows a roll: the rest of the old file, then the new one. Only whole
lines are sent. It takes `count` and `every` as `GET /events/stream`
does.

```bash
curl -sN http://127.0.0.1:8787/log/stream | jq -r '"\(.time) \(.event) \(.workflow // "")"'
```

Both streams read what is already saved: with nobody listening they cost
the daemon nothing, and a slow listener is read at its own pace. Through
`itero daemon --all` they are `/ws/<name>/events/stream` and
`/ws/<name>/log/stream`.

## GET /runs/\{id or name\}

The run as in the list, with its `input` (the data it started with), its
`state`, and its `events`: each with `seq`, `type`, `time`, `handled`,
`error` and `data`. A running run also has `progress`: each running step's
`step`, what it `said` last on stderr, and `at` when, newest first. Of two runs with one name, the newest. 404 for a run
there is not.

## GET /workflows

```json
[ { "name": "order", "path": "workflows/shop/order.json", "internal": false,
    "on": { "events": [{ "event": "order.placed" }], "schedules": [] } } ]
```

`path` is from the workspace, and `null` for a built-in workflow.

## One itero per machine

`itero daemon --all` ([One itero per machine](/guides/machine/)) serves
every workspace on the machine's list on one port, with the same `Host`
check:

| Route | Does |
|---|---|
| `GET /health` | the daemon, each running workspace's passes and connectors by name, itero-ai |
| `GET /workspaces` | each listed workspace: `name`, `folder`, `state`, `why` |
| `POST /events` | an event for every workspace on the list, as [`itero emit`](/commands/emit/#every-workspace) sends it |
| `/ws/<name>/...` | that workspace's routes above: `POST /ws/<name>/events`, `GET /ws/<name>/runs` |

```json
[{ "name": "quiet-river", "folder": "/Users/me/projects/home", "state": "running", "why": null },
 { "name": "misty-lake", "folder": "/Users/me/projects/mail", "state": "stopped",
   "why": "/Users/me/projects/mail: this workspace is run by brave-otter, pid 4211 already" }]
```

`POST /events` takes the same body as a workspace's, and answers 202 with
the event's id in each workspace, by name, and why it was not sent to any
that could not take it. When it reached none it is 409 (an empty list,
or every folder missing), or 500 when a journal could not save it. It has
no `?wait=`: that is `POST /ws/<name>/events?wait=`.

```json
{ "ids": { "quiet-river": "01a1602a-43f3-70a6-a731-c2210dc038ef" },
  "errors": { "misty-lake": "no workspace in /Users/me/projects/mail" } }
```

A name that is not running there is 404, saying to look at
`itero workspace list`.
