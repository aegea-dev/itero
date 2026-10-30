# Notifications on macOS: what we found

Findings from the owner's Mac (Apple silicon, macOS of October 2026,
Homebrew, itero 0.18.0), on 2026-10-29, while finding why `#notify`
stopped working after an upgrade. Kept for whoever changes Itero.app or
the notifier next. What itero does about it is in
[plan 22](plan-22-notify-macos.md).

## The failure

After `brew upgrade itero` to 0.18.0, a workflow's `#notify` step ended
in error with no notification and no prompt:

```
itero-notifier: cannot ask to notify: Notifications are not allowed for this application
```

Itero had been allowed to notify before, and System Settings,
Notifications, had no Itero entry at all.

## How it was found

1. `itero wf runs` in the workspace: the run ended in `error`.
2. `itero wf runs <name>`: `#ai` done, `#notify` in error with the
   message above. It is macOS's own error, passed on by
   `itero-notifier` from `requestAuthorizationWithOptions`.
3. macOS's notification settings, read without changing them:

   ```sh
   defaults export com.apple.ncprefs - | plutil -p - | grep -B2 -A8 dev.aegea.itero.notifier
   ```

   The entry for `dev.aegea.itero.notifier` had a `path`:
   `/opt/homebrew/Cellar/itero/0.15.0/libexec/Itero.app`.
4. `ls /opt/homebrew/Cellar/itero/`: only `0.18.0`. Homebrew deletes a
   version's Cellar folder on upgrade, so the copy macOS remembered was
   gone.
5. The error's path showed itero ran the notifier from
   `/opt/homebrew/Cellar/itero/0.18.0/libexec/Itero.app`: itero follows
   Homebrew's link into the versioned folder.
6. `open /opt/homebrew/opt/itero/libexec/Itero.app --args itero test`
   showed the notification, with no prompt, and the entry's `path` became
   the 0.18.0 copy's. Notifications worked again.

The system log (`log show`) had nothing useful for the moment of the
refusal.

## What macOS does

Measured with scratch copies of Itero.app (`ditto` copies, read back with
`codesign -dvvv` and the `ncprefs` entry after each step):

1. **One record per bundle id, following one copy.** macOS keeps one
   entry per bundle id, with the path of one copy of the app. It tracks
   the file: when that copy was moved, the entry's path followed it.
2. **While that copy exists, any copy may notify.** A copy at another
   path, and a copy re-signed ad-hoc (a new `CDHash`, the same bundle
   id), were both allowed when run directly. The code signature does not
   matter here, so Developer ID signing is not needed for this.
3. **When that copy is deleted, every copy is refused,** with no prompt:
   run directly, and (from some places) launched through LaunchServices
   too.
4. **Registering is not enough; launching is.** `lsregister -f` on a copy
   (what `LSRegisterURL` does) left it refused. Launching a copy through
   LaunchServices (`open`) moved the record to it, with or without `-g`
   (in the background), and with no prompt when Itero was allowed
   before.
5. **Where the copy is matters.** With the recorded copy deleted, a copy
   under `/private/tmp` was refused even when launched with `open`; a
   copy in `~/Library/Application Support/itero` launched the same way
   took the record over. Why is not known.
6. **Deleting and restoring a copy at the same path can leave it
   refused.** A copy deleted and put back (a new file, the same path,
   the same signature) was refused until it was launched with `open`
   again.
7. **`open` does not take the record over from a copy that exists.**
   With the recorded copy present, opening another copy left the record
   where it was (and both notified).

## What else to know

- **`open -W` cannot wait on a notifier that ends at once:** it prints
  "Unable to block on application (GetProcessPID() returned ...)" and
  returns. Have the app write its result to a file and wait for that.
- **Several copies with one bundle id:** Homebrew's, the install
  script's, and dev builds (`target/debug`, `target/release`) all use
  `dev.aegea.itero.notifier`. A click on a notification starts whichever
  copy LaunchServices picks. To see which:

  ```sh
  osascript -l JavaScript -e 'ObjC.import("AppKit");$.NSWorkspace.sharedWorkspace.URLForApplicationWithBundleIdentifier("dev.aegea.itero.notifier").path.js'
  ```

- A copy launched with `open` registers with LaunchServices; undo with
  `lsregister -u <app>` (in
  `/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/`).
- The `ncprefs` entry's `flags` and `auth` numbers stayed the same
  through all of this (`auth=6`); only `path` changed.
- Banners arriving within seconds of each other can stack: Notification
  Center has every notification macOS took.

## What itero does (plan 22)

- Notifies from its own copy, `~/Library/Application Support/itero/Itero.app`,
  brought up to date in place from the Itero.app it ships with when that
  is newer (`CFBundleVersion`), so no upgrade deletes the copy the record
  follows.
- When refused outright, launches that copy once through LaunchServices
  (`open -n -g`, the notifier writing its result to a file), which moves
  the record to it; refused again, the step says to turn notifications on
  in System Settings, Notifications, Itero.
- `itero info` says whether Itero may notify (`itero-notifier --status`).

## Open questions

- Why a copy under `/private/tmp` cannot take the record over.
- Whether a first run (no record at all) started from the background
  service shows the permission prompt through the background launch.
- Whether macOS ever drops the record on its own (after a long time, or
  a macOS upgrade).
