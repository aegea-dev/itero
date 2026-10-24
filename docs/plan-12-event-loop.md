# Plan 12: the daemon waits on events

`itero daemon` runs a pass every `--interval` seconds (default 1); a
message, a connector or a watched path wakes it sooner. Idle, it still
wakes every second: it opens the journal, rereads every workflow and
walks `.itero/` to measure it, about 9 ms a pass, so close to 1% of a core
all day, more as the workspace grows, and a laptop never idles deeply. Its
1-second wait also stops counting while the machine sleeps, so a schedule
due during sleep runs late by the sleep.

The daemon becomes an event loop, as libuv's: it blocks until something
happens or the next thing is due, and uses no CPU while it waits. Nothing
wakes it on a period.

## Spec

- **What wakes it:** a message (`POST /events`, `itero emit`, a click on a
  notification), a line from a connector, a watched path, an edit to a
  workflow or `itero.config.json`, and one alarm set to the next due time.
  Nothing else. Actions and connectors are read when they run, so an edit
  to one needs no wake.
- **Messages from other processes:** `itero emit` and a click (`itero
  notification-clicked`) save the message in the journal themselves, which
  no wake told the daemon about; the 1-second pass found it. Now they
  touch `.itero/wake` once it is saved, and the file watcher wakes the
  daemon. The daemon never writes that file, so it cannot wake itself.
- **The next due time** is the earliest of: each schedule's next slot
  after its last; the earliest timer of a waiting run (its hourly retry,
  its deadline); the earliest `--reply` due again. A workflow that is off
  or past its `until` has no slots; one before its `from`, its first slot
  from then. With none, there is no alarm and the daemon waits for a
  message or an edit. (The built-in `keep` runs hourly, so an idle
  workspace wakes once an hour unless it is turned off.)
- **Work left over:** a pass that stops with work still to do (the cap of
  messages a pass takes) runs again at once; one that defers a message
  because its worker was lost runs again after a second, as now. A pass
  that fails, or a next due time that cannot be read, is tried again a
  minute later.
- **The alarm is wall-clock time in the kernel:** on macOS a kqueue
  `EVFILT_TIMER` with `NOTE_ABSOLUTE | NOTE_MACH_CONTINUOUS_TIME`, on Linux
  a `timerfd` on `CLOCK_REALTIME` with `TFD_TIMER_ABSTIME`. It counts the
  time the machine sleeps: a time that passes during sleep fires as soon
  as the machine runs again, not later by the sleep. (Measured on macOS:
  across 54 s and 203 s of sleep this timer fired on time, an interval
  timer late by the whole sleep.)
- **Edits take effect when they happen:** the daemon rereads the
  workflows on a wake, as each pass does now; `itero.config.json`, the
  workflow folders, their subfolders and each workflow file are watched
  for that.
- **The file watcher** waits without a timeout when nothing is pending.
  Today it rereads every workflow each second to move its watches; now
  only an edit does. A watched path whose folder is not there yet is
  watched for from the nearest folder that is.
- **The watchdog** restarts a daemon whose pass has been running past its
  limit (`/health` gains `busy_since`, the start of the running pass, null
  when idle), or one that stops answering, as now. An idle daemon makes
  no passes, so "no pass for 10 minutes" goes. `/health` keeps `passes`
  and `last_pass`.
- **`--interval` goes,** from `itero daemon` and `itero service install`.
  A service installed before passes it, so starting it fails with an
  error saying to run `itero service install` again. A breaking change:
  the release bumps the minor version, with a Breaking note.
- **Unchanged:** how workflows, actions and connectors are written and
  run; the threads (the HTTP API on tokio, the file watcher in
  kqueue/inotify, a thread per connector) already wait in the kernel.

## Tasks

1. `schedule::next(cron, last, tz)`: a schedule's next slot. Tests in
   local time and across a daylight saving change.
2. `daemon::next_due`: the earliest of schedules, timers and replies,
   from the workflows and the journal; `None` with none. Tests: each
   source alone, the earliest of several, none.
3. The alarm, `src/engine/alarm.rs`: a thread blocked on the kernel timer
   (kqueue on macOS, timerfd on Linux), set to a time or to none; when it
   fires it wakes the daemon on the channel every other wake uses. Tests:
   fires at a time a fraction of a second away; set again, the old time
   does not fire; none never fires.
4. `run_loop`: after a pass, set the alarm to `next_due` (now, after a
   second, or none for leftover work as above) and wait on the channel
   with no timeout. `--interval` goes from the daemon and the service.
5. Watch `workflows/`, `actions/`, `connectors/` and `itero.config.json`
   with the file watcher that already watches workflows' paths; a change
   wakes the daemon. The watcher waits without a timeout.
6. `busy_since` in `/health` and the watchdog's rule.
7. Tests of the whole daemon: idle for 3 s, it makes no pass; a schedule,
   a waiting run's deadline and a `--reply` retry each fire at their time
   with no other wake; editing a workflow wakes it; `itero emit` wakes it.
   `make verify` and `make test-linux`.
8. Docs: the daemon and service pages lose `--interval`; the daemon page
   says what wakes it; the HTTP page lists `busy_since`. A behaviour
   change, so the same PR.

## Decided

- **No rewrite onto tokio.** libuv's model is one wait in the kernel for
  everything; itero's threads already wait there, each on its own source,
  and wake the daemon on one channel. Only the 1-second timeout was
  periodic. tokio stays where it is, for the HTTP API.
- **No safety-net wake.** A source left out of the next due time would
  never fire, so each has a test instead.
- **Wall-clock alarms,** not intervals: schedules are wall-clock times, and
  an interval stops counting during sleep.
