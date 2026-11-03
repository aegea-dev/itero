---
title: emit
description: Sends an event to one workspace, or to every workspace on the machine's list.
---

`itero ws emit` sends an event (a message) to one workspace's workflows:
the ones that start on it run, and runs waiting for it go on. Use it from a
script, an app, or by hand to try a workflow.
[`itero emit`](#every-workspace) sends it to every workspace on the
machine's list.

```sh
itero ws emit <type> ['<data>']
itero ws emit --wait [--timeout 60s] <type> ['<data>']
itero emit <type> ['<data>']
```

`itero ws emit` picks the workspace of the folder it runs in, or the one
`--ws <name>` names.

An event has a type, two or more words joined by dots (`order.placed`), and
data, a JSON object. It is saved at once, so one sent while the daemon is
stopped waits for it.

| Option | Does |
|---|---|
| `--wait` | waits for the runs it starts to end, and prints them as JSON (below) |
| `--timeout D` | with `--wait`: how long, like `30s` or `5m` (60s) |
| `--json` | prints its id and what it starts as JSON |

It prints the event's id (which [`itero ws ev cancel`](/commands/event/#cancel)
takes back while no daemon has handled it), then (on stderr) what the
event will do, from the workflows on disk:

```
$ itero ws emit greet.someone '{"name": "Can"}'
01a1602a-43f3-70a6-a731-c2210dc038ef
starts: greet

$ itero ws emit greet.someon '{}'
01a1602a-43fc-71b8-b210-dbf25ba0b65e
nothing starts on greet.someon: did you mean greet.someone?
no daemon is running: it waits for itero daemon
```

- `starts:` the workflows it starts;
- `may reach N waiting run(s)`, when runs wait for data;
- `nothing starts on …`, with the nearest type that does start something;
- `no daemon is running`, when none answers on the workspace's port.

## Waiting for the answer

With `--wait`, `itero ws emit` sends the event and returns when the runs it started
have ended, printing them; a script gets the workflow's output, not just an
id:

```
$ itero ws emit --wait website.requested '{"url": "https://example.com"}'
{
  "message": "01a1602a-43f3-70a6-a731-c2210dc038ef",
  "runs": [
    { "run": "01a1602a-4401-7c8e-9e0b-1f2b5a3c4d5e", "name": "misty-lake",
      "workflow": "page-summary", "status": "done",
      "state": { "event": { "url": "https://example.com" }, "output": "- The page is …",
                 "steps": { "fetch": "<!doctype html>…", "ask-ai": "- The page is …" } } }
  ]
}
$ itero ws emit --wait website.requested '{"url": "https://example.com"}' | jq -r '.runs[0].state.output'
```

Exit codes:

| Exit | Means |
|---|---|
| `0` | every run ended `done`; an event that starts nothing is answered `"runs": []` |
| `1` | one failed or ended in error (the answer says which), or the event was refused |
| `2` | `--timeout` ran out: the answer has the runs so far, such as one `waiting` |

It needs a running daemon, and says so at once when there is none, sending
nothing.

For runs that wait for days, a workflow can post its result itself, from
its `success`, `failure` and `error` steps
([Posting a result](/guides/workflows/#posting-a-result)).

Types starting with `itero.` are the daemon's own and refused. An app
sends the same with `POST /events` ([HTTP API](/reference/http/)). See
[Connectors](/guides/connectors/).

## Every workspace

`itero emit` saves the event for every workspace on the machine's list,
from any folder, one id each. It prints each workspace's name and the
event's id there (`--json`: a list). A stopped workspace gets it too, as
with `itero ws emit`: it waits for its daemon.

```
$ itero emit backup.done '{"disk": "yedek"}'
misty-lake 01a1602a-43f3-70a6-a731-c2210dc038ef
quiet-river 01a1602a-4401-7c8e-9e0b-1f2b5a3c4d5e
```

- A workspace missing from its folder (`missing` in
  [`itero ws list`](/commands/workspace/#list)) is named on stderr and
  fails nothing: the others get the event. It exits 1 when one could not
  save it, or when none got it.
- A list with no workspaces is an error:
  [`itero ws add`](/commands/workspace/#add) puts one on it.
- `--wait` waits for the runs it started in all of them, within one
  `--timeout`, and prints them by workspace: `{"misty-lake": {"message":
  ..., "runs": [...]}, ...}`. The exit codes are the ones above: `1` when
  any failed, `2` when time ran out. It needs a daemon running each of them,
  and sends nothing when one has none.

The machine's API does the same with `POST /events`
([HTTP API](/reference/http/#one-itero-per-machine)).

An action runs in its workspace, so `itero ws emit` in an action sends to
its own workspace, and `itero emit` to every workspace. Use `itero emit`
for what the others may act on.
