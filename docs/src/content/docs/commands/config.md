---
title: itero config
description: Sets a workflow's settings, or shows them.
---

```sh
itero config <workflow> '<settings>'
itero config <workflow> --file <path>
itero config <workflow>
```

Writes `config/<workflow>.json`: the workflow's settings, the values its
runs start with. An object. Without settings, it prints them.

```sh
itero config sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
itero config disk '{"min_free": 15}'
itero config order-mail '{"off": true}'
```

A run's state starts with its workflow's settings, then the message's data
on top: a step names one as `"$sites"`, an action gets it as `$sites`.
`off`, `from` and `until` are switches, not values: they turn a workflow
off or bound it in time, and win over its `meta`. They are checked as
they are written. See [Settings](/guides/workflows/#settings-config).
