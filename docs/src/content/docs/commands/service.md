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
itero service install [--port <port>] [--yes]
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

An itero before one per machine installed a service per workspace
(`dev.aegea.itero.daemon-<hash>`). `install` finds them, says which
workspaces they ran, and asks; on a yes it removes each one and adds its
workspace to the machine's list, then installs the machine's service.
`--yes` does it without asking, and is needed where there is no terminal
to ask on.

```
$ itero service install
itero's services from before, one per workspace:
  /Users/me/home: onto the machine's list
  /Users/me/mail: onto the machine's list
remove them, and add their workspaces to the machine's list? [y/N] y
removed dev.aegea.itero.daemon-3f2a9c41d07e
added quiet-river (/Users/me/home)
removed dev.aegea.itero.daemon-77a42351f78b
added misty-lake (/Users/me/mail)
installed: dev.aegea.itero.daemon runs every workspace on this machine's list (itero workspace list); it starts at login
```

The service keeps the `PATH` of the shell that installed it, but not your
shell's other variables: keep tokens with [`itero secret`](/commands/secret/).
See [Start at login](/guides/service/).
