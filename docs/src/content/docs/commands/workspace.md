---
title: workspace
description: The machine's list of workspaces, which one daemon runs.
---

`itero workspace` (`itero ws` for short) is everything about one
workspace, and the machine's list of workspaces, which one
[`itero daemon`](/commands/daemon/) runs; `itero ws daemon` runs one
alone. See
[One itero per machine](/guides/machine/).

```sh
itero ws add
itero ws remove [<name>]
itero ws list [--json]
itero ws info [--json]
itero ws check                # reads your files
itero ws which <name>         # where a name is
itero ws emit <type>          # sends an event to this workspace
itero ws secret <NAME>        # keeps a secret for this workspace
itero ws wf list | runs | stop  # the workflows, their runs, stopping one
itero ws ac list | errors
itero ws cn list
itero ws ev list | cancel     # events not yet handled
```

Each page: [`check`](/commands/check/), [`which`](/commands/which/),
[`emit`](/commands/emit/), [`secret`](/commands/secret/#one-workspaces-secret),
[`wf`](/commands/workflow/), [`ac`](/commands/action/),
[`cn`](/commands/connector/).

## Which workspace

A `ws` command acts on the workspace of the folder it runs in. Another is
named right after `ws`, from any folder: by its name from `itero ws list`,
or by its folder's path (a word with a `/` in it, or `.` or `..`), listed
or not:

```sh
itero ws quiet-river wf runs
itero ws quiet-river info
itero ws ~/projects/shop check
itero ws ./shop wf list
```

A name not on the list is an error; to mean a folder named like a word,
write `./shop`. Outside any workspace, with none named, a `ws` command
says the names on the list. `add` takes no workspace before it (it adds
the folder it runs in), and neither do `list` and `remove`, which are
about the list.

## `add`

Puts the workspace of this folder on the list, under its journal's name
(two words, as `itero ws info` shows). A name another listed workspace has
is replaced in this workspace's journal by a new one, and `add` says so.
A workspace already on the list (its folder moved) has its folder
updated. A daemon running `--all` starts it at once.

It also adds `.env` to the workspace's `.gitignore` (making the file when
there is none), unless a line there names it already: `.env` holds your
own values, secrets among them, and stays out of git.

```
$ itero ws add
added quiet-river (/Users/me/projects/home)
.env, where its secrets and values may be kept, is in .gitignore now
the machine's daemon runs it
```

With the machine's daemon running, it waits for it to take the workspace
up (a few seconds) and says so, or why not:
`the machine's daemon does not run it: <why>`, and, when that daemon runs
another itero, `itero service restart` puts this one in place.

## `remove`

Takes a workspace off the list: this folder's, or one by its name, from
anywhere. A daemon running `--all` stops it at once; its folder and its
journal stay.

## `list`

Each listed workspace: its name, folder, state and last run. The state
is the running daemon's when one answers (`running`, `stuck`, `failed`,
`stopped` with why, or `pending` until it reads a workspace just added),
else `stopped`, or `missing` when its folder is gone or holds another
workspace. `--json` gives the same JSON as the machine's
[`GET /workspaces`](/reference/http/#one-itero-per-machine); with
[`--context`](/commands/context/), that server's list.

```
$ itero ws list
NAME          FOLDER                    STATE     LAST RUN
quiet-river   /Users/me/projects/home   running   2m ago
misty-lake    /Users/me/projects/mail   running   1h ago
```

## `daemon`

`itero ws daemon` runs this workspace alone, in the foreground, as
[`itero daemon`](/commands/daemon/) runs every one on the list: for a
workspace you are making, before `itero ws add`. Ctrl-C stops it. A
workspace the machine's daemon runs already is refused, naming it.

```sh
itero ws daemon                 # this folder's
itero ws ./shop daemon          # a folder's, from elsewhere
```

## `apply`

`itero ws apply --context <name>` sends this folder's workspace to that
[context](/commands/context/)'s server: made there the first time (on
its list, its daemon runs it), made to match after. Files gone here are
removed there, so a workflow deleted here stops running there too.

```sh
itero ws apply --context prod             # the context's workspace
itero ws shop apply --context prod        # or one named
itero ws apply --dry-run --context prod   # what it would do
```

```
$ itero ws apply --context prod
~ workflows/order.json
+ actions/ship
- workflows/old.json
```

Every file in the folder goes, but this machine's own: `.itero/` (the
journal and log: the server has its own), `.env` (values differ from one
environment to another) and `.git/`. A symlink is refused, as it points
at this machine; so is more than 64 MiB in all. Secrets never travel:
set them on the server. `--json` gives what the server answered: see
[`PUT /workspaces/{name}`](/reference/http/#put-workspacesname).

## `info`

`itero ws info` tells you about the workspace and the daemon running it,
if one is. For the machine's daemon, see [`itero info`](/commands/info/).

```sh
itero ws info [--json]
```

The workspace: its id, name, when it was made, its folder, the itero that
made it, what its manifest says it is, and its `limits`. Then the daemon running it, when
one answers on the port: its id, name, start, pid, host, itero version and port.
Last, whether notifications can be shown: on macOS, macOS's setting for
Itero (`allowed`, `off`, `not asked`) and the copy of Itero.app that
notifies; on Linux, the notifier it uses. `--json` gives the same JSON as
the API's [`GET /info`](/reference/http/#get-info).

```
$ itero ws info
workspace
  id 01a16000-8c56-7202-87a3-b6b17d7a725d
  name warm-otter
  created 2026-10-21T19:46:51Z
  path /Users/me/home
  itero 0.15.0
  limits
    workflows 100
    runs 1000
    steps 8
    space 1073741824
    keep 1day
    log 10485760
    logs 3
daemon: not running (no answer on port 8787)
notifications: allowed, Itero.app 22 in /Users/me/Library/Application Support/itero/Itero.app
```

See [`itero.config.json`](/reference/itero-config/) and
[The log, IDs and names](/guides/log/).
