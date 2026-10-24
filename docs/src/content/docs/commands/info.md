---
title: info
description: The workspace and its daemon - IDs, names, limits.
---

`itero info` tells you about the workspace you are in and the daemon
running it, if one is.

```sh
itero info [--json]
```

The workspace: its id, name, when it was made, its folder, the itero that
made it, what its manifest says it is, and its `limits`. Then the daemon running it, when
one answers on the port: its id, name, start, pid, host, itero version and port.

```
$ itero info
workspace
  id 01a16000-8c56-7202-87a3-b6b17d7a725d
  name warm-otter
  created 2026-10-21T19:46:51Z
  path /Users/me/home
  itero 0.11.0
  limits
    workflows 100
    runs 1000
    steps 8
    space 1073741824
    keep 1day
    log 10485760
    logs 3
daemon: not running (no answer on port 8787)
```

See [`itero.config.json`](/reference/itero-config/) and
[The log, IDs and names](/guides/log/).
