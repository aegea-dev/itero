---
title: Args and secrets
description: Where an action's values come from, and how tokens and passwords stay out of files and the journal.
---

An action needs values to work with: a host, a list of sites, a password.
It gets them as its args, written in the step that runs it. Passwords and
tokens are kept as secrets, out of every file; values that change from one
machine to another go in the workspace's `.env`.

## Where a value comes from

An action gets only its step's args, as environment variables and on its
stdin ([Actions](/guides/actions/#what-it-gets)):

```json
{ "#imap-check": { "args": { "host": "imap.example.com", "user": "$$IMAP_USER",
                             "password": "$$IMAP_PASS", "since": "$event.since" } } }
```

In args, a value that is exactly `"$event.x"`, `"$output.x"`,
`"$steps.<name>.x"` or `"$config.x"` takes that value, one inside text is
filled ([Workflows](/guides/workflows/#a-value-inside-text)), and
`"$$NAME"` is a secret, a value in `.env` or an environment variable
([below](#secrets-and-env)). `itero.config.json` holds no values for
actions or workflows: an `args` in an action's entry, or a `settings` in a
workflow's, is an error saying where the values go.

## Repeat

```sh
itero action repeat set imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                                     {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

writes `action.imap-check.repeat`: the action's step runs once per item,
each item's values on top of its step's args. Each run has its own copy
of the state for the rest of the run: the run that reaches the step ends
as `split`, and one run per item goes on from there, through the rest of
the steps and the end lists, each on its own; they never join. A step
side by side cannot repeat.

## A connector's args

A [connector](/guides/connectors/) gets its args the same way, from
`connector.<name>.args` ([`itero connector args set`](/commands/connector/#args)): an object, each value an environment variable, read
each time it starts. A connector has no run, so its values are themselves or
`"$$NAME"`, never `"$event.x"`.

```json
"connector": {
  "mail.pushed": { "args": { "user": "me@example.com", "password": "$$ME_PASS" } }
}
```

A secret that is not set keeps the connector from starting, and the log
says which.

## Secrets and `.env`

`"$$NAME"` stands for a secret (a token, a password, an API key) or a
value of yours that changes from one machine to another (a folder, an
address). It is looked up only when an action or the connector starts,
and the value never reaches the run's state, the journal, a log or a
command line; only the action whose args name it gets it. A `$$NAME` is a
whole value only: inside a text (`"Bearer $$TOKEN"`) it is refused, as
`itero check` says. itero looks in three places, in this order:

1. **the OS secret store** (the macOS Keychain, the Secret Service on
   Linux), where [`itero secret`](/commands/secret/) keeps it;
2. **the workspace's `.env`**, `NAME=value` lines (`#` starts a comment;
   quotes and `export` are allowed), for values that are not secret;
3. **the daemon's environment variables**.

Secrets go in the store, never in `.env`. If the store and the
environment both have a name, the store wins and the environment variable
is passed over. That is easy to miss, so `itero check` and the daemon,
when it starts, warn about each such name:

```
warning: $$IMAP_PASS is a secret and an environment variable: the secret is used
```

```sh
itero secret IMAP_PASS                  # asks for it, without echo
itero secret IMAP_PASS --file pass.txt  # its last newline left out
```

A name is a letter or `_`, then letters, digits or `_`. In an `on`,
`{"has": "$$NAME"}` is a condition: it holds once `NAME` is defined.

An action passes a secret to a program on its stdin, never on its command
line, where `ps` would show it. With curl:

```sh
printf 'user = "%s:%s"\n' "$user" "$password" | curl -fsS -K - "imaps://$host/INBOX"
```

`ITERO_NO_SECRET_STORE` set keeps itero away from the store: secrets then
come from the environment only, as in tests and CI.
