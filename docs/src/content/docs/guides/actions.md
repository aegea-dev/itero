---
title: Actions
description: An executable file that reads its values from the environment, prints its result, and says with its exit code how it went.
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
# hands $order to the carrier; prints its tracking number
curl -fsS "https://carrier.example.com/ship?order=$order" | jq -r '.id'
```

## What it gets

- **Its values, as environment variables**: `$order`, `$host`. A value is
  the first found in the step's args, the action's own args, then the
  run's state ([Args and secrets](/guides/secrets/)), where `$output` is
  the step before's result. A value that is not text arrives as JSON
  text. A name that cannot be an environment variable, and a state value
  over 64 KB (a whole web page, say), are on stdin only: the system would
  refuse to start the action with them. A value that big in a step's
  args is an error naming it.
- **The run's whole state, as JSON on stdin.**
- **`$ITERO_MEMORY`**: a file it may read and write, kept between runs, one
  per workflow (for "the last mail seen").
- **`$ITERO_WAITING_SINCE`**, when it runs again after waiting.

It runs in the workspace, with the daemon's own `itero` first on its
`PATH`, so an action can send an event with `itero emit`.

## What it says

What it prints is its result, the next step's `$output`: read as JSON
when it is JSON (`3`, `true`, `[1, 2]`, `{"count": 3}`), else kept as
text, trimmed. A plain command's output is enough (`ls | wc -l`), and an
object carries several values. Printing nothing is `""`. Nothing it prints
joins the state by itself: the workflow passes on what it keeps, as a
later step's args ([Workflows](/guides/workflows/#a-steps-result-output)).
Its exit code:

| Exit | Means |
|---|---|
| `0` | done |
| `75` | not yet: the run waits. The step runs again every hour, or when an event comes that agrees with what it printed (an object, such as `{"payment_ref": "pay-1"}`); then that event is its `$output` |
| anything else | an error: the end of its stderr says why |

A step may print nothing for 10 minutes, `quiet` in
[`limits`](/reference/itero-config/#limits). Anything it prints, on stdout
or stderr, starts the 10 minutes again, so a long step that tells how it
goes (`creating the cluster...` on stderr) runs as long as it needs, up to
`most` (a day). Past either, or when the daemon stops while it runs, it
runs again, up to 3 times; then it is an error, `quiet for 10m` or
`ran for 1d`.

Its last line on stderr is also its progress: `itero wf runs` shows it
beside a running run, and the run's story on the step, so a step of hours
can say how far it is:

```sh
#!/bin/sh
for f in data/*.csv; do
  n=$((n + 1)); echo "[$n] $f" >&2
  ./import "$f"
done
```

```
$ itero wf runs
0199...  calm-river  import  running  2026-10-27T10:00:00Z  -  #import: [21] data/orders-21.csv
```

## Versions

A version is part of the file name: `notify@0.2`, `notify@0.10`. `#notify`
is the highest, compared as numbers; `#notify@0.2` pins one. Args and
secrets are the action's, whatever the version.

## Built in

`#notify` shows a desktop notification: through Itero.app on macOS
(`osascript` without it), `notify-send` on Linux.

| Value | Is |
|---|---|
| `$text` | what it says; needed |
| `$title` | its title; `itero` when not set |
| `$subtitle` | a line under the title (on Linux, the text's first line) |
| `$sound` | `default`, or a macOS sound's name such as `Glass` (those in `/System/Library/Sounds`); on Linux, a sound theme's name |
| `$url` | opened by a click on it, on macOS through Itero.app: http or https, anything else an error, as it may come from an event |
| `$buttons` | a list of 1 to 4 labels, shown as its buttons (macOS); the step then waits for an answer |
| `$wait` | `true`: the step waits for a click on it, with no buttons |
| `$data` | an object a click on it passes on, as `data` |

```json
{ "#notify": { "args": { "title": "Backup", "subtitle": "disk 2", "text": "$why", "sound": "Basso" } } }
```

On macOS, each click on a notification from `#notify` is an event,
`itero.notification.clicked`, in the workspace that showed it: `answer` is
the button's label, `open` for a click on the notification itself, or
`dismissed` when one that waits is closed without one. A workflow can start
on it ([Events from the daemon](/guides/maintenance/#events-from-the-daemon)),
and a step with `$buttons` or `$wait` waits for it; then the click is
its `$output`, and the run goes on:

```json
{ "steps": [
    "#plan",
    { "#notify": { "args": { "title": "Deploy?", "text": "$output",
                             "buttons": ["Deploy", "Cancel"] } } },
    { "#apply": { "on": [{ "$output.answer": "Deploy" }] } }
] }
```

A run nobody answers waits until its workflow's `timeout`. On Linux a
notification has no buttons and tells no clicks.

How long a notification stays is yours to set, in System Settings,
Notifications, itero: Banners go away by themselves, Alerts stay until
dismissed.

`#forget` forgets ended runs and their events past `keep`, and
`#roll-log` rolls the log; the built-in workflows use them
([The daemon's own workflows](/guides/maintenance/)).

A workspace action of the same name is used instead of a built-in one.

## Other languages, and their dependencies

The first line picks the language. A Python action:

```python
#!/usr/bin/env python3
import json, os, sys
state = json.load(sys.stdin)
print(state["price"] * state["count"])
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

`examples/languages` has this action (`python/slugify`) and another
(`python/parse-date`, with `dateparser`), each with a workflow around it,
beside the Node, Bun and Docker ones below.

### JavaScript with Bun

[Bun](https://bun.sh) installs a package an action imports, at the version
the import names, with no `package.json`:

```js
#!/usr/bin/env -S bun --install=force
import { z } from "zod@3.23.8";
```

`--install=force` matters: Bun turns this off when a `node_modules` folder
sits in any folder above the action, and one in your home folder is enough.
`examples/languages`'s `bun/check-order` validates an order with zod; a bad
one ends the run in error, its why listing the fields.

### Node, and anything with a project folder

A program with its own `package.json`, `pyproject.toml` or build keeps its
project in a folder, and the action starts it:

```
actions/node/links              #!/bin/sh
                                cd "$(dirname "$0")/links-app" && exec node main.js
actions/node/links-app/         package.json, package-lock.json, node_modules/, main.js
```

That is `examples/languages`'s `node/links`: a page's title and links, with
cheerio.

Only executable files are actions. The rest of the folder (`package.json`,
`main.js`, a `README.md`) is left alone, and `node_modules` and hidden folders
(`.venv`) are never looked into. Install it once yourself (`npm install`),
in that folder. itero starts the action in the workspace; this one moves
into its project before it starts the program.

### Docker

A program that comes as an image runs in it, with Docker pulling it the
first time. `examples/languages`'s `docker/to-markdown` turns a page's HTML,
the step before's result, into Markdown with pandoc, reading it from the
state on stdin:

```sh
#!/bin/sh
jq -r '.output' |
  docker run --rm -i pandoc/minimal:3.11.0 -f html -t gfm-raw_html --wrap=none |
  jq -Rrs '.[0:20000]'
```

Pin the image's version, as here, so a new release does not change what
the action does. A value goes into the container as `-e name` (it takes
the value from the action's environment, never from the command line) or
on stdin.

## When something it needs is missing

itero finds what an action needs from the action itself, with no list to
keep up to date:

- **When a step fails,** it reads the exit code and stderr, and the run's
  `why` (in `itero wf runs <name>` and `itero action errors`) starts with what was
  missing:

  | The action printed | The run's why |
  |---|---|
  | exit 127, `docker: not found` (sh, bash, zsh, env) | `needs docker: not found on PATH` |
  | `ModuleNotFoundError: No module named 'requests'` | `needs the Python module requests` |
  | `Cannot find module 'axios'` (Node) | `needs the module axios` |
  | `Cannot find package 'axios'` (Node, Bun) | `needs the package axios` |
  | a first line naming a program that is not there | `needs /opt/python3 (the first line of …): not found` |

- **Before anything runs,** [`itero check`](/commands/check/) lists, under
  each action, what its first line needs and is not on the `PATH` actions
  run with (`needs bun: not found on PATH`). For a shell script it also
  reads the commands it runs, and lists the missing ones as `may need`: a
  hint, since a script can build a command at run time or guard one with
  `command -v`.

Versions (`python >= 3.11`) and whether a service is up (Docker's daemon, a
model server) are left to the action: it checks them itself, and exits 75 to
try again later.
