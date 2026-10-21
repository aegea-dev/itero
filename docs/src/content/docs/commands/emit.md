---
title: itero emit
description: Sends a message, and says what it starts.
---

```sh
itero emit <type> ['<data>']
```

Sends a message: its type, two or more words joined by dots, and its data,
a JSON object. It is saved in the journal at once, so one sent while the
daemon is stopped waits for it.

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

Types starting with `itero.` are the daemon's own and refused. An app
sends the same with `POST /events` ([HTTP API](/reference/http/)). See
[Messages and sources](/guides/messages/).
