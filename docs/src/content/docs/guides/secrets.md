---
title: Args and secrets
description: Where an action's values come from, and how tokens and passwords stay out of files and the journal.
---

## Where a value comes from

An action's environment variables are found by name, the first in:

1. **the step's data** in the workflow: `{"#imap-check": {"host": "imap.example.com"}}`;
2. **the action's args**, `action.<name>.args` in `itero.config.json`;
3. **the run's state**: the message that started it, messages since, and
   what earlier steps printed.

In the step's data and in args, a value that is exactly `"$name"` takes
the state's value, and `"$$NAME"` a secret's.

## Args

```sh
itero args imap-check '{"host": "imap.example.com", "port": 993}'
itero args imap-check --file accounts.json
```

Either writes `action.imap-check.args` in `itero.config.json`, which you can also edit. A list runs
the step once per item, each with its own copy of the state for the rest
of the run: the run that reaches the step ends as `split`, and one run per
item goes on from there, through the rest of the steps and the end lists,
each on its own; they never join. A step side by side cannot have a list.

```sh
itero args imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                        {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

## A connector's args

A [connector](/guides/messages/#connectors) gets its args the same way, from
`connector.<name>.args`: an object, each value an environment variable, read
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
looked up in the OS secret store (the macOS Keychain, the Secret Service
on Linux), then in the daemon's environment, only when the step or the
connector starts:
the value never reaches the run's state, the journal, a log or a command
line.

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
