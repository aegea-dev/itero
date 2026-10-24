---
title: itero emit
description: Sends a message, and says what it starts.
---

```sh
itero emit <type> ['<data>']
itero emit --wait [--timeout 60s] <type> ['<data>']
itero emit --reply <url> <type> ['<data>']
```

Sends a message: its type, two or more words joined by dots, and its data,
a JSON object. It is saved in the journal at once, so one sent while the
daemon is stopped waits for it.

| Option | Does |
|---|---|
| `--wait` | waits for the runs it starts to end, and prints them as JSON (below) |
| `--timeout D` | with `--wait`: how long, like `30s` or `5m` (60s) |
| `--reply URL` | posts each run's end to `URL` when it ends ([Getting an answer](/guides/messages/#getting-an-answer)) |

It prints the message's id, then (on stderr) what the message will do, from
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

With `--wait`, it sends the message and returns when the runs it started
have ended, printing them; a script gets the workflow's output, not just a
message id:

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
| `0` | every run ended `done`; a message that starts nothing is answered `"runs": []` |
| `1` | one failed or ended in error (the answer says which), or the message was refused |
| `2` | `--timeout` ran out: the answer has the runs so far, such as one `waiting` |

It needs a running daemon, and says so at once when there is none, sending
nothing.

Types starting with `itero.` are the daemon's own and refused. An app
sends the same with `POST /events` ([HTTP API](/reference/http/)). See
[Messages and connectors](/guides/messages/).
