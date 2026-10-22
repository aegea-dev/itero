---
title: itero workflows
description: Each workflow, its runs and the last; whether the daemon runs.
---

```sh
itero workflows
```

Each workflow on a line: its file, what starts it, its runs so far, how the
last one went and when, and whether it is `off` or internal. The last line says
whether this workspace's daemon is running, and how many messages wait
for one.

```
$ itero workflows
page-summary   workflows/web/page-summary.json   on website.requested                runs 2   last: done 4m ago
order          workflows/shop/order.json         on order.placed                     runs 3   last: done 2h ago
keep           built in                          on itero.space.full, at 0 * * * *   runs 1   last: done 40m ago   internal
logs           built in                          on itero.log.full                   runs 0   last: none           internal
daemon: not running, 1 message waiting
```

The first place to look when nothing seems to happen: a message waiting
and no daemon running means [`itero daemon`](/commands/daemon/) is not
running.
