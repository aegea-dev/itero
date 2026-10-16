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
triggers = "triggers"
sources = ["sources"]
```

| Key | Is | Default |
|---|---|---|
| `port` | the HTTP API's port, on 127.0.0.1 | `8787` |
| `workflows` | the folders with workflows, any number | `["workflows"]` |
| `actions` | the folders with actions, any number | `["actions"]` |
| `triggers` | the folder with the workspace's own triggers | `"triggers"` |
| `sources` | the folders with [sources](/guides/sources/), any number | `["sources"]` |

A folder is relative to the workspace; `~/` is your home folder. A folder
that is not there holds nothing, and `itero check` notes it.

- Two workflows with one `name`, in any of the folders, is an error naming
  both files.
- An action whose name is in two folders is the first folder's.

Two workspaces on one machine need two ports.
