---
title: Actions
description: An executable file that reads its values from the environment, prints JSON, and says with its exit code how it went.
---

An action is an executable file in `actions/`, in any language its first
line names (`#!/bin/sh`, `#!/usr/bin/env python3`). It has no extension
and no description file; its name is its file name.

`actions/ship`:

```sh
#!/bin/sh
# hands $order to the carrier
curl -fsS "https://carrier.example.com/ship?order=$order" | jq '{tracking: .id}'
```

## What it gets

- **Its values, as environment variables**: `$order`, `$host`. A value is
  the first found in the step's data, the action's args, then the run's
  state ([Args and secrets](/guides/secrets/)). A value that is not text
  arrives as JSON text; a name that cannot be an environment variable is
  on stdin only.
- **The run's whole state, as JSON on stdin.**
- **`$ITERO_MEMORY`**: a file it may read and write, kept between runs, one
  per workflow (for "the last mail seen").
- **`$ITERO_WAITING_SINCE`**, when it runs again after waiting.

It runs in the workspace, with the daemon's own `itero` first on its
`PATH`, so an action can send a message with `itero emit`.

## What it says

It prints one JSON object, whose fields join the run's state; a field set
to `null` is removed. Printing nothing adds nothing. Its exit code:

| Exit | Means |
|---|---|
| `0` | done |
| `75` | not yet: what it printed joins the state, and the run waits; the step runs again when a matching message comes, or every hour |
| anything else | an error: the end of its stderr says why |

A step may take 10 minutes. Past that, or when the daemon stops while it
runs, it runs again, up to 3 times; then it is an error.

## Versions

A version is part of the file name: `notify@0.2`, `notify@0.10`. `#notify`
is the highest, compared as numbers; `#notify@0.2` pins one. Args and
secrets are the action's, whatever the version.

## Built in

`#notify` shows `$text` (and `$title`) as a desktop notification: through
Itero.app on macOS (`osascript` without it), `notify-send` on Linux.

`#forget` forgets ended runs and their messages past `keep`, and
`#roll-log` rolls the log; the built-in workflows use them
([The daemon's own workflows](/guides/maintenance/)).

A workspace action of the same name is used instead of a built-in one.

## Other languages, and Docker

The first line picks the language. A Python action:

```python
#!/usr/bin/env python3
import json, os, sys
state = json.load(sys.stdin)
print(json.dumps({"total": state["price"] * state["count"]}))
```

A program that needs its own image runs it itself: `exec docker run --rm
-i -e order my/image`.
