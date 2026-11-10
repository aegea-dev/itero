---
title: Secrets and environment variables
description: Where to keep a password, a token or a value that changes from one machine to another, and how an action gets it.
---

An action needs values to work with: a host, a list of sites, a password.
It gets them as its args, written in the step that runs it. Passwords and
tokens are kept as secrets, out of every file; values that change from one
machine to another go in the workspace's `.env`.

## In a minute

Keep a value in one of three places:

| Where | For | How |
|---|---|---|
| the OS secret store | passwords, tokens, API keys | `itero secret API_KEY`, which asks for it |
| the workspace's `.env` | values that change from one machine to another | a line `API_URL=https://api.example.com` |
| the daemon's environment | what its shell has | `export API_URL=...` before `itero daemon` |

Name it as `$$NAME` in a step's args, whichever place it is in:

```json
{ "#upload": { "args": { "key": "$$API_KEY", "url": "$$API_URL" } } }
```

The action reads them as its args, `$key` and `$url`. `actions/upload`:

```sh
#!/bin/sh
printf 'header = "Authorization: Bearer %s"\n' "$key" |
  curl -fsS -K - --data-binary @report.json "$url"
```

The workflow file holds only the names, so it can be shared. A service
started at login has no variables from your shell but `PATH`
([Start at login](/guides/service/)): keep values in the store or `.env`.

## Where a value comes from

An action gets only its step's args, as environment variables and on its
stdin ([Actions](/guides/actions/#what-it-gets)):

```json
{ "#imap-check": { "args": { "host": "imap.example.com", "user": "$$IMAP_USER",
                             "password": "$$IMAP_PASS", "since": "$event.since" } } }
```

In args, a value that is exactly `"$event.x"`, `"$output.x"`,
`"$steps.<name>.x"`, `"$config.x"` or `"$count"` takes that value, one inside text is
filled ([Workflows](/guides/workflows/#a-value-inside-text)), and
`"$$NAME"` is a secret, a value in `.env` or an environment variable
([below](#secrets-and-env)). `itero.config.json` holds no values for
actions, workflows or connectors: an `args` or a `repeat` in an action's
entry, an `args` in a connector's, or a `settings` in a workflow's, is an
error saying where it goes.

## A connector's args

A [connector](/guides/connectors/) gets its args from the workflows that
listen to it: each `on` entry gives them beside its event, and they reach
the connector as environment variables, read each time it starts. A
connector has no run, so its values are themselves or `"$$NAME"`, never
`"$event.x"`.

```json
{ "on": [{ "event": "mail.pushed",
           "args": { "user": "me@example.com", "password": "$$ME_PASS" } }] }
```

A secret that is not set keeps the connector from starting, and the log
says which. Workflows giving different args run the connector once for
each set ([Connectors](/guides/connectors/#one-connector-many-sets-of-args)).

## Secrets and `.env`

`"$$NAME"` stands for a secret (a token, a password, an API key) or a
value of yours that changes from one machine to another (a folder, an
address). It is looked up only when an action or the connector starts,
and the value never reaches the run's state, the journal, a log or a
command line; only the action whose args name it gets it. A `$$NAME` is a
whole value only: inside a text (`"Bearer $$TOKEN"`) it is refused, as
`itero ws check` says. It counts only where a step's args write it: an
event or a step's result holding `$$NAME` holds that text, so nothing from
outside can name a secret. itero looks in four places, in this order:

1. **the workspace's own secrets** in the OS secret store (the macOS
   Keychain, the Secret Service on Linux), where
   [`itero ws secret`](/commands/secret/#one-workspaces-secret) keeps them;
2. **the machine's secrets** in the same store, for every workspace, where
   [`itero secret`](/commands/secret/) keeps them;
3. **the workspace's `.env`**, `NAME=value` lines (`#` starts a comment;
   quotes and `export` are allowed): your own values, secret or not, kept
   out of git (`itero ws add` adds it to the workspace's `.gitignore`);
4. **the daemon's environment variables**.

A secret may live in the store or in `.env`: `.env` is a local file of
yours, never synced or shared, and the store keeps it off the disk. If the
store and the environment both have a name, the store wins and the environment variable
is passed over. That is easy to miss, so `itero ws check` and the daemon,
when it starts, warn about each such name:

```
warning: $$IMAP_PASS is a secret and an environment variable: the secret is used
```

```sh
itero secret IMAP_PASS                  # asks for it, without echo
itero secret IMAP_PASS --file pass.txt  # its last newline left out
itero ws secret IMAP_PASS               # for this workspace alone
```

A name is a letter or `_`, then letters, digits or `_`. In an `on`,
`{"has": "$$NAME"}` is a condition: it holds once `NAME` is defined.

An action passes a secret to a program on its stdin, never on its command
line, where `ps` would show it. With curl:

```sh
printf 'user = "%s:%s"\n' "$user" "$password" | curl -fsS -K - "imaps://$host/INBOX"
```

A model's server takes its key the same way, as the `key` of an `#ai`
step: `"key": "$$OPENAI_API_KEY"`
([A model behind a URL](/ai/workflows/#a-model-behind-a-url)). itero
sends it only to that step's `url`.

## What every action sees

An action also inherits the daemon's own environment: `PATH`, `HOME` and
whatever else its shell exported. So a value you `export` before
`itero daemon` is visible to every action, named in its args or not. A
value in the store or in `.env` reaches only the actions whose args name
it, so prefer those for anything sensitive.

`$$NAME` gives an action the value; what the action then does with it is
its own. What it prints becomes its result, which is saved in the journal
and the log, so an action should never print a secret.

## On a server

A server or a container often has no OS secret store. There a secret
comes from the platform, as Kubernetes Secrets, Docker secrets and
systemd credentials give them:

- **its environment**, as on any machine;
- **a folder of files**, one per secret, named after it, which
  [`server.json`](/reference/server-json/)'s `secrets` names:

```json
{ "secrets": "/run/secrets" }
```

`$$API_KEY` then reads `/run/secrets/API_KEY` (its last newline left
out), after the OS store and `.env`, before the environment. A file
`/run/secrets/<workspace>.API_KEY` is that workspace's own, and comes
first.

On a server that has an OS secret store, keep one from your machine
over a [context](/commands/context/):

```sh
itero secret API_KEY --context prod            # for every workspace on it
itero ws shop secret API_KEY --context prod    # for shop alone
```

A server without one says so, and where it reads secrets from instead.
A secret is never sent over plain HTTP to another machine: give the
context an `https://` address. Secrets never travel with
[`itero ws apply`](/commands/workspace/#apply).

## Tests and CI

`ITERO_NO_SECRET_STORE` set keeps itero away from the store: a `$$NAME`
then comes from `.env` and the environment only, as in tests and CI.
