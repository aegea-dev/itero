---
title: token
description: The tokens a server lets in, each shown once and kept as its hash.
---

`itero token` makes the tokens other machines sign in to this server
with, where [`server.json`](/reference/server-json/#opening-the-api)'s
`listen` opens its API: a [context](/commands/context/) holds one and
sends it with every request. A token reaches the whole server, every workspace on it. Run
these on the server itself.

```sh
itero token create <name> [--json]
itero token list [--json]
itero token revoke <name>
```

## create

Makes a token and prints it once, on its own line, so a script can take
it. The server keeps only its SHA-256 hash, with its name and when it
was made: a lost token cannot be shown again; revoke it and make
another.

```
$ itero token create laptop
itero_Jp3kQ0n0kVd2yH8x2a4mGQx7bW5cZr1sT9eLfUoPqNs
laptop: shown this once, as this server keeps only its hash: itero context add, on the machine that will use it, takes it
```

A token is `itero_` and 32 random bytes in base64url, so a leaked one is
easy to search for. Its name, letters, digits, `-` and `_`, says whose it
is; two cannot share one.

## list, revoke

`list` shows each token's name and when it was made, never a token or
its hash. `revoke laptop` takes it out: it lets nobody in from the next
request on, with no restart.

## Where they are kept

In `tokens.json` in itero's state folder: `$XDG_STATE_HOME/itero/`, else
`~/.local/state/itero/`, readable by its owner alone.
