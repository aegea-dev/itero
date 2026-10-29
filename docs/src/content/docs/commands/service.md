---
title: service
description: Runs the machine's daemon from login on, kept up by a watchdog.
---

`itero service` runs the machine's daemon for you: one
[`itero daemon --all`](/commands/daemon/), for every workspace on the
machine's list ([One itero per machine](/guides/machine/)). It starts
when you log in, and is restarted if it stops. Its commands work from
any folder.

```sh
itero service install [--port <port>]
itero service status [--json]
itero service start | stop | restart
itero service uninstall
```

| Command | Does |
|---|---|
| `install` | runs the machine's daemon as a service (a launchd agent on macOS, a systemd user unit on Linux), from login on, kept up by a watchdog; `--port` fixes its port |
| `status` | the service's state, and the daemon's health: its pid, its port, each workspace's passes and last error |
| `start` | starts the installed service |
| `stop` | stops it; it starts again at your next login |
| `restart` | stops and starts it: after an upgrade, every workspace runs on the new itero |
| `uninstall` | stops the service and removes it; the workspaces, their journals and logs stay |

```
$ itero service install --port 8787
installed: dev.aegea.itero.daemon runs every workspace on this machine's list (itero workspace list); it starts at login
API: http://127.0.0.1:8787/
log: /Users/me/.local/state/itero/daemon.log
it keeps this shell's PATH; install again after changing it
```

## The services from before

An itero before 0.18.0 installed a service per workspace
(`dev.aegea.itero.daemon-<hash>`). `install` removes them and says which
workspace each ran; add those workspaces to the list with
[`itero workspace add`](/commands/workspace/) to run them again.

```
$ itero service install
removed dev.aegea.itero.daemon-3f2a9c41d07e, which ran /Users/me/home alone: itero workspace add there puts it on the list
installed: dev.aegea.itero.daemon runs every workspace on this machine's list (itero workspace list); it starts at login
```

The service keeps the `PATH` of the shell that installed it, but not your
shell's other variables: keep tokens with [`itero secret`](/commands/secret/).
See [Start at login](/guides/service/).
