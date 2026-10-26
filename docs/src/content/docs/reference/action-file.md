---
title: Actions
description: What an action file gets, and what it gives back.
---

`actions/<name>` or `actions/<name>@<version>`: an executable file, no
extension, in any language its first line names.

| It gets | |
|---|---|
| environment variables | its values: the step's args, then the action's, then the run's state (`$output`, the step before's result, among them); a state value over 64 KB is on stdin only |
| stdin | the run's whole state, as JSON |
| `$ITERO_MEMORY` | a file it may keep things in, one per workflow |
| `$ITERO_WAITING_SINCE` | when it runs again after waiting |
| working folder | the workspace |
| `PATH` | the daemon's own `itero` first |

| It gives | |
|---|---|
| stdout | its result, the next step's `$output`: JSON when it is JSON, else the text, trimmed; `""` for nothing |
| exit `0` | done |
| exit `75` | not yet: the run waits for an event that agrees with what it printed (then its `$output`), or the hourly retry |
| any other exit | an error; the end of stderr says why |

A step may print nothing for 10 minutes and run for a day at most
(`quiet` and `most` in [`limits`](/reference/itero-config/#limits)); a
lost or stopped step runs again, up to 3 times. Built in, unless the workspace has an action of the name:
`#notify` (shows `$text` and `$title`), `#forget` and `#roll-log`. See
[Actions](/guides/actions/).
