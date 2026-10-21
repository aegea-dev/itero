---
title: itero args
description: Sets an action's data.
---

```sh
itero args <action> '<data>'
itero args <action> --file <path>
```

Writes `args/<action>.json`: the data an action gets whatever workflow
runs it. An object, or a list of objects: then the step runs once per
item. The action's name goes without `#` or a version.

```sh
itero args imap-check '{"host": "imap.example.com", "port": 993}'
itero args imap-check '[{"user": "me@example.com",  "password": "$$ME_PASS"},
                        {"user": "you@example.com", "password": "$$YOU_PASS"}]'
```

It prints the file it wrote, which you can also edit. A value that is
exactly `"$$NAME"` is a secret's, looked up only when the step starts. See
[Args and secrets](/guides/secrets/).
