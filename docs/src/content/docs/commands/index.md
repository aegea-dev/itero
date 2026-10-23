---
title: Commands
description: Every itero command, at a glance.
---

Every command works in the workspace of the folder it runs in: the nearest
folder upwards with an `itero.toml`. `itero help <command>` shows a
command's options.

| Command | Does |
|---|---|
| [`itero daemon`](/commands/daemon/) | runs the workspace: messages, schedules, watched files, steps |
| [`itero emit`](/commands/emit/) | sends a message, and says what it starts; `--wait` for its answer |
| [`itero workflows`](/commands/workflows/) | each workflow, its runs and the last; whether the daemon runs |
| [`itero runs`](/commands/runs/) | the runs, or one run step by step |
| [`itero errors`](/commands/errors/) | the runs that ended in error, and why |
| [`itero check`](/commands/check/) | reads every file and says what is wrong |
| [`itero which`](/commands/which/) | where a workflow, action or source of a name is |
| [`itero info`](/commands/info/) | the workspace and its daemon: IDs, names, limits |
| [`itero args`](/commands/args/) | sets an action's data |
| [`itero config`](/commands/config/) | sets a workflow's settings, or shows them |
| [`itero secret`](/commands/secret/) | keeps a secret in the OS secret store |
| [`itero service`](/commands/service/) | runs the daemon from login on |
| [`itero watch`](/commands/watch/) | for sources: a line per change to a file or folder |
| [`itero help`](/commands/help/) | the commands, or one command's options |

A day with itero, in four of them:

```sh
itero daemon                                   # in one terminal, left running
itero emit order.placed '{"order": 43}'        # from an app, a script, you
itero workflows                                # what ran, and what waits
itero runs misty-lake                          # one run, step by step
```
