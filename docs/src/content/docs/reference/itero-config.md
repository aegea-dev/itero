---
title: itero.config.json
description: A workspace's manifest, everything about it in one file.
---

A folder with an `itero.config.json` is a workspace: itero uses the
nearest one upwards, as npm does with `package.json`. Every key has a
default, so `{}` is a whole workspace. The folders keep only the things
themselves: workflow files, actions, connectors. It says what the
workspace is and how it runs, never values to work on: those come from
an event, a step's args, or the workspace's `.env` ([Secrets and
environment variables](/guides/secrets/)). A workflow reads it, read-only, as `$config`.
What belongs to the machine running it, not the workspace, is in
[`daemon.json`](/reference/daemon-json/)
([Which file?](/reference/concepts/#which-file)).

Every key is itero's, and each is read through the config's
[JSON Schema](#the-schema): a key itero does not know, or a value of the
wrong form, is an error, from every command and from the daemon. Your
own keys go in [`metadata`](#metadata). How to write one, step by step:
[Writing itero.config.json](/guides/config/).

```json
{
  "$schema": "https://docs.itero.sh/schema/itero.config.json",
  "name": "home",
  "version": "0.1.0",
  "description": "Mail, orders and a beacon on this machine",
  "tags": ["home", "mail"],
  "port": 8787,
  "folders": { "workflows": ["workflows"], "actions": ["actions"], "connectors": ["connectors"] },
  "limits": { "runs": 1000, "quiet": "10m" },
  "workflow": {
    "sites": { "description": "Each site, every minute", "tags": ["beacon"] },
    "keep": false
  },
  "action": {
    "imap-check": { "description": "New mail on the server" }
  },
  "connector": {
    "mail.received": true
  },
  "metadata": { "owner": "ayşe", "region": "eu" }
}
```

## What it is

| Key | JSON type | Is | Default |
|---|---|---|---|
| `$schema` | string | the schema's URL, for your editor ([below](#the-schema)) | none |
| `name` | string | the workspace's name: lowercase letters, digits and hyphens, 1 to 63, a letter or digit at each end | the name itero gives it (two words) |
| `version` | string | its version, as you number it | none |
| `description` | string | what it does | none |
| `tags` | array of strings | words to find it by | none |
| `metadata` | object | your own keys, anything inside ([below](#metadata)) | none |

`itero ws info` shows each, and the log's `daemon.started` line has them.
The machine's list (`itero ws list`) and the API go by the name itero gave
the workspace, which `ws info` shows when `name` is not set. `id`, `created`
and `path` are given by itero; setting one is an error.

## How it runs

| Key | JSON type | Is | Default |
|---|---|---|---|
| `port` | integer, 1 to 65535 | the HTTP API's port, on 127.0.0.1: set it when an app needs a fixed address | one the OS gives (below) |
| `folders.workflows` | array of strings | the folders with workflows, any number | `["workflows"]` |
| `folders.actions` | array of strings | the folders with actions, any number | `["actions"]` |
| `folders.connectors` | array of strings | the folders with [connectors](/guides/connectors/), any number | `["connectors"]` |

A folder is relative to the workspace; `~/` is your home folder. A folder
that is not there holds nothing, and `itero ws check` notes it. Each folder's
subfolders count too (hidden ones and `node_modules` apart). Two workflows,
two actions or two connectors of one name, in any of the folders, is an error
naming both files. The journal, the log and each workflow's memory live in
`.itero/`.

Without a `port`, the daemon asks the OS for a free one when it starts
and keeps it in `.itero/port`; its next start asks for the same one, and
only when that is taken gets another (the log says so). `itero ws info` shows
it. A daemon that cannot listen on its port stops before it starts
anything.

### `limits`

| Key | JSON type | Is | Default |
|---|---|---|---|
| `workflows` | integer, at least 1 | workflows, the internal ones apart; more is an error | `100` |
| `runs` | integer, at least 1 | runs under way at once, the internal ones apart; past it, events wait their turn | `1000` |
| `steps` | integer, at least 1 | steps running at once, no more than the daemon's open files allow | `64` |
| `space` | string, a size | what `.itero/` may take | `"1GB"` |
| `keep` | string, a duration | how long ended runs and their events are kept | `"1d"` |
| `log` | string, a size | the log's size before it is rolled | `"10MB"` |
| `logs` | integer, at least 1 | rolled logs kept | `3` |
| `quiet` | string, a duration | how long a step may print nothing; past it, it is stopped and runs again | `"10m"` |
| `most` | string, a duration | how long a step may run, printing or not | `"1d"` |

A size is a whole number and `KB`, `MB` or `GB` (`"500MB"`, 1024 to a
step); a duration, a number and a unit (`s`, `m`, `h`, `d`, `w` among
them), or several (`"30m"`, `"1h 30m"`), at least a second.

What happens at each: [The daemon's own workflows](/guides/maintenance/#limits).
A step may set its own `quiet` and `most`, in its workflow ([a step's keys](/reference/workflow-file/)).

## `workflow`, `action`, `connector`

An entry per workflow, action or connector, by its name. An entry describes;
it never creates: one that names nothing is an error, from `itero ws check`
and from the daemon, which runs nothing in the workspace until it is put
right ([Problems in your files](/commands/daemon/#problems-in-your-files)).
The built-in workflows (`keep`, `logs`, `models`) always have one to name.

An entry is an object, or `true` or `false`: `true` is on, as `{}`;
`false` is off, as `{ "off": true }`.

| In a `workflow` entry | JSON type | Is |
|---|---|---|
| `description` | string | what it does |
| `tags` | array of strings | words to find it by |
| `off` | boolean | `true`: nothing starts it |
| `from`, `until` | string | a date (`2026-11-01`, a whole day) or a time (`2026-11-01T09:00:00Z`): nothing starts it before or from then |
| `internal` | boolean | `true`: it looks after the workspace; its runs are left out of lists unless asked, and outside the limits |
| `metadata` | object | your own keys about it |

| In an `action` or a `connector` entry | JSON type | Is |
|---|---|---|
| `description` | string | what it does |
| `tags` | array of strings | words to find it by |
| `metadata` | object | your own keys about it |

`itero ws check` shows each workflow's entry. A step's own `quiet` and
`most` are on the step, in its workflow
([a step's keys](/reference/workflow-file/)); an action's entry has none.

A workflow's `settings`, an action's `args` or `repeat`, and a
connector's `args` are errors saying where they go: a step's `args` and
`repeat` in its workflow, a connector's in each `on` entry that listens
([A connector's args](/guides/secrets/#a-connectors-args)). A workflow file has no `meta`: one is an
error saying where it goes.

## `metadata`

Anything of your own: who looks after it, a region, links. Itero keeps it,
shows it and never acts on it. A workflow reads the workspace's as
`$config.metadata`:

```json
{ "#report": { "args": { "region": "$config.metadata.region" } } }
```

`metadata` is the one place for them: a key of your own anywhere else is
an error, which says to put it in `metadata`.

## The schema

The config's JSON Schema is at
[`https://docs.itero.sh/schema/itero.config.json`](https://docs.itero.sh/schema/itero.config.json),
made from the same definitions itero reads the config with, so what your
editor underlines is what itero refuses. `"$schema"` with that URL, at the
top of the file, gives VS Code, JetBrains IDEs and other editors that read
JSON Schema its keys, their types and their descriptions as you type. The
schema is in [SchemaStore](https://www.schemastore.org)'s catalog under the
file's name, so JetBrains IDEs and other editors that use the catalog find
it without `$schema`.

Itero reads values the schema cannot check too: a size or a duration it
can read (`"0s"` is not one), and a date that is a real day. Every problem
is told in one run, by the file and the place in it:

```
$ itero ws check
itero.config.json
  error: limits.keep: a duration like 30m or 1d, not "1x"
  error: workflow.greet: "of" is not a key; did you mean "off"?
  error: "prot" is not a key; did you mean "port"?
  error: "owner" is not a key; your own keys go in "metadata"
4 errors, 0 warnings
```

Every other command says the same and does nothing else; the daemon runs
nothing in the workspace until it is put right.
