---
title: Start at login
description: Run the machine's daemon as a per-user service, kept up by a watchdog.
---

`itero daemon` runs until you stop it. To run your workspaces from login
on, add each one to the machine's list, then install the service once:

```bash
itero workspace add        # in each workspace
itero service install      # once, from anywhere
```

This registers a per-user service (a launchd agent on macOS, a systemd user
unit on Linux) that starts at login and runs a watchdog. The watchdog runs
[`itero daemon --all`](/guides/machine/), which runs every workspace on
the list, and restarts it when it:

- exits;
- stops answering `/health` (3 checks in a row, 10 seconds apart).

A workspace whose pass runs past its limit (10 minutes) does not restart
the daemon: it is shown as `stuck`, and the others run on.

What each workspace does is in its `.itero/itero.log`
([The log, IDs and names](/guides/log/)); what the daemon and the
watchdog print themselves, such as what a crash leaves, is in
`~/.local/state/itero/daemon.log`.

The service keeps the `PATH` of the shell that installed it, so actions find
the same programs you do. After changing your `PATH`, install it again.

## Status

```bash
itero service status
```

shows the service's state and the daemon's health: its process id, its
port, and each workspace's passes and last error.
[`itero workspace list`](/commands/workspace/) shows each workspace's
state.

## After an upgrade

```bash
itero service restart
```

runs every workspace on the new itero.

## Remove it

```bash
itero service uninstall
```

stops the service and removes it. The workspaces, their journals and
their logs stay, and so does the machine's list.
