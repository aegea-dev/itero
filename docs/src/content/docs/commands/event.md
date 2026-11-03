---
title: ws event
description: The events no daemon has handled yet, and taking one back.
---

`itero ws ev` (`itero ws event` in full) shows the events sent to the
workspace that no daemon has handled yet, and cancels one. A running
daemon handles an event in a moment, so they are mostly events held back
(no room under the workspace's [`limits.runs`](/reference/itero-config/#limits))
or waiting for a daemon to start.

```sh
itero ws ev list
itero ws ev cancel <id>
```

Each one takes `--json`, to print JSON for scripts instead of a table.

## list

The events not yet handled, oldest first: id, type, time and data.

```
$ itero ws ev list
01a1a2f0-5c3e-7a10-9d3b-2f6e0c8a4b11   order.placed   2026-11-03T21:04:10Z   {"order":43}
```

## cancel

Takes back an event no daemon has handled yet, by the id
[`itero ws emit`](/commands/emit/) printed: it starts and reaches nothing,
and the journal notes it `cancelled by command`.

```
$ itero ws ev cancel 01a1a2f0-5c3e-7a10-9d3b-2f6e0c8a4b11
cancelled 01a1a2f0-5c3e-7a10-9d3b-2f6e0c8a4b11
```

An event a daemon has handled already is refused, saying what it did
(the runs it started); a daemon taking it at the same moment never also
starts runs for it. To stop a run it started, see
[`itero ws wf stop`](/commands/workflow/#stop).
