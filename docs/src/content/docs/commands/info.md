---
title: info
description: The machine's daemon - its version, port and workspaces.
---

`itero info` tells you about this machine's daemon, from any folder: the
question after an upgrade is which itero it runs. For one workspace, see
[`itero ws info`](/commands/workspace/#info).

```sh
itero info [--json]
itero info --context <name>
```

With [`--context`](/commands/context/), it tells the daemon of that
server instead: its address, pid, itero version and start, and each
workspace.

This itero's version, then the daemon that runs the machine's list: its
pid, port, itero version and start, and each workspace with its passes and
its last error. Then [itero-ai](/ai/), when it has started, and whether
notifications can be shown: on macOS, macOS's setting for Itero (`allowed`,
`off`, `not asked`) and the copy of Itero.app that notifies; on Linux, the
notifier it uses.

```
$ itero info
itero 0.22.0
daemon: pid 4211, port 8787, itero 0.21.0, started 2026-10-31T09:12:03Z
this itero is 0.22.0: itero service restart runs it
  misty-lake: 214 passes, last 2026-10-31T10:02:11Z
  quiet-river: 214 passes, last 2026-10-31T10:02:11Z, last error: no such file
ai: running unsloth/Qwen3.5-4B-GGUF
notifications: allowed, Itero.app 24 in /Applications/Itero.app
```

With no daemon answering it says so, and exits 0:

```
$ itero info
itero 0.22.0
daemon: none answering on port 8787
itero service status says why
notifications: allowed, Itero.app 24 in /Applications/Itero.app
```

See [One itero per machine](/guides/machine/) and
[Upgrading](/start/install/#upgrading).
