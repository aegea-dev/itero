# Plan 23: a watched folder on macOS tells a file's new contents (FSEvents)

## Why

A `path` in `on`, `itero watch` and the daemon's own watches use the OS's
file events. On Linux (inotify) a watched folder reports a file that comes,
goes or changes inside it. On macOS (kqueue) it reports only a file coming
or going: kqueue watches one vnode per descriptor, and a write to a file in
a folder changes the file, not the folder. Measured with `itero watch`
(2026-10-30): appending to a file in a watched folder gave `changed` on
Linux and nothing on macOS.

Linux stays as it is. macOS gets the same: a watched folder's direct
children coming, going or changing give `changed`.

## What FSEvents does (measured, 2026-10-30)

A throwaway probe on the FSEvents API, with file events on:

- A file's contents changing in a watched folder: an event with the
  file's path (`gelen/a.txt`, `Modified`).
- A file in a subfolder: an event too (`gelen/alt/b.txt`): FSEvents is
  recursive, so deeper events are dropped here to match Linux.
- A path that is not there yet: watched; its creation comes as
  `RootChanged` and `Created`.
- A neighbour of a watched file: no event.
- A path given through a symlink (`/tmp` for `/private/tmp`) or with
  `..`: only `RootChanged`, never the files' events. Paths must be
  resolved first.
- Flags accumulate: an append to a new file still says `Created`, a
  removal says `Created`, `Removed`, `Modified`. The kind cannot be read
  from them: whether the path is there now decides it.
- Each change came within about 0.8 seconds.

## Design

FSEvents replaces kqueue for every path on macOS: one backend for files
and folders, no descriptor per path. `src/watch/fsevents.rs` implements the
same `Backend` trait; `src/watch/kqueue.rs` goes. `mod.rs`, the debouncer,
Linux and every caller stay as they are.

- **Paths.** Each watched path is kept as named and as resolved: its
  deepest existing folder canonicalized, the rest as written. One stream
  watches the resolved paths, on its own serial dispatch queue. Changes
  are reported under the path as named, so `{path, kind}` and every
  caller are unchanged.
- **Kinds.** An event on a watched path itself: `created` when it was not
  there and is now, `removed` when it was and is not, else `changed`. An
  event on a direct child of a watched folder: `changed` for the folder.
  Deeper events: none. `MustScanSubDirs`, `UserDropped` or
  `KernelDropped`: `Rescan` for the paths it concerns (all of them when
  dropped). `RootChanged`: the path is resolved again (the stream starts
  again when that moves it), and only a coming or going is told.
- **Threads.** The stream's callback sends each raw event (path, flags,
  id) over a channel; `wait(timeout)` receives with that timeout, then
  takes what else is there and maps it. No polling.
- **Changing the paths.** `set(paths)` keeps the entries still wanted
  (with whether each was there), and starts a new stream from the last
  event id seen, so nothing in between is missed.
- **Stream settings.** File events, no defer, watch root; latency 50 ms
  (the debouncer waits half a second anyway).
- **Errors.** A stream that cannot be created or started is an
  `io::Error` naming the paths, as `naming` does for kqueue now.

## Dependencies

macOS only, under `[target.'cfg(target_os = "macos")'.dependencies]`, the
objc2 family itero already uses for notifications:

- `objc2-core-foundation ~0.3.2` (CFArray, CFString, CFDate, CFRunLoop,
  CFUUID: what the FSEvents feature needs)
- `objc2-core-services ~0.3.2` (FSEvents, dispatch2)
- `dispatch2 ~0.3.1`

All three are Zlib, Apache-2.0 or MIT, already accepted by
`packaging/licenses/about.toml`.

## Tests

`tests/watch.rs` passes unchanged on both systems. New ones there, for
both:

- a file's contents changing in a watched folder is a change;
- a file changing in a subfolder is not;
- a path through a symlink is watched.

Unit tests in `fsevents.rs`: resolving a path (a missing tail kept, a
symlink followed) and mapping raw events to changes (the kinds above,
subfolders dropped, the dropped flags as `Rescan`).

`make verify` and `make test-linux` pass.

## Docs

The connectors guide's table (added with the docs on path events) gets
`changed` in its macOS column for a file's contents changing: both systems
then read the same, and the table can go, its sentence kept.

## Not here

Naming the file that changed (a path event that says which file) is its
own change: FSEvents and inotify both know it, and this design keeps the
event shape as it is.
