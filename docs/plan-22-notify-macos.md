# Plan 22: notifications on macOS that survive upgrades

`#notify` on macOS stopped working after `brew upgrade itero`: the run
ended in error with "Notifications are not allowed for this
application", with no prompt, though Itero was allowed before. Users
upgrade often; an upgrade must never break notifications, nor need a
step from them.

## What a probe showed (2026-10-29, macOS, Homebrew 0.18.0)

Scratch copies of Itero.app, run on the owner's Mac:

1. macOS keeps one record of notification permission per bundle id
   (`dev.aegea.itero.notifier`), following one copy of the app: it
   tracks the file itself (the copy was moved and the record followed
   it to its new name).
2. While that copy exists, any copy may notify: one at another path,
   and one re-signed ad-hoc (a new signature, the same bundle id).
   Developer ID signing is not needed for this.
3. When the recorded copy is deleted, every copy is refused, with no
   prompt. Homebrew deletes the old version's Cellar on upgrade, and
   itero ran the notifier from that versioned path: the record pointed
   at a deleted copy (0.15.0's) when 0.18.0 notified.
4. Registering a copy with LaunchServices (`lsregister -f`, as
   `LSRegisterURL`) does not fix it. Launching a copy through
   LaunchServices (`open`) does: the record moves to that copy.

### A second run (2026-10-29), building the plan

5. Three upgrades in a row (a fake Homebrew, each version's folder
   deleted after the next): the stable copy was brought up to date in
   place each time and every notification showed, with the record on an
   existing copy.
6. The record's copy deleted, a copy under `/private/tmp` was refused
   even when launched through LaunchServices; the same test with the
   stable copy in `~/Library/Application Support/itero` worked: a
   background launch (`open -g -n`) moved the record to it, and from then
   every copy notified directly.
7. `open -W` cannot wait on a notifier that ends at once ("Unable to
   block on application"): the healing launch has the notifier write its
   result to a file instead.

## Goals

- **Upgrades never break notifications**, however often, by Homebrew or
  the install script.
- **A first run asks**: the permission prompt shows, from a step run by
  the background service too.
- **A refusal says what to do**: the step's error names System
  Settings, Notifications, Itero.
- **One copy answers clicks**, not a stale or deleted one.
- **The state can be seen** before a step fails.

## The design

### One stable copy, updated in place

- itero keeps its own copy of Itero.app in
  `~/Library/Application Support/itero/Itero.app`: macOS's place for an
  app's own files, out of Finder and Launchpad, never deleted by an
  upgrade.
- Before a notification, itero compares that copy's `CFBundleVersion`
  with the Itero.app it ships with (found as today: beside `itero`, in a
  Homebrew formula's `libexec`, in `/Applications`). Missing, or older:
  the shipped one's contents are copied into the same folder (the
  folder itself stays, so the record keeps following it), under a lock
  in itero's state folder, so steps side by side copy once.
- The notifier runs from the stable copy; clicks start it.
- `ITERO_NOTIFIER` still names the notifier, for tests and dev builds:
  no stable copy then.

### Healing on refusal

- When the notifier exits with "Notifications are not allowed", itero
  launches the stable copy once through LaunchServices (`open -n -g`)
  with the same notification and `--result <file>`: the notifier writes
  `ok` or its error there as it ends, and itero waits for that file
  (watching its folder, two and a half minutes at most, for a first
  run's prompt). The record moves to it, and the notification shows, or
  the permission prompt on a first run.
- Refused again: notifications are off for Itero. The step fails with
  "notifications are off for Itero: turn them on in System Settings,
  Notifications, Itero".
- One healing a step; it never loops.

### Seeing the state

- The notifier reports macOS's setting for it without showing anything
  (allowed, off, not asked yet), through a flag of its own.
- `itero info` has a `notifications` line on macOS: that state, and the
  stable copy's version. Elsewhere, which notifier the desktop has.

## Testing

- Unit and integration tests, any OS: finding the shipped app, comparing
  versions, copying in place (the folder's identity kept), the lock with
  two steps at once, the healing decision from the notifier's exit and
  stderr, the error text. No notification center needed.
- **Upgrades in a row, by hand on macOS** (the owner's Mac, asked
  first): three builds with rising `CFBundleVersion` installed one after
  another as Homebrew would (a new Cellar folder each, the old one
  deleted), a notification after each: all shown, no prompt after the
  first. Then the record pointed at a deleted copy (as the probe did):
  the next notification heals and shows.
- Whether copying in place keeps the record is checked in that run; if
  it does not, healing covers it, once per upgrade, and the spec says
  so.
- `make verify`; Linux unchanged (`make test-linux`).

## Docs

- `#notify` in the actions guide: on macOS, the stable copy, and what a
  refusal says; the install page's macOS note.
- `itero info`'s notifications line.

## Not in this plan

- Developer ID signing and notarization (not needed for this; a choice
  for later, for Gatekeeper's warnings).
- Clicks and waiting on Linux.
