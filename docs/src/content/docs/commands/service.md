---
title: itero service
description: Runs the daemon from login on, kept up by a watchdog.
---

```sh
itero service install [--interval N]
itero service status
itero service uninstall
```

| Command | Does |
|---|---|
| `install` | runs the workspace's daemon as a service (a launchd agent on macOS, a systemd user unit on Linux), from login on, kept up by a watchdog; `--interval N` as for [`itero daemon`](/commands/daemon/) |
| `status` | the service's state, and the daemon's health: its pid, passes, the last one, and its last error |
| `uninstall` | stops the service and removes it; the workspace, its journal and its log stay |

```
$ itero service install
installed: dev.aegea.itero.daemon-3f2a9c41d07e runs itero in /Users/me/home; it starts at login
API: http://127.0.0.1:8787/
log: /Users/me/home/.itero/daemon.log
it keeps this shell's PATH; install again after changing it
```

The service keeps the `PATH` of the shell that installed it, but not your
shell's other variables: keep tokens with [`itero secret`](/commands/secret/).
See [Start at login](/guides/service/).
