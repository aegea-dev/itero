---
title: ws action
description: Your actions, and the ones that break.
---

`itero ws ac` (`itero ws action` in full) shows your actions. An action's
args, and its `repeat`, are written in the step that runs it
([Secrets and environment variables](/guides/secrets/), [Repeat](/guides/workflows/#repeat-rounds)). Actions
are where things usually go wrong, so `itero ws ac errors` tells you which
ones broke your runs, and why.

```sh
itero ws ac list
itero ws ac errors [<action>] [--limit N]
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

Every action: its name and its file (one per version). The built-in
ones are listed too.

```
$ itero ws ac list
#check-payment   actions/check-payment
#hello           actions/hello
#imap-check      actions/imap-check
#notify          built in
#forget          built in
#roll-log        built in
#ai              built in
#find-models     built in
```

## errors

Which actions broke runs: how often, when last, and why.

```
$ itero ws ac errors
#send-mail   2 errors   last 1s ago   exit 1: smtp: connection refused
```

With an action's name, its failures, newest first, each with the run to
look into ([`itero ws wf runs <name>`](/commands/workflow/#runs)):

```
$ itero ws ac errors send-mail
1s ago   order   sunny-moss    exit 1: smtp: connection refused
1s ago   order   fast-canyon   exit 1: smtp: connection refused
```

`why` is the action's exit code and the end of what it printed on stderr,
where an error message or a traceback usually is. `--limit N` shows at most
`N` (20). itero keeps how each run ended, so a step that failed and then
passed when it ran again does not count.
