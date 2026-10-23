---
title: itero config
description: Sets a workflow's settings, or shows them.
---

```sh
itero config <workflow> '<settings>'
itero config <workflow> --file <path>
itero config <workflow>
```

Writes `workflow.<workflow>.settings` in `itero.config.json`: the workflow's settings, the values its
runs start with. An object. Without settings, it prints them.

```sh
itero config sites '{"sites": ["https://example.com", "http://localhost:3000"]}'
itero config disk '{"min_free": 15}'
```

A run's state starts with its workflow's settings, then the message's data
on top: a step names one as `"$sites"`, an action gets it as `$sites`.
The rest of the file is kept as it was. To turn a workflow off, or bound
it in time, its entry says `"off": true`, `from` or `until`
([itero.config.json](/reference/itero-config/)). See
[Settings](/guides/workflows/#settings).
