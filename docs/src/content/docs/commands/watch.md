---
title: watch
description: For connectors - a line per change to a file or folder, or a command run on each.
---

`itero watch` is for connectors: it prints a line each time a file or a
folder changes, which a connector turns into events.

```sh
itero watch <path>
itero watch <path> -- <command> [args...]
```

Prints a JSON line each time the file or folder changes, from the OS's own
file events, once it has been quiet for half a second:

```
$ itero watch notes.txt
{"kind":"changed","path":"notes.txt"}
```

`kind` is `created`, `changed`, `removed`, `renamed` or `rescan`. It ends
when its stdin closes, so a [connector](/guides/connectors/) running it
ends with the daemon:

```sh
#!/bin/sh
exec itero watch "$HOME/notes.txt"
```

## Running a command

A connector that reads a state (a mailbox, a folder's files) and tells
what is new gives `itero watch` a command after `--`. It sets the watch,
then runs the command once, then again after each change, one run at a
time (a change during a run gives one more):

```sh
#!/bin/sh
# check: reads the mailbox, prints a line per mail it has not told yet
if [ "$1" = check ]; then
  check
  exit
fi
exec itero watch "$mailbox" -- "$0" check
```

The first run comes after the watch is set, so nothing can change
between the two unseen: reading first and then starting `itero watch`
would miss a change in between. The command's output is the connector's
lines; it runs with no shell (name `sh -c` for one). A run that cannot
start or ends with an error ends `itero watch`, so the daemon starts the
connector again. When `itero watch` ends, a run under way ends with it.
`examples/mail-alert` works so.

A workflow does not need it: a `path` in its `on` is watched by the daemon
itself ([Watching files](/guides/connectors/#watching-files)).
