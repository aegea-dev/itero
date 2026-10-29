---
title: workspace
description: The machine's list of workspaces, which one daemon runs.
---

`itero workspace` keeps the machine's list of workspaces, which one
[`itero daemon --all`](/commands/daemon/) runs. See
[One itero per machine](/guides/machine/).

```sh
itero workspace add
itero workspace remove [<name>]
itero workspace list [--json]
```

## `add`

Puts the workspace of this folder on the list, under its journal's name
(two words, as `itero info` shows). A name another listed workspace has
is replaced in this workspace's journal by a new one, and `add` says so.
A workspace already on the list (its folder moved) has its folder
updated. A daemon running `--all` starts it at once.

```
$ itero workspace add
added quiet-river (/Users/me/projects/home)
the machine's daemon starts it now
```

## `remove`

Takes a workspace off the list: this folder's, or one by its name, from
anywhere. A daemon running `--all` stops it at once; its folder and its
journal stay.

## `list`

Each listed workspace: its name, folder, state and last run. The state
is the running daemon's when one answers (`running`, `stuck`, `failed`,
`stopped` with why), else `stopped`, or `missing` when its folder is gone
or holds another workspace.

```
$ itero workspace list
NAME          FOLDER                    STATE     LAST RUN
quiet-river   /Users/me/projects/home   running   2m ago
misty-lake    /Users/me/projects/mail   running   1h ago
```
