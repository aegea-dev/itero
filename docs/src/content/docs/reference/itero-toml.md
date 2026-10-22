---
title: itero.toml
description: A workspace's settings.
---

A folder with an `itero.toml` is a workspace. Every key has a default, so
an empty file is a workspace too; an unknown key is an error.

```toml
port = 8787
workflows = ["workflows", "~/projects/photos/workflows"]
actions = ["actions"]
sources = ["sources"]
```

| Key | Is | Default |
|---|---|---|
| `port` | the HTTP API's port, on 127.0.0.1 | `8787` |
| `workflows` | the folders with workflows, any number | `["workflows"]` |
| `actions` | the folders with actions, any number | `["actions"]` |
| `sources` | the folders with [sources](/guides/messages/#sources), any number | `["sources"]` |

A folder is relative to the workspace; `~/` is your home folder. A folder
that is not there holds nothing, and `itero check` notes it. Each folder's
subfolders count too (hidden ones and `node_modules` apart). Two workflows,
two actions or two sources of one name, in any of the folders, is an error
naming both files.

Args live in `args/`; the journal, the log and each workflow's memory in
`.itero/`. Two workspaces on one machine need two ports.

## `[meta]`

What you say about the workspace, any keys: `itero info` and the log's
`daemon.started` line show them.

```toml
[meta]
description = "Home: mail and orders"
owner = "ayşe"
```

`id`, `name`, `created`, `path` and `version` are given by itero; setting
one is an error.

## `[limits]`

```toml
[limits]
workflows = 100
runs = 1000
steps = 8
space = "1GB"
keep = "1d"
log = "10MB"
logs = 3
quiet = "10m"
most = "1d"
```

| Key | Is | Default |
|---|---|---|
| `workflows` | workflows, the internal ones apart; more is an error | `100` |
| `runs` | runs under way at once, the internal ones apart; past it, messages wait their turn | `1000` |
| `steps` | steps running at once | `8` |
| `space` | what `.itero/` may take (`KB`, `MB`, `GB`) | `"1GB"` |
| `keep` | how long ended runs and their messages are kept | `"1d"` |
| `log` | the log's size before it is rolled | `"10MB"` |
| `logs` | rolled logs kept | `3` |
| `quiet` | how long a step may print nothing; past it, it is stopped and runs again | `"10m"` |
| `most` | how long a step may run, printing or not | `"1d"` |

What happens at each: [The daemon's own workflows](/guides/maintenance/#limits).
