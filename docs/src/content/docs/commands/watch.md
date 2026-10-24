---
title: watch
description: For connectors - a line per change to a file or folder.
---

`itero watch` is for connectors: it prints a line each time a file or a
folder changes, which a connector turns into events.

```sh
itero watch <path>
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

A workflow does not need it: a `path` in its `on` is watched by the daemon
itself ([Watching files](/guides/connectors/#watching-files)).
