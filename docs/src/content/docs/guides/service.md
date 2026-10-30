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
unit on Linux) that starts at login and runs a watchdog, which keeps
[`itero daemon --all`](/guides/machine/) running: one daemon for every
workspace on the list. If the watchdog itself ends, the service starts it
again.

The service keeps the `PATH` of the shell that installed it, so actions find
the same programs you do. After changing your `PATH`, install it again.

## The watchdog

The watchdog starts the daemon, then asks its `/health` every 10 seconds.
It restarts the daemon when it:

- exits, for any reason;
- does not answer 3 checks in a row. A daemon just started has 30
  seconds to answer before a missed check counts.

A workspace whose pass runs past its limit (10 minutes) does not restart
the daemon: that workspace is shown as `stuck`, and the others run on.

Between restarts it waits 1 second, then twice as long each time, up to a
minute, so a daemon that cannot start does not start over and over. Once
a daemon has run for 5 minutes, the wait is back to 1 second.

Stopping the watchdog (`SIGTERM` or `SIGINT`, as the service or a `kill`
sends) stops its daemon too, as `itero stop` does, so no daemon is left
running without one.

What each workspace does is in its `.itero/itero.log`
([The log, IDs and names](/guides/log/)). What the daemon and the watchdog
print themselves is in `~/.local/state/itero/daemon.log`: each start of
the daemon with its process id, why it was restarted, and what a crash
leaves.

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
