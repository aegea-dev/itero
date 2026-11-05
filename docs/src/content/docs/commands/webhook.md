---
title: webhook
description: For connectors - takes webhooks from outside, each checked against a secret.
---

`itero webhook` is for connectors: it takes requests from a service outside
the machine, checks each against a secret, and saves it as the event
`itero.webhook.received`. [Webhooks](/guides/webhooks/) has the whole story.

```sh
itero webhook --port <port> [--host <address>] --hook <path>:<check>:<NAME> ...
```

| Option | Means |
|---|---|
| `--port <port>` | the port to listen on (needed); `0` for one the OS gives, told on stderr |
| `--host <address>` | the address to listen on; `127.0.0.1` unless given |
| `--hook <path>:<check>:<NAME>` | a path, its check (`token`, `hmac` or `twilio`) and its secret's name, looked up as a `$$NAME` is; once or more |

Every problem with the hooks is told at once, and it does not start with any.

It ends when its stdin closes, so a [connector](/guides/connectors/) running it
ends with the daemon:

```sh
#!/bin/sh
exec itero webhook --port 8788 --hook /github:hmac:GITHUB_SECRET
```

A file named after what it does is fine here (`webhook.listen`): it prints
nothing, and saves its events itself.
