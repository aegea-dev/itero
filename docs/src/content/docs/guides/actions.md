---
title: Actions
description: The built-in actions, and actions of your own in any language.
---

An action is named with its version, like `notify@1`: a workflow keeps
working the same way when a newer version comes.

## Built-in actions

| Action | Does |
|---|---|
| `command@1` | runs a program and its arguments, no shell |
| `sh@1` | runs a shell line (`sh -c`), for pipelines |
| `http.request@1` | an HTTP request |
| `notify@1` | a desktop notification |
| `memory.set@1` | sets keys in the workflow's memory |
| `json.pick@1` | picks a value out of the previous output |
| `event.emit@1` | sends an event to a run |
| `event.publish@1` | publishes an event for the workspace: written as a `publish` step |

### command@1

```json
{ "action": "command@1", "with": { "run": ["git", "-C", "notes", "pull", "--ff-only"] } }
```

`run` is the program and its arguments, run in the workspace folder. Each
argument is one argument, whatever it holds: no shell reads it.

### sh@1

```json
{ "action": "sh@1",
  "with": { "run": "grep -c \"$WORD\" notes.md", "env": { "WORD": "${input.word}" } } }
```

`run` is fixed text: a `${...}` template in it is refused when the workflow
is checked. Values reach the shell only as environment variables, through
`env`, so a value holding `; rm -rf ~` stays text.

### Output of a program

`command@1`, `sh@1` and your own actions give:

| Key | Is |
|---|---|
| `output.exit` | the exit code |
| `output.stdout`, `output.stderr` | what it printed, as text |
| `output.lines` | stdout's lines |
| `output.json` | stdout, when it parses as JSON |

`command@1` and `sh@1` fail on a non-zero exit unless their `expect` says
otherwise (`"expect": "output.exit <= 1"` for `grep`, which exits 1 when
nothing matches).

### http.request@1

```json
{ "action": "http.request@1",
  "with": { "url": "https://api.example.com/items", "method": "POST",
            "headers": { "Authorization": "Bearer ${env.EXAMPLE_TOKEN}" },
            "json": { "name": "${input.name}" } } }
```

| Key | Is |
|---|---|
| `url` | the address (needed) |
| `method` | `GET` unless given |
| `headers` | an object of header names and values |
| `json` or `body` | a JSON body, or text; not both |
| `max_bytes` | the largest response accepted, 10 MB unless given |

Every request carries an `Idempotency-Key` header, the same when the job
runs again, unless `headers` sets one. The output is `status`, `headers`,
`text`, and `json` when the body parses. A status outside 200 to 299 fails
the action unless `expect` says otherwise.

### notify@1

```json
{ "action": "notify@1", "with": { "title": "Mail", "text": "${steps.check.json.summary}" } }
```

On macOS through `Itero.app` (or `osascript` without it); on Linux through
`notify-send`, `gdbus` or `kdialog`. See
[Install](/start/install/#notifications).

### memory.set@1, json.pick@1, event.emit@1

These run inside the manager and touch nothing outside the decision.

- `memory.set@1` takes any keys in `with` and sets them in the workflow's
  memory when the chain succeeds.
- `json.pick@1` takes `path`, like `json.items.0.name`, and gives that value
  of the previous output.
- `event.emit@1` takes `type`, `target` (a run's id) and `data`, and sends
  the event when the chain succeeds.
- `event.publish@1` is what a `{ "publish": "<type>", "data": {...} }` step
  is: the event goes to the workspace when the chain succeeds (see
  [Publish and subscribe](/guides/apps/#publish-and-subscribe)).

## Your own actions

An action of your own is a folder in one of the workspace's action folders,
with an `action.json`:

```json title="actions/mail/action.json"
{
  "name": "mail",
  "version": 1,
  "type": "executable",
  "limits": { "timeout": "30s" },
  "targets": { "check": { "executable": "./check.sh" } }
}
```

A workflow names it `<name>.<target>@<version>`: `mail.check@1`. `name`
matches the folder's name and cannot be a built-in's (`command`, `sh`,
`http`, `notify`, `memory`, `json`, `event`).

### The job

The program runs in the action's folder and gets the job as one line of
JSON on stdin:

```json
{"args": {}, "input": {}, "key": "...", "memory": {}, "prev": {}, "run": {}, "with": {}}
```

`with` holds the arguments the workflow gave, templates filled in; `key` is
the job's idempotency key. What it prints is its output (see
[Output of a program](#output-of-a-program)); printing JSON makes it
`output.json`. A non-zero exit fails the action.

### Types

| `type` | A target is | Needs |
|---|---|---|
| `executable` | `executable` and `arguments`; a path with `/` is in the action's folder, a bare name is looked up on `PATH` | nothing |
| `python@3.12` | `module` and `callable`, called with the job (awaited when async) | [uv](https://docs.astral.sh/uv/) |
| `bun@1` | `file` and `export`, called with the job (awaited) | [Bun](https://bun.sh) |
| `container` | `executable` and `arguments`, run inside `image` | [Docker](https://docs.docker.com/get-docker/) |

A Python or Bun function returns its result, and that is `output.json`. A
Python action gets its own virtual environment, made with `uv` the first
time in `home` (`.venv` unless given), with `requirements` installed again
whenever that file changes. A Bun action gets `bun install` whenever
`package.json` or `bun.lock` change.

### In a container

A `container` action runs in a Docker image, so the machine needs Docker
and nothing the program itself needs:

```json title="actions/imap/action.json"
{
  "name": "imap",
  "version": 1,
  "type": "container",
  "image": "python:3.13-alpine",
  "limits": { "timeout": "120s" },
  "targets": { "check": { "executable": "python3", "arguments": ["check.py"] } }
}
```

- The job comes on stdin and the output is what it prints, as for any
  action (`docker run --rm -i`).
- The action's folder is mounted read-only at `/action`, where the program
  runs.
- Its `environment` is passed to the container by name only, so no value,
  and no secret, stands on a command line.
- `host.docker.internal` is this machine, on Linux too.
- The container is named after the job's idempotency key: a job run again
  removes one left by an earlier run, and a job stopped at its time limit
  has its container removed.
- An image is pulled the first time it is used; give `limits.timeout` room
  for that, or pull it beforehand.
- `limits.heartbeat` does not reach a container yet; use `timeout`.

### Limits

`limits.timeout` is the most a job may take (60 seconds unless given).
`limits.heartbeat` makes it a lease: a long job renews it by touching the
file `$ITERO_HEARTBEAT`, or is killed and run again.

All keys: [Action files](/reference/action-file/).
