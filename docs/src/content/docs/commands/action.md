---
title: action
description: Your actions, the ones that break, and their repeat.
---

`itero action` shows your actions and sets their repeat. An action's
args are written in the step that runs it ([Args and secrets](/guides/secrets/)). Actions
are where things usually go wrong, so `itero action errors` tells you which
ones broke your runs, and why.

```sh
itero action list
itero action errors [<action>] [--limit N]
itero action repeat get <action>
itero action repeat set <action> '<items>'
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

Every action: its name, its file (one per version), and whether it
repeats. The built-in ones are listed too.

```
$ itero action list
#check-payment   actions/check-payment
#hello           actions/hello
#imap-check      actions/imap-check      repeat
#notify          built in
#forget          built in
#roll-log        built in
```

## errors

Which actions broke runs: how often, when last, and why.

```
$ itero action errors
#send-mail   2 errors   last 1s ago   exit 1: smtp: connection refused
```

With an action's name, its failures, newest first, each with the run to
look into ([`itero wf runs <name>`](/commands/workflow/#runs)):

```
$ itero action errors send-mail
1s ago   order   sunny-moss    exit 1: smtp: connection refused
1s ago   order   fast-canyon   exit 1: smtp: connection refused
```

`why` is the action's exit code and the end of what it printed on stderr,
where an error message or a traceback usually is. `--limit N` shows at most
`N` (20). itero keeps how each run ended, so a step that failed and then
passed when it ran again does not count.

## repeat

An action's repeat: its step runs once per item, each item's values on
top of its step's args. Kept in `itero.config.json` under `action.<name>.repeat`:
a list of one or more objects.

```sh
itero action repeat set imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                                     {"user": "you@example.com", "password": "$$YOU_PASS"}]'
itero action repeat get imap-check
```

`set` prints the file it wrote. See
[Args and secrets](/guides/secrets/#repeat).
