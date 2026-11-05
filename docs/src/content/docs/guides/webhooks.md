---
title: Webhooks
description: How a service outside the machine sends itero an event, safely.
---

A webhook is a service outside the machine (WhatsApp, an SMS provider,
GitHub, your own app) telling yours that something happened. `itero webhook`
takes those requests, checks each against a secret, and saves it as an
event. A connector runs it:

```sh
#!/bin/sh
# connectors/webhook/webhook.listen
exec itero webhook --port 8788 \
  --hook /whatsapp:hmac:WHATSAPP_APP_SECRET \
  --hook /sms:twilio:TWILIO_AUTH_TOKEN \
  --hook /app:token:APP_WEBHOOK_TOKEN
```

The daemon starts it, starts it again if it ends, and `/health` shows it. Give
it no args from a workflow's `on`: that runs a second copy of the connector, on
the same port.
Keep its secrets as [secrets](/guides/secrets/) or in `.env`: `--hook` holds
only their names. It does not start while a secret is missing or shorter than
16 bytes, and says which, so there is no webhook without a check.

## Reaching it

It listens on 127.0.0.1, and does no TLS. A tunnel or a proxy gives it a
public HTTPS address and sends requests on to this port:

- [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/):
  `cloudflared tunnel --url http://127.0.0.1:8788`, for a quick one;
- [Tailscale Funnel](https://tailscale.com/kb/1223/funnel): `tailscale funnel 8788`.
  The first time, enable HTTPS Certificates in its admin console's DNS page; then
  it prints a link that enables Funnel for your tailnet;
- [Caddy](https://caddyserver.com/), on a server with a public address:
  `reverse_proxy 127.0.0.1:8788`.

:::caution
Point a tunnel at this port, and never at the daemon's own. The daemon's
[HTTP API](/reference/http/) can stop runs and cancel events, and answers only
on this machine for that reason.
:::

`--host` listens on another address, for a proxy on another machine or in a
container, or on a server: `--host 0.0.0.0`. Requests then cross the network
as they are, unencrypted unless a proxy does it, and it warns about that when
it starts.

## Hooks

A `--hook` is `<path>:<check>:<secret's name>`. The path is the end of the URL
you give the service, and yours to name: `https://you.example.com/whatsapp` is
`/whatsapp`. Only the paths listed are answered. The check says how a request
to it proves where it comes from:

| Check | The service sends | Use it for |
|---|---|---|
| `hmac` | `X-Hub-Signature-256: sha256=<hex>`, an HMAC-SHA256 of the raw body | GitHub, WhatsApp, your own app |
| `token` | `Authorization: Bearer <secret>` | your own app, when it can only set a header |
| `twilio` | `X-Twilio-Signature`, Twilio's signature of the URL and the form | SMS by Twilio |

A signature is the stronger of the two: the secret never travels with the
request, and a changed body fails. A service that does this another way needs
a [connector](/guides/connectors/) of your own, which prints a line per event.

**WhatsApp** asks with a GET when you register the URL. An `hmac` hook answers
it with the `hub.challenge` it was sent. Your verify token can be any text: it
is not compared, because a request is still checked against your app secret.

**Twilio** signs the URL it called, so its check needs the `Host` your tunnel
or proxy received: one that rewrites it fails the check, and nothing gets in.

## The event

A request that passes is saved as `itero.webhook.received`, then answered 200.
Its data:

```json
{ "path": "/whatsapp",
  "headers": { "user-agent": "..." },
  "body": { "entry": [] } }
```

- `headers`, in lower case, without `authorization`, `cookie`,
  `proxy-authorization` and the hook's own signature. A `-` ends a name in a
  workflow, so write `_` for it: `$event.headers.x_github_event` reads
  `x-github-event`.
- `body`: the JSON itself; for a form, an object of texts, a field sent twice
  a list; anything else as text.

Only itero sends this type: `POST /events` and `itero ws emit` refuse it. It
starts workflows and reaches no step waiting for an event, whatever its data
says, because what it holds was written by someone else.

## The forward workflow

One workflow takes every webhook, and its action sends the events the others
use, from the fields it trusts:

```json
{ "on": [{ "event": "itero.webhook.received" }],
  "steps": [{ "#forward-webhook": { "args": { "path": "$event.path", "body": "$event.body" } } }] }
```

`actions/forward-webhook`:

```sh
#!/bin/sh
input=$(cat)
case $(printf '%s' "$input" | jq -r '.args.path') in
/app)
  # a service sends again when a delivery failed: each message once
  id=$(printf '%s' "$input" | jq -c '.args.body.id')
  touch "$ITERO_MEMORY"
  grep -qxF -- "$id" "$ITERO_MEMORY" && exit 0
  printf '%s\n' "$id" >>"$ITERO_MEMORY"
  printf '%s' "$input" |
    jq -c '.args.body | {from: (.from | tostring), text: (.text | tostring)}' |
    itero ws emit app.message.received --file -
  ;;
esac
```

So a service's shape, nested lists and all, stays in one place, and everything
else reads `$event.from`. `examples/webhook` is this, working, to try with
`curl`; `examples/github-webhook` is GitHub's pushes through a tunnel, as a
notification.

## Waiting for a reply

To send a message and go on when its reply comes, a step sends it, prints what
the reply will have, and exits 75
([Waiting](/guides/workflows/#waiting-timeout)):

```sh
#!/bin/sh
if [ -n "${event:-}" ]; then
  echo "$event"   # the reply: this step's result
  exit 0
fi
# here, call your app with the question
jq -c '{from: .args.to}'
exit 75
```

`timeout` on the workflow is how long it waits. A service that takes its
webhook's URL with each message gets it as an arg, kept in `.env` since it
changes from one machine to another: `"callback": "$$WEBHOOK_URL"`.

## What to watch for

- **The words are a stranger's.** A check proves the service sent a request,
  not that its content is safe: anyone can text your number. Never `eval` it.
- **Forward only what you chose,** and not under a field another workflow
  waits on: a flat event wakes any step waiting on a field it shares.
- **Requests come twice.** A service sends again when a delivery failed, so
  skip an id already seen, as above. That check is simple: two deliveries at
  the very same moment can both pass it.
- **The log has the body,** as it has every event's data.
- **Floods.** A request that fails its check is never saved, so it starts no
  run and fills no disk. Each hook takes 32 requests at once, and answers 503
  past that while the others go on. Each connection carries one request and
  lives 30 seconds, and a body is at most 5 MiB. Many open connections can
  still fill the process's file descriptors for those 30 seconds, and only a
  tunnel or proxy, which sees who is sending, can stop them coming.

A webhook that must stay apart from a flood on another gets its own connector
and port, best on its own hostname:

```sh
#!/bin/sh
# connectors/webhook/webhook.listen-orders
exec itero webhook --port 8789 --hook /orders:hmac:ORDERS_SECRET
```

The same forward workflow takes both.

## What it answers

| Answer | When |
|---|---|
| 200, empty | checked and saved |
| 400 | a body that says JSON and is not; to a `twilio` hook, one that is not a form; a handshake without digits |
| 401 | the check failed |
| 404 | not a listed path |
| 405 | not a POST, and no handshake |
| 413 | a body over 5 MiB |
| 415 | a body that is not text |
| 500 | the journal could not save it: a service sends it again |
| 503 | 32 requests at once on this hook |

It never waits for a run: 200 means saved. A refusal is told on stderr (the
daemon's log), once a minute per path and reason, without anything a stranger
sent.
