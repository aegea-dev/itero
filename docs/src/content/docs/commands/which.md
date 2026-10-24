---
title: which
description: Where a workflow, action or connector of a name is.
---

`itero which` tells you which file a name means: a workflow, an action or
a connector.

```sh
itero which <name> [--json]
itero which '#<name>'
```

Workflows, actions and connectors can sit in subfolders, grouped any way you
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
connector connectors/mail/mail.received
$ itero which notify
action    built in
```

- One line for each kind of that name: an action and a workflow may share
  one, and each of an action's versions has its line.
- `#name` (in quotes, for the shell) looks for the action only.
- A name nothing has is an error, with exit 1.

itero reads the folders each time, so nothing is kept that could go stale.
[`itero wf list`](/commands/workflow/#list) shows each workflow's file too.
