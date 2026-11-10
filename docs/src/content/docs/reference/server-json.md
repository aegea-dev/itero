---
title: server.json
description: How the machine's daemon serves, read at its start.
---

`server.json` holds the settings of the machine's daemon
([`itero daemon --all`](/commands/daemon/), which
[`itero service`](/commands/service/) runs). It is in itero's config
folder: `$XDG_CONFIG_HOME/itero/server.json`, else
`~/.config/itero/server.json`. `itero daemon --all --config <path>` reads
another file instead.

```json
{ "port": 8787,
  "listen": "0.0.0.0:8443",
  "tls": { "cert": "/etc/itero/tls/cert.pem", "key": "/etc/itero/tls/key.pem" } }
```

| Setting | Is |
|---|---|
| `port` | the API's port on `127.0.0.1`, from 1 to 65535. Without it, the port the daemon kept from its last start, else one the OS gives, which it keeps |
| `listen` | an address and a port where the API is opened too, beside the local one: there every request needs a [token](/commands/token/). Without it, the API answers on `127.0.0.1` only |
| `tls` | `cert`, the certificate's PEM file (its chain after it), and `key`, its private key's: `listen` serves HTTPS with them. Without it, plain HTTP |

## Opening the API

With `listen`, the daemon serves the same routes on a second listener.
There, every request needs `Authorization: Bearer <token>`, a token
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
seconds to finish its handshake. The daemon warns when the key file is
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
$ itero daemon --all
error: /Users/me/.config/itero/server.json: port: a number from 1 to 65535, not "8787"
/Users/me/.config/itero/server.json: prot: not a setting of server.json (port)
```

On a server or in a container, set `XDG_CONFIG_HOME=/etc` and
`XDG_STATE_HOME=/var/lib`: the file is then `/etc/itero/server.json`, and
the daemon's state is in `/var/lib/itero/`.
