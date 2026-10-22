---
title: Actions
description: An executable file that reads its values from the environment, prints JSON, and says with its exit code how it went.
---

An action is an executable file in `actions/` or any subfolder of it, in
any language its first line names (`#!/bin/sh`, `#!/usr/bin/env python3`).
It has no extension and no description file; its name is its file name,
unique among the actions (`actions/mail/imap-check` is `#imap-check`).
Files that are not executable (a `README.md`, a `package.json`) can sit
beside actions; they are not actions.

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
  arrives as JSON text. A name that cannot be an environment variable,
  and a state value over 64 KB (a whole web page, say), are on stdin
  only: the system would refuse to start the action with them. A value
  that big in a step's data or args is an error naming it.
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

## Other languages, and their dependencies

The first line picks the language. A Python action:

```python
#!/usr/bin/env python3
import json, os, sys
state = json.load(sys.stdin)
print(json.dumps({"total": state["price"] * state["count"]}))
```

An action that uses packages should bring them, so it runs the same on any
machine with its tool, and nobody installs anything by hand. itero installs
nothing itself; each language's tool does it:

### Python with uv

[uv](https://docs.astral.sh/uv/) runs a script whose dependencies are
written at its top (PEP 723, a Python standard). It installs them on the
first run, in a cache of its own:

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = ["python-slugify>=8"]
# ///
import json, os
from slugify import slugify
print(json.dumps({"slug": slugify(os.environ["title"])}))
```

`examples/slug` is this action, with a workflow around it.

### JavaScript with Bun

[Bun](https://bun.sh) installs a package an action imports when there is no
`node_modules`; a version in the import pins it:

```js
#!/usr/bin/env bun
import slugify from "slugify@1.6.6";
console.log(JSON.stringify({ slug: slugify(process.env.title ?? "", { lower: true }) }));
```

### Node, and anything with a project folder

A program with its own `package.json`, `pyproject.toml` or build keeps its
project in a folder, and the action starts it:

```
actions/web/screenshot          #!/bin/sh
                                cd "$(dirname "$0")/screenshot-app" && exec node main.js
actions/web/screenshot-app/     package.json, node_modules/, main.js
```

Only executable files are actions. The rest of the folder (`package.json`,
`main.js`, a `README.md`) is left alone, and `node_modules` and hidden folders
(`.venv`) are never looked into. Install it once yourself (`npm install`),
in that folder. itero starts the action in the workspace; this one moves
into its project before it starts the program.

### Docker

A program that needs its own image runs it itself: `exec docker run --rm
-i -e order my/image`.

When a program or a package is missing, the step ends in error with what the
action printed, such as `docker: not found` or Python's `No module named …`.
