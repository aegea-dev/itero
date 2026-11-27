---
title: Actions
description: An executable file that gets its args as environment variables and on stdin, prints its result, and says with its exit code how it went.
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
# hands $order (its arg) to the carrier; prints its tracking number
curl -fsS "https://carrier.example.com/ship?order=$order" | jq -r '.id'
```

## What it gets

- **Its args, as environment variables**: `$order`, `$host`, the args
  its step gives (`{ "#ship": { "args": { "order": "$event.order" } } }`),
  filled when the step starts ([Workflows](/guides/workflows/#args-what-a-step-gives-its-action)).
  A value that is not text arrives as JSON text; a `null` arrives as no
  variable. A name that cannot be an environment variable, and a value
  over 64 KB (a whole web page, say), are on stdin only: the system would
  refuse to start the action with them. Secrets in its args arrive as
  their values ([Secrets and environment variables](/guides/secrets/)).
- **Its args again, as JSON on stdin**: `{"args": {...}}`, each with its
  type (`1042` stays a number).
- **The event it waited for**, when it runs again after waiting: as
  `$event` (JSON text) and in its stdin, `{"args": {...}, "event": {...}}`.
- Nothing else: not the event that started the run, not another step's
  result, unless its step gives it as an arg.
- **`$ITERO_MEMORY`**: a file it may read and write, kept between runs, one
  per workflow (for "the last mail seen").
- **`$ITERO_WAITING_SINCE`**, when it runs again after waiting.

It runs in the workspace, and inherits the daemon's environment
([What every action sees](/guides/secrets/#what-every-action-sees)), with
the daemon's own `itero` first on its `PATH`, so an action can send an event: `itero ws emit` to its own
workspace, [`itero emit`](/commands/emit/#every-workspace) to every
workspace on the machine's list.

## What it says

What it prints is its result, the next step's `$output`: read as JSON
when it is JSON (`3`, `true`, `[1, 2]`, `{"count": 3}`), else kept as
text, trimmed. A plain command's output is enough (`ls | wc -l`), and an
object carries several values. Printing nothing is `""`. Later steps read
it as `$steps.<name>` ([Workflows](/guides/workflows/#steps-every-step-by-its-name)).
Its exit code:

| Exit | Means |
|---|---|
| `0` | done |
| `75` | not yet: the run waits. The step runs again every hour, or when an event comes that agrees with what it printed (an object, such as `{"payment_ref": "pay-1"}`); then it gets that event as `$event` |
| anything else | an error: the end of its stderr says why |

A step may print nothing for 10 minutes, `quiet` in
[`limits`](/reference/itero-config/#limits). Anything it prints, on stdout
or stderr, starts the 10 minutes again, so a long step that tells how it
goes (`creating the cluster...` on stderr) runs as long as it needs, up to
`most` (a day). Past either, or when the daemon stops while it runs, it
runs again, up to 3 times; then it is an error, `quiet for 10m` or
`ran for 1d`. It never runs again while a copy of it still runs: one a
daemon that stopped left behind is waited for. A step whose action thinks long before it prints, or runs
for days, gets its own on the step, in its workflow:
`{ "#train-model": { "limits": { "quiet": "1h", "most": "3d" } } }`.

Its last line on stderr is also its progress: `itero ws wf runs` shows it
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
$ itero ws wf runs
0199...  calm-river  import  running  2026-10-27T10:00:00Z  -  #import: [21] data/orders-21.csv
```

## Versions

A version is part of the file name: `notify@0.2`, `notify@0.10`. `#notify`
is the highest, compared as numbers; `#notify@0.2` pins one. Its entry
in `itero.config.json` is the action's, whatever the version.

## Built in

`#notify` shows a desktop notification: through Itero.app on macOS
(`osascript` without it), `notify-send` on Linux. On a machine whose
[`daemon.json`](/reference/daemon-json/#notifications) lists `notify`
channels (a server, which has no desktop), it sends its `title`,
`subtitle`, `text` and `url` to them instead; the same workflow runs on
both. `buttons`, `wait` and clicks need a desktop: a channel cannot
answer.

| Arg | Is |
|---|---|
| `text` | what it says; needed |
| `title` | its title; `itero` when not given |
| `subtitle` | a line under the title (on Linux, the text's first line) |
| `sound` | `default`, or a macOS sound's name such as `Glass` (those in `/System/Library/Sounds`); on Linux, a sound theme's name |
| `url` | opened by a click on it, on macOS through Itero.app: http or https, anything else an error, as it may come from an event |
| `buttons` | a list of 1 to 4 labels, shown as its buttons (macOS); the step then waits for an answer |
| `wait` | `true`: the step waits for a click on it, with no buttons |
| `data` | an object a click on it passes on, as `data` |

```json
{ "#notify": { "args": { "title": "Backup", "subtitle": "disk 2", "text": "$output.why", "sound": "Basso" } } }
```

On macOS, each click on a notification from `#notify` is an event,
`itero.notification.clicked`, in the workspace that showed it: `answer` is
the button's label, `open` for a click on the notification itself, or
`dismissed` when one that waits is closed without one. A workflow can start
on it ([Events from the daemon](/guides/maintenance/#events-from-the-daemon)),
and a step with `buttons` or `wait` waits for it; then the click is
its result, `$output` to the next step, and the run goes on:

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
Notifications, Itero: Banners go away by themselves, Alerts stay until
dismissed.

On macOS, notifications come from itero's own copy of Itero.app, in
`~/Library/Application Support/itero/`, which itero brings up to date
from the one it ships with: an upgrade needs nothing from you, and
macOS's permission stays. If macOS refuses Itero anyway, `#notify` asks
it once more as an app (macOS may then ask you to allow it); refused
again, the step fails saying to turn notifications on in System Settings,
Notifications, Itero. [`itero ws info`](/commands/workspace/#info) says whether Itero
may notify.

`#forget` forgets ended runs and their events past `keep`, `#roll-log`
rolls the log, `#find-models` finds the models on the machine, and
`#record-load` keeps a `#load` result (its arg `load`) in the day's file;
the built-in workflows use them
([The daemon's own workflows](/guides/maintenance/)). `#ai` asks a
model, on this machine or behind a URL ([#ai in a workflow](/ai/workflows/)).
`#browser` opens, reads, clicks through, inspects, photographs and
saves pages in Chrome ([The browser](/guides/browser/)).

A workspace action of the same name is used instead of a built-in one.

### `#load`

`#load` measures the machine and itero at once, on macOS and Linux alike,
and never waits: each part is a read of the system, the journal or the
files. The built-in workflow `load` runs it every minute
([Measuring the machine and itero](/guides/maintenance/#measuring-the-machine-and-itero)),
and any workflow can run it as a step.

| Arg | Is |
|---|---|
| `measure` | a list of the parts to measure; every part when not given. A part it does not know is an error of `itero ws check` and the daemon, with the one you may have meant |

```json
{ "#load": { "args": { "measure": ["memory", "disk"] } } }
```

Its result has `at` (when it measured), each part asked for, and `took`
(what measuring took). Sizes are bytes; CPU time is milliseconds.

| Part | Fields |
|---|---|
| `os` | `name` (`macos`, `linux`), `version`, `arch` (`aarch64`, `x86_64`), `host` |
| `cpu` | `count` (the CPUs itero may use), `load` (the 1, 5 and 15 minute load averages) |
| `memory` | `total`, `used` (by apps and the system, as Activity Monitor counts it on macOS), `free` (the rest: `total` minus `used`), `swap_used` |
| `disk` | `total`, `free` (what may still be written), of the disk the workspace is on |
| `daemon` | `pid`, `cpu_ms` (its CPU time so far), `memory` (resident), `files` (open), `workers` (steps running, this one apart), `workers_memory` (theirs and the warm copies', with all they started), `warm` (warm copies), `machine` (`true` under the machine's daemon, `itero daemon`) |
| `queue` | `events` (not handled yet, the daemon's own among them), `running`, `waiting` (runs) |
| `runs` | `last_minute` (runs started), `pick_up_ms` (`p50`, `p95`: from an event's saving to its run's start, in the last minute; `null` with none) |
| `journal` | `size` (the journal), `wal` (its write-ahead log), `itero` (all of `.itero/`) |
| `took` | `ms`, `cpu_ms`: what this `#load` cost |

Under the machine's daemon, `daemon` is that one daemon, shared by every
workspace on the list; `queue`, `runs` and `journal` are the workspace's
own. `journal` measures every file in `.itero/`: the one part to leave out
of a frequent `#load` that does not need it.

## Other languages, and their dependencies

The first line picks the language. A Python action:

```python
#!/usr/bin/env python3
import json, sys
args = json.load(sys.stdin)["args"]
print(args["price"] * args["count"])
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
its arg `page`, into Markdown with pandoc, reading it from stdin:

```sh
#!/bin/sh
jq -r '.args.page' |
  docker run --rm -i pandoc/minimal:3.11.0 -f html -t gfm-raw_html --wrap=none |
  jq -Rrs '.[0:20000]'
```

Pin the image's version, as here, so a new release does not change what
the action does. A value goes into the container as `-e name` (it takes
the value from the action's environment, never from the command line) or
on stdin.

## Warm actions

A Python or Node action starts its interpreter and loads its libraries at
every step, and for a fast action that start is most of the step. A step with
`"warm": true` runs its action in a copy itero keeps loaded: the
interpreter starts once, the imports load once, and each step runs the
file again in it.

```json
"steps": [
  { "#slugify": { "args": { "title": "$event.title" }, "warm": true } }
]
```

The action's file stays as it is: it gets its args, prints its result and
exits with its code, as any action does. itero runs it again for each step
with that step's args, as a fresh process would, so its result, its exit
code, an exception and its progress are what they would be.

Measured on a Mac (10 CPUs), a workflow of steps of one small Python
action:

| steps | one run, fresh | one run, warm | runs a second, fresh | runs a second, warm |
|---|---|---|---|---|
| 1 | 28 ms | 8 ms | 225 | 615 |
| 3 | 91 ms | 26 ms | 85 | 261 |
| 10 | 291 ms | 84 ms | 25 | 80 |

A Node action, a step of the same kind, measured the same way: 195 runs a
second fresh, 946 warm; 3 steps 73 and 435.

Which actions can be warm, by their first line: Node ones
(`#!/usr/bin/env node`, Node 18 or later), and Python ones:
`#!/usr/bin/env python3` (or `python`, `python3.12`, a path), a
[uv script](#python-with-uv) (`#!/usr/bin/env -S uv run --script`), and a
Python project's file naming its project:

```python
#!/usr/bin/env -S uv run --project actions/py/links-app python
```

For a uv script or project, itero runs `uv sync` first and uses its
environment. On any other action, or a built-in one, `warm` is an error,
from [`itero ws check`](/commands/check/) and when the step runs.

What starts fresh at each step: its environment, its folder (the
workspace), its arguments, its stdin and the file's own variables. What
stays: the libraries it imported, and whatever they keep. An action that
keeps something in a library between calls, or leaves work for its exit
(`atexit`, `process.on("exit")`), should not be warm. A process it starts
gets an empty stdin.

A Node action's step ends when what it started has ended (its timers, its
requests), as `node` would exit then. Its stdin comes as `process.stdin`
and as reads of fd 0 (`fs.readFileSync(0)`, `fs.readSync(0, ...)`); a raw
stream opened on fd 0 reads nothing. A CommonJS file and an ES module both
work; what either loads stays loaded. An uncaught error ends the step with
exit 1, printed as `node` prints it; the end of it, which the run keeps as
the step's why, lacks the frames of node's own loader that a fresh `node`
shows below the action's.

itero runs as many copies of a warm action as its steps need at once, one
per CPU at most and never more than [`limits.steps`](/reference/itero-config/#limits),
started only when needed. Each copy holds its interpreter and libraries in
memory. A copy is replaced when the action's file changes, when a step
leaves a thread or a process running in it, or when it ends; one with no
step for 10 minutes stops. Steps of the same action without `warm` run as
before, each in a fresh process. The log says when a copy starts and ends
(`warm.started`, `warm.ended`).

`examples/languages` has two: `post`, three Python steps all warm
(`slugify`, `parse-date`, `post-path`), and `order-flow`, four languages
in a row: Bun checks an order, Python prices it warm, Node writes a
receipt line warm and a shell script keeps it.

## When something it needs is missing

itero finds what an action needs from the action itself, with no list to
keep up to date:

- **When a step fails,** it reads the exit code and stderr, and the run's
  `why` (in `itero ws wf runs <name>` and `itero ws ac errors`) starts with what was
  missing:

  | The action printed | The run's why |
  |---|---|
  | exit 127, `docker: not found` (sh, bash, zsh, env) | `needs docker: not found on PATH` |
  | `ModuleNotFoundError: No module named 'requests'` | `needs the Python module requests` |
  | `Cannot find module 'axios'` (Node) | `needs the module axios` |
  | `Cannot find package 'axios'` (Node, Bun) | `needs the package axios` |
  | a first line naming a program that is not there | `needs /opt/python3 (the first line of …): not found` |

- **Before anything runs,** [`itero ws check`](/commands/check/) lists, under
  each action, what its first line needs and is not on the `PATH` actions
  run with (`needs bun: not found on PATH`). For a shell script it also
  reads the commands it runs, and lists the missing ones as `may need`: a
  hint, since a script can build a command at run time or guard one with
  `command -v`.

Versions (`python >= 3.11`) and whether a service is up (Docker's daemon, a
model server) are left to the action: it checks them itself, and exits 75 to
try again later.
