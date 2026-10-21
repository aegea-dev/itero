---
title: itero watch
description: For sources - a line per change to a file or folder.
---

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
when its stdin closes, so a [source](/guides/messages/#sources) running it
ends with the daemon:

```sh
#!/bin/sh
exec itero watch "$HOME/notes.txt"
```

A workflow does not need it: a `path` in its `on` is watched by the daemon
itself ([Watching files](/guides/messages/#watching-files)).
