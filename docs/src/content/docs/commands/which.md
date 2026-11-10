---
title: ws which
description: Where a workflow, action or connector of a name is.
---

`itero ws which` tells you which file a name means: a workflow, an action or
a connector.

```sh
itero ws which <name> [--json]
itero ws which '#<name>'
```

Workflows, actions and connectors can sit in subfolders, grouped any way you
like; their names stay their file names. `itero ws which` says where one is:

```
$ itero ws which gönder
workflow  workflows/shop/gönder.json
action    actions/shipping/gönder
action    actions/shipping/old/gönder@0.1
$ itero ws which '#gönder'
action    actions/shipping/gönder
action    actions/shipping/old/gönder@0.1
$ itero ws which mail.received
connector connectors/mail/mail.received
$ itero ws which notify
action    built in
```

- One line for each kind of that name: an action and a workflow may share
  one, and each of an action's versions has its line.
- `#name` (in quotes, for the shell) looks for the action only.
- A name nothing has is an error, with exit 1.
- `--json` gives the same JSON as the API's
  [`GET /which/{name}`](/reference/http/#get-whichname).

itero reads the folders each time, so nothing is kept that could go stale.
[`itero ws wf list`](/commands/workflow/#list) shows each workflow's file too.
