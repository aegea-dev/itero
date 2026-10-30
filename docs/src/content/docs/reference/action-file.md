---
title: Actions
description: What an action file gets, and what it gives back.
---

`actions/<name>` or `actions/<name>@<version>`: an executable file, no
extension, in any language its first line names.

| It gets | |
|---|---|
| environment variables | its step's args, nothing else; a value over 64 KB is on stdin only; `$event`, the event it waited for, when it runs again after waiting |
| stdin | `{"args": {...}}`, with `"event"` when it waited for one |
| `$ITERO_MEMORY` | a file it may keep things in, one per workflow |
| `$ITERO_WAITING_SINCE` | when it runs again after waiting |
| working folder | the workspace |
| `PATH` | the daemon's own `itero` first |

| It gives | |
|---|---|
| stdout | its result, the next step's `$output`: JSON when it is JSON, else the text, trimmed; `""` for nothing |
| stderr | while it runs, its last line is its progress, shown by `itero wf runs` (cut to 200 characters); when it fails, its end says why |
| exit `0` | done |
| exit `75` | not yet: the run waits for an event that agrees with what it printed (then its `$output`), or the hourly retry |
| any other exit | an error; the end of stderr says why |

A step may print nothing for 10 minutes and run for a day at most
(`quiet` and `most` in [`limits`](/reference/itero-config/#limits)); a
lost or stopped step runs again, up to 3 times. Built in, unless the workspace has an action of the name:
`#notify` (shows its args `text` and `title`), `#forget` and `#roll-log`. See
[Actions](/guides/actions/).
