---
title: action
description: Your actions, the ones that break, and their args.
---

`itero action` shows your actions and sets the data they get. Actions
are where things usually go wrong, so `itero action errors` tells you which
ones broke your runs, and why.

```sh
itero action list
itero action errors [<action>] [--limit N]
itero action args get <action>
itero action args set <action> '<args>'
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

Every action: its name, its file (one per version), and whether it has
args. The built-in ones are listed too.

```
$ itero action list
#check-payment   actions/check-payment
#hello           actions/hello
#send-mail       actions/send-mail       args
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

## args

An action's args: the data it gets whatever workflow runs it, kept in
`itero.config.json` under `action.<name>.args`. The name goes without `#`
or a version.

```sh
itero action args set imap-check '{"host": "imap.example.com", "port": 993}'
itero action args set imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                                   {"user": "you@example.com", "password": "$$YOU_PASS"}]'
itero action args get imap-check
```

Args are an object, or a list of objects: then the step runs once per item.
`set` prints the file it wrote, which you can also edit. A value that is
exactly `"$$NAME"` is a secret's or an environment variable's, looked up
only when the step starts. See [Args and secrets](/guides/secrets/).
