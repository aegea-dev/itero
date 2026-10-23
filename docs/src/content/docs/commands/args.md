---
title: itero args
description: Sets an action's or a source's data.
---

```sh
itero args <name> '<data>'
itero args <name> --file <path>
```

Writes `action.<name>.args` in `itero.config.json`: the data an action
gets whatever workflow runs it. An object, or a list of objects: then the
step runs once per item. The action's name goes without `#` or a version.

```sh
itero args imap-check '{"host": "imap.example.com", "port": 993}'
itero args imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                        {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

When the workspace has a source of that name, it writes
`source.<name>.args` instead: an object, as a source runs once.

```sh
itero args mail.pushed '{"user": "me@example.com", "password": "$$ME_PASS"}'
```

It prints the file it wrote, which you can also edit. A value that is
exactly `"$$NAME"` is a secret's, looked up only when the step or the
source starts. See [Args and secrets](/guides/secrets/).
