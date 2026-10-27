---
title: itero.config.json
description: A workspace's manifest, everything about it in one file.
---

A folder with an `itero.config.json` is a workspace: itero uses the
nearest one upwards, as npm does with `package.json`. Every key has a
default, so `{}` is a whole workspace. The folders keep only the things
themselves: workflow files, actions, connectors.

```json
{
  "name": "home",
  "version": "0.1.0",
  "description": "Mail, orders and a beacon on this machine",
  "port": 8787,
  "folders": { "workflows": ["workflows"], "actions": ["actions"], "connectors": ["connectors"] },
  "limits": { "runs": 1000, "quiet": "10m" },
  "workflow": {
    "sites": { "description": "Each site, every minute", "settings": { "sites": ["https://example.com"] } },
    "keep": { "off": true }
  },
  "action": {
    "imap-check": { "description": "New mail on the server", "args": { "host": "imap.example.com" } }
  },
  "connector": {
    "mail.received": { "description": "New mail in the local mailbox" }
  }
}
```

## What it is

`name`, `version`, `description`, `tags`, and any keys of your own: kept,
and shown by `itero info` and the log's `daemon.started` line. `id`,
`created` and `path` are given by itero; setting one is an error.

## How it runs

| Key | Is | Default |
|---|---|---|
| `port` | the HTTP API's port, on 127.0.0.1: set it when an app needs a fixed address | one the OS gives (below) |
| `folders.workflows` | the folders with workflows, any number | `["workflows"]` |
| `folders.actions` | the folders with actions, any number | `["actions"]` |
| `folders.connectors` | the folders with [connectors](/guides/connectors/), any number | `["connectors"]` |

A folder is relative to the workspace; `~/` is your home folder. A folder
that is not there holds nothing, and `itero check` notes it. Each folder's
subfolders count too (hidden ones and `node_modules` apart). Two workflows,
two actions or two connectors of one name, in any of the folders, is an error
naming both files. The journal, the log and each workflow's memory live in
`.itero/`.

Without a `port`, the daemon asks the OS for a free one when it starts
and keeps it in `.itero/port`; its next start asks for the same one, and
only when that is taken gets another (the log says so). `itero info` shows
it. A daemon that cannot listen on its port stops before it starts
anything.

### `limits`

| Key | Is | Default |
|---|---|---|
| `workflows` | workflows, the internal ones apart; more is an error | `100` |
| `runs` | runs under way at once, the internal ones apart; past it, events wait their turn | `1000` |
| `steps` | steps running at once | `8` |
| `space` | what `.itero/` may take (`KB`, `MB`, `GB`) | `"1GB"` |
| `keep` | how long ended runs and their events are kept | `"1d"` |
| `log` | the log's size before it is rolled | `"10MB"` |
| `logs` | rolled logs kept | `3` |
| `quiet` | how long a step may print nothing; past it, it is stopped and runs again | `"10m"` |
| `most` | how long a step may run, printing or not | `"1d"` |

What happens at each: [The daemon's own workflows](/guides/maintenance/#limits).
An action's entry may set its own `quiet` and `most` (below).

## `workflow`, `action`, `connector`

An entry per workflow, action or connector, by its name. An entry describes;
it never creates: one that names nothing is an error from `itero check`.
An entry may not set `name`, `id`, `version`, `created` or `path`.

| In a `workflow` entry | Is |
|---|---|
| `description`, `tags`, your own keys | what it is |
| `off` | `true`: nothing starts it |
| `from`, `until` | a date (`2026-11-01`, a whole day) or a time: nothing starts it before or from then |
| `internal` | `true`: it looks after the workspace; its runs are left out of lists unless asked, and outside the limits |
| `settings` | values a run's state starts with: [Settings](/guides/workflows/#settings) |

| In an `action` entry | Is |
|---|---|
| `description`, `tags`, your own keys | what it is |
| `args` | data for the action, whatever workflow runs it: an object, or a list of objects (the step runs once per item); [Args and secrets](/guides/secrets/) |
| `limits` | `quiet` and `most` for this action's steps, like `{"quiet": "1h", "most": "2d"}`; unless given, the workspace's [`limits`](#limits) |

| In a `connector` entry | Is |
|---|---|
| `description`, `tags`, your own keys | what it is |
| `args` | data for the connector, as environment variables: an object; [A connector's args](/guides/secrets/#a-connectors-args) |

`itero action args set`, `itero connector args set` and `itero wf config set` write `args` and `settings` here, keeping
the rest of the file as it was. A workflow file has no `meta`: one is an
error saying where it goes.
