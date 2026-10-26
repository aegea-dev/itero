---
title: Args and secrets
description: Where an action's values come from, and how tokens and passwords stay out of files and the journal.
---

An action needs values to work with: a host, a list of sites, a password.
They come from the workflow, from the action's args, or from the run so
far. Passwords and tokens are kept as secrets, out of every file.

## Where a value comes from

An action's environment variables are found by name, the first in:

1. **the step's args** in the workflow: `{"#imap-check": {"args": {"host": "imap.example.com"}}}`;
2. **the action's args**, `action.<name>.args` in `itero.config.json`;
3. **the run's state**: the event that started it, the args of earlier
   steps, and `$output`, the step before's result.

In a step's args and in an action's, a value that is exactly `"$name"`
takes the state's value, and `"$$NAME"` a secret's or an environment
variable's ([Secrets](#secrets)). A step's args join the state, for the
steps after it, unless the step is `strict`
([Workflows](/guides/workflows/#a-steps-result-output)).

## Args

```sh
itero action args set imap-check '{"host": "imap.example.com", "port": 993}'
itero action args set imap-check --file accounts.json
```

Either writes `action.imap-check.args` in `itero.config.json`, which you can also edit. A list runs
the step once per item, each with its own copy of the state for the rest
of the run: the run that reaches the step ends as `split`, and one run per
item goes on from there, through the rest of the steps and the end lists,
each on its own; they never join. A step side by side cannot have a list.

```sh
itero action args set imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                                   {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

## A connector's args

A [connector](/guides/connectors/) gets its args the same way, from
`connector.<name>.args` ([`itero connector args set`](/commands/connector/#args)): an object, each value an environment variable, read
each time it starts. A connector has no run, so its values are themselves or
`"$$NAME"`, never `"$name"`.

```json
"connector": {
  "mail.pushed": { "args": { "user": "me@example.com", "password": "$$ME_PASS" } }
}
```

A secret that is not set keeps the connector from starting, and the log
says which.

## Secrets

`"$$NAME"` stands for a secret: a token, a password, an API key. It is
looked up only when an action or the connector starts, and the value never
reaches the run's state, the journal, a log or a command line. A secret in
a step's args joins the state as its name, `"$$NAME"`, so the actions of
later steps get its value too; make the step `"strict": true` to keep it to
that step. itero looks in two places, in this order:

1. **the OS secret store** (the macOS Keychain, the Secret Service on
   Linux), where [`itero secret`](/commands/secret/) keeps it;
2. **the daemon's environment variables**, when the store has no such
   name.

If both have it, the secret store wins and the environment variable is
passed over. That is easy to miss, so `itero check` and the daemon, when it
starts, warn about each such name:

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
