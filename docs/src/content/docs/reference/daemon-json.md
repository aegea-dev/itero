---
title: daemon.json
description: How the machine's daemon serves and where it notifies, read at its start.
---

`daemon.json` holds the settings of the machine's daemon
([`itero daemon`](/commands/daemon/), which
[`itero service`](/commands/service/) runs). It is in itero's config
folder: `$XDG_CONFIG_HOME/itero/daemon.json`, else
`~/.config/itero/daemon.json`. `itero daemon --config <path>` reads
another file instead. It describes the machine, never in a workspace's
repo; a workspace's own settings are in its
[`itero.config.json`](/reference/itero-config/)
([Which file?](/reference/concepts/#which-file)).

```json
{ "port": 8787,
  "listen": "0.0.0.0:8443",
  "tls": { "cert": "/etc/itero/tls/cert.pem", "key": "/etc/itero/tls/key.pem" },
  "notify": [{ "url": "https://ntfy.sh/itero-alerts", "format": "ntfy" }] }
```

| Setting | Is |
|---|---|
| `port` | the API's port on `127.0.0.1`, from 1 to 65535. Without it, the port the daemon kept from its last start, else one the OS gives, which it keeps |
| `listen` | an address and a port where the API is opened too, beside the local one: there every request needs a [token](/commands/token/). Without it, the API answers on `127.0.0.1` only |
| `secrets` | a folder of secrets the platform mounts, a file each, named after the secret (`/run/secrets/API_KEY`): `$$API_KEY` reads it ([On a server](/guides/secrets/#on-a-server)) |
| `tls` | `cert`, the certificate's PEM file (its chain after it), and `key`, its private key's: `listen` serves HTTPS with them. Without it, plain HTTP |
| `notify` | the channels `#notify` sends to on this machine, instead of a desktop ([Notifications](#notifications)). Without it, the desktop |

## Opening the API

With `listen`, the daemon serves the same routes on a second listener.
There, every request but `/livez` and `/readyz` (for probes,
[HTTP API](/reference/http/#one-itero-per-machine)) needs
`Authorization: Bearer <token>`, a token
[`itero token create`](/commands/token/) made on this machine; without
one it answers 401. Any `Host` is taken there, as the token is what a web
page cannot add. A request that changes something is logged with its
token's name (`laptop: POST /ws/shop/events`). The local API on
`127.0.0.1:<port>` stays as it is, for this machine's own commands and
the service's watchdog.

The daemon does not start opened when no token exists, as nobody could
reach it.

With `tls`, `listen` serves HTTPS (TLS 1.2 and 1.3, through rustls) with
that certificate, read and checked against its key when the daemon
starts: after renewing it, `itero service restart`. A connection has 10
seconds to finish its handshake, then 10 to send each request's
headers, as on every [listener](/reference/http/). The daemon warns when the key file is
readable by others on the machine (`chmod 600` it). Without `tls` it
serves plain HTTP and says so when it starts: then put a proxy that adds
TLS in front (Caddy, Traefik, a load balancer), or keep `listen` on a
network of your own. The local API stays plain HTTP on `127.0.0.1`.

The daemon reads it when it starts: after a change, `itero service
restart`. With no file there, the daemon runs as it always has; a file
named by `--config` must be there.

A setting it does not know, or a value that is wrong, stops the daemon
before it serves anything, with every problem told at once:

```
$ itero daemon
error: /Users/me/.config/itero/daemon.json: port: a number from 1 to 65535, not "8787"
/Users/me/.config/itero/daemon.json: prot: not a setting of daemon.json (port)
```

Up to 0.40 the file was `server.json`. A `server.json` left beside the
file read stops the daemon too, saying to rename it, so its settings
are never ignored without a word.

On a server or in a container, run `itero daemon --config
/etc/itero/daemon.json` with `XDG_STATE_HOME=/var/lib`, so the daemon's
state is in `/var/lib/itero/`, as itero's images do
([In a container](/guides/container/)). Setting `XDG_CONFIG_HOME=/etc`
would also find the file, but every program the daemon's actions run
inherits it, and those that keep settings there cannot write to `/etc`.

## Notifications

With `notify`, every `#notify` of the machine's workspaces is sent to
each of its channels, instead of a desktop: the same workflow runs on a
Mac and on a server. A server has no desktop, so there a channel is how
a person hears.

```json
"notify": [
  { "url": "https://ntfy.sh/itero-alerts", "format": "ntfy" },
  { "url": "$$SLACK_WEBHOOK", "format": "slack" },
  { "url": "https://ops.example.com/hook" }
]
```

A channel's `url` is an `http://` or `https://` address, or `$$NAME`: a
secret of the machine's holding one, from the `secrets` folder, else
the secret store, else the environment, read at each send (a
workspace's own secrets and `.env` are not). A webhook's URL is a secret
itself (Slack's, Discord's): give it as `$$NAME`. `format` is what the
channel takes:

| `format` | Sends |
|---|---|
| `json` (the default) | `{"title", "subtitle", "text", "url", "workspace", "workflow"}`, to any endpoint of yours |
| `ntfy` | an [ntfy](https://ntfy.sh) message, to the phone and desktop apps that follow the topic: the channel's url ends in the topic, and the notification's `url` is its Open button |
| `slack` | a Slack incoming webhook's message, with an Open button; Mattermost and Google Chat show its text, the link in it |
| `discord` | a Discord webhook's embed, its title linking to `url` |

Every channel is tried. One that does not take it (an answer other than
2xx, no connection, a secret not there) fails the step after the others,
named by its place and host, never its URL: `notify[1]
(hooks.slack.com): 404 Not Found`. A wrong channel, or a `$$NAME` with
no such secret, stops the daemon at its start, with the other problems.

A channel shows `title`, `subtitle`, `text` and `url`; `sound` and
`data` are the desktop's. A channel cannot answer: a `#notify` with
`buttons` or `wait` fails before sending anything, and
`itero.notification.clicked` never comes from one. `itero ws daemon`,
one workspace alone, reads no `daemon.json`: its `#notify` is the
desktop's.
