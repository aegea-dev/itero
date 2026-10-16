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

The engine's own types, starting with `action.`, `timer.`, `run.`,
`workflow.` or `source.`, are refused.

Apps are one way events come in. A [source](/guides/sources/) is the other:
a long-lived program the daemon runs, which watches a file or holds a
connection and sends events as things happen.

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

## Without the run's id: match

The app may not know the run's id, only what the event is about: a draft's
id, a conversation's subject. A transition with a `match` hears its type's
events for the workspace too, and takes only those that are its run's:

```json
{ "from": "asked", "to": "approved", "on": "draft.approved",
  "match": "event.data.draft == run.input.draft" }
```

`run.input` is what the run started with: here the data of the
`draft.ready` that started it. Now the app sends:

```bash
itero emit draft.approved --data '{"draft": "d-42"}'
```

and the run waiting on draft `d-42` takes it; the others stay where they
are. In `match`, `event` is the event: its `id`, `type` and `data`.

## Publish and subscribe

Events for the workspace are a bus. **Subscribers** are the workflows with
a trigger `on` a type (each event starts a run of each) and the runs
waiting in a state with a transition `on` it and a `match`. **Publishers**
are apps (`POST /events`, `itero emit`), [sources](/guides/sources/), and
workflows, with a `publish` step:

```json
{ "publish": "report.ready", "data": { "month": "${input.month}" } }
```

A published event goes out when the chain succeeds, never back to the run
that published it. Each subscriber gets it once.

### Runs that live long

A state that loops to itself on an event, with a `match`, makes a run that
lives for good and reacts to every event that is its own:

```json
{ "name": "room", "triggers": [ { "on": "room.opened" } ],
  "states": ["listening"], "start": "listening",
  "transitions": [
    { "from": "listening", "to": "listening", "on": "reading.taken",
      "match": "event.data.room == run.input.room",
      "do": [ { "action": "memory.set@1", "with": { "last": "${input.celsius}" } } ] } ] }
```

Short runs stay the usual way: a recurring job is many short runs. A long
run suits something with a lifecycle: a conversation, an approval, a
device.

### When something fails

The engine publishes `run.failed` (the workflow, its version, the run,
where and why) when a run fails, and `workflow.paused` when a workflow is
paused. A maintenance workflow with a trigger `on` them hears every
workflow's failures but its own. `itero errors` lists the failed runs.

## Actions out

A workflow acts on an app through the app's own command line or API:
`command@1` running the app's program, or `http.request@1` calling its API.
An app's records stay in the app; a workflow's [memory](/guides/workflows/#memory)
is for the workflow's own bookkeeping.
