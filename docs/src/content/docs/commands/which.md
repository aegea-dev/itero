---
title: itero which
description: Where a workflow, action or source of a name is.
---

```sh
itero which <name>
itero which '#<name>'
```

Workflows, actions and sources can sit in subfolders, grouped any way you
like; their names stay their file names. `itero which` says where one is:

```
$ itero which gönder
workflow  workflows/shop/gönder.json
action    actions/shipping/gönder
action    actions/shipping/old/gönder@0.1
$ itero which '#gönder'
action    actions/shipping/gönder
action    actions/shipping/old/gönder@0.1
$ itero which mail.received
source    sources/mail/mail.received
$ itero which notify
action    built in
```

- One line for each kind of that name: an action and a workflow may share
  one, and each of an action's versions has its line.
- `#name` (in quotes, for the shell) looks for the action only.
- A name nothing has is an error, with exit 1.

itero reads the folders each time, so nothing is kept that could go stale.
[`itero workflows`](/commands/workflows/) shows each workflow's file too.
