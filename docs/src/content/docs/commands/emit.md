---
title: emit
description: Sends an event, and says what it starts.
---

`itero emit` sends an event (a message) to your workflows: the ones that
start on it run, and runs waiting for it go on. Use it from a script, an
app, or by hand to try a workflow.

```sh
itero emit <type> ['<data>']
itero emit --wait [--timeout 60s] <type> ['<data>']
```

An event has a type, two or more words joined by dots (`order.placed`), and
data, a JSON object. It is saved at once, so one sent while the daemon is
stopped waits for it.

| Option | Does |
|---|---|
| `--wait` | waits for the runs it starts to end, and prints them as JSON (below) |
| `--timeout D` | with `--wait`: how long, like `30s` or `5m` (60s) |
| `--json` | prints its id and what it starts as JSON |

It prints the event's id, then (on stderr) what the event will do, from
the workflows on disk:

```
$ itero emit greet.someone '{"name": "Can"}'
01a1602a-43f3-70a6-a731-c2210dc038ef
starts: greet

$ itero emit greet.someon '{}'
01a1602a-43fc-71b8-b210-dbf25ba0b65e
nothing starts on greet.someon: did you mean greet.someone?
no daemon is running: it waits for itero daemon
```

- `starts:` the workflows it starts;
- `may reach N waiting run(s)`, when runs wait for data;
- `nothing starts on …`, with the nearest type that does start something;
- `no daemon is running`, when none answers on the workspace's port.

## Waiting for the answer

With `--wait`, it sends the event and returns when the runs it started
have ended, printing them; a script gets the workflow's output, not just an
id:

```
$ itero emit --wait website.requested '{"url": "https://example.com"}'
{
  "message": "01a1602a-43f3-70a6-a731-c2210dc038ef",
  "runs": [
    { "run": "01a1602a-4401-7c8e-9e0b-1f2b5a3c4d5e", "name": "misty-lake",
      "workflow": "page-summary", "status": "done",
      "state": { "url": "https://example.com", "text": "- The page is …" } }
  ]
}
$ itero emit --wait website.requested '{"url": "https://example.com"}' | jq -r '.runs[0].state.text'
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
