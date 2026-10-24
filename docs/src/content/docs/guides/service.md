---
title: Start at login
description: Run a workspace's daemon as a per-user service, kept up by a watchdog.
---

`itero daemon` runs until you stop it. To run a workspace's daemon from
login on, install it as a service, in the workspace:

```bash
itero service install
```

This registers a per-user service (a launchd agent on macOS, a systemd user
unit on Linux) that starts at login and runs a watchdog in the workspace.
The watchdog runs `itero daemon` and restarts it when it:

- exits;
- stops answering `/health` (3 checks in a row, 10 seconds apart);
- hangs: one pass runs for 10 minutes (an idle daemon makes no passes,
  and is fine).

One service per workspace, each on its workspace's port. What happens is
in `.itero/itero.log` ([The log, IDs and names](/guides/log/));
`.itero/daemon.log` keeps only what the daemon printed itself, such as
what a crash leaves.

The service keeps the `PATH` of the shell that installed it, so actions find
the same programs you do. After changing your `PATH`, install it again.

## Status

```bash
itero service status
```

shows the service's state and the daemon's health: its process id, its
passes, the last one, and its last error.

## Remove it

```bash
itero service uninstall
```

stops the service and removes it. The workspace, its journal and its log
stay.
