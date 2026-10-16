---
title: Secrets
description: Tokens and passwords in actions, kept out of workflow files and the journal.
---

`${env.NAME}` in an action's `with` stands for a secret: a token, a
password, an API key. Unlike other templates it is not filled in when the
run decides what to do, so the value never reaches the journal. The daemon
fills it in only when the job starts.

```json
{ "action": "http.request@1",
  "with": { "url": "https://api.example.com/items",
            "headers": { "Authorization": "Bearer ${env.EXAMPLE_TOKEN}" } } }
```

## Where the value comes from

The daemon looks the name up in the OS secret store first, under the
service `itero`: the Keychain on macOS, the Secret Service on Linux. Then
in its own environment. A secret that is in neither fails the action
(`secret EXAMPLE_TOKEN is not set`).

On macOS, keep one in the Keychain:

```bash
security add-generic-password -s itero -a EXAMPLE_TOKEN -w
```

`-w` last makes `security` ask for the value, so it stays out of your
shell's history. On Linux without a secret store (a server, say), set the
secret in the daemon's environment instead.

A name is a letter or `_`, then letters, digits or `_`.

`ITERO_NO_SECRET_STORE` set in the daemon's environment keeps it away from
the store: secrets then come from the environment only. A store that cannot
be read (locked, not running) is reported once in the log, and the daemon
goes on with its environment.

## In shell lines

`sh@1` takes values only through `env`, so a secret reaches a shell line as
an environment variable:

```json
{ "action": "sh@1",
  "with": { "run": "curl -fsS -H \"Authorization: Bearer $TOKEN\" https://api.example.com/ping",
            "env": { "TOKEN": "${env.EXAMPLE_TOKEN}" } } }
```
