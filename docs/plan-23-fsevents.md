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

## Design: two modes, each with its own tool

A further probe (2026-10-30) found that FSEvents cannot watch a single file
whose name has a composed letter (`ü` stored as one character, as Rust,
the shell and most tools write it): it reports only `RootChanged`, never
the file's writes. A folder works in every case, any file's name in it
included. FSEvents is also recursive, so watching a single file through
its folder would bring in its whole tree. Linux's inotify does both
modes with one tool; macOS needs one tool for each:

- **File mode: kqueue, as today.** Exactly that file, any name, no noise.
  `src/watch/kqueue.rs` stays as it is.
- **Folder mode: kqueue for the folder's own events (made, removed,
  entries coming and going), and FSEvents for a file in it changing.**
  Direct children only, as inotify.

The mode follows what is on disk: a path that is a folder now is in
folder mode; a file, or a path not there yet, in file mode, and a path
that comes as a folder moves to folder mode (and back when it goes).
Nothing a user writes changes.

- **One wait.** The kqueue backend owns an FSEvents stream for its
  folders (`src/watch/fsevents.rs`), on its own serial dispatch queue.
  The stream's callback sends each raw event (path, flags, id) over a
  channel and wakes the kqueue wait with a user event (`EVFILT_USER`).
  No polling, no extra thread.
- **Matching.** Folders are given to FSEvents as real paths
  (canonicalized: through a symlink FSEvents gives no file events). An
  event is compared in decomposed form (NFD) on both sides, so a name is
  the same whether it was written composed or not: an event on a direct
  child of a watched folder is `changed` for that folder, told under the
  folder as named; the folder's own events and deeper ones are left out
  (kqueue tells the folder's own). `MustScanSubDirs`, `UserDropped` or
  `KernelDropped`: `Rescan` for the folders concerned.
- **Keeping up.** After `set` and after each wait, the backend gives the
  stream the folders its paths are now; a different list starts a new
  stream from the last event id seen, so nothing in between is missed.
- **Stream settings.** File events, no defer; latency 50 ms (the
  debouncer waits half a second anyway).
- **Errors.** A stream that cannot be made or started is an `io::Error`
  naming the folders.

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

Unit tests in `fsevents.rs`: a name composed or not compares equal;
mapping raw events to changes (a direct child is `changed` for its
folder, the folder itself and deeper ones nothing, the dropped flags
`Rescan`).

`make verify` and `make test-linux` pass.

## Docs

The connectors guide's table (added with the docs on path events) gets
`changed` in its macOS column for a file's contents changing: both systems
then read the same, and the table can go, its sentence kept.

## Not here

Naming the file that changed (a path event that says which file) is its
own change: FSEvents and inotify both know it, and this design keeps the
event shape as it is.
