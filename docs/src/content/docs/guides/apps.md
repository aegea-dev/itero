---
title: Apps and events
description: How an app starts runs and moves them along, and how a workflow acts on the app.
---

Apps connect to itero through the daemon, not as a library: **events in**,
**actions out**.

## Events in

An event has a `type`, like `photo.added`, and may have `data` (a JSON
object) and a `run`.

Over the [HTTP API](/reference/http/):

```bash
curl -s -X POST http://127.0.0.1:8787/events \
  -H 'content-type: application/json' \
  -d '{"type": "photo.added", "data": {"path": "trip/market.jpg"}}'
```

From the command line, in the workspace:

```bash
itero emit photo.added --data '{"path": "trip/market.jpg"}'
```

`itero emit` writes the journal directly, so it works while the daemon is
stopped: the daemon handles the event when it starts. Both print the
event's id.

- **Without `run`**, the event is for the workspace: it starts a run of
  every workflow with a trigger `on` its type, with its `data` as the run's
  `input`. An event nothing waits for is handled with a note.
- **With `run`**, the event goes to that run: a transition from the run's
  state with `on` that type is taken, and the event's `data` is its
  `input`.

The engine's own types, starting with `action.`, `timer.` or `run.`, are
refused.

## Waiting for an app

A run can wait in a state for the app to answer:

```json
{
  "name": "review",
  "states": ["asked", "approved", "rejected"],
  "start": "asked",
  "end": ["approved", "rejected"],
  "triggers": [ { "on": "draft.ready" } ],
  "transitions": [
    { "from": "asked", "to": "approved", "on": "draft.approved" },
    { "from": "asked", "to": "rejected", "on": "draft.rejected",
      "timeout": "7d", "on_timeout": "rejected" }
  ]
}
```

The app sends `draft.approved` with the run's id:

```bash
itero emit draft.approved --run <run id>
```

`itero runs` and `GET /runs` list the runs and their ids. An event for a run
that has ended is refused.

## Actions out

A workflow acts on an app through the app's own command line or API:
`command@1` running the app's program, or `http.request@1` calling its API.
An app's records stay in the app; a workflow's [memory](/guides/workflows/#memory)
is for the workflow's own bookkeeping.
