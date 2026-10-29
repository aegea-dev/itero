---
title: Commands
description: Every itero command, at a glance.
---

Every command works in the workspace of the folder it runs in: the nearest
folder upwards with an `itero.config.json`. `itero help <command>` shows a
command's options, and every command that prints something takes `--json`.

| Command | Does |
|---|---|
| [`daemon`](/commands/daemon/) | runs your workflows; leave it running |
| [`workflow`](/commands/workflow/) <span class="wf">●</span> (`wf`) | your workflows, their runs (and why one broke), their settings |
| [`action`](/commands/action/) <span class="act">■</span> | your actions, which ones break and why, their args |
| [`connector`](/commands/connector/) <span class="con">▲</span> | your connectors, their args |
| [`emit`](/commands/emit/) | sends an event, and says what it starts; `--wait` for the answer |
| [`check`](/commands/check/) | reads your files before anything runs, and says what is wrong |
| [`which`](/commands/which/) | where a workflow, action or connector of a name is |
| [`info`](/commands/info/) | the workspace and its daemon: IDs, names, limits |
| [`secret`](/commands/secret/) | keeps a secret in the OS secret store |
| [`workspace`](/commands/workspace/) (`ws`) | the machine's workspaces, which `itero daemon --all` runs |
| [`service`](/commands/service/) | runs the daemon from login on |
| [`watch`](/commands/watch/) | for connectors: a line per change to a file or folder |
| [`completion`](/commands/completion/) | Tab completion for your shell |
| [`help`](/commands/help/) | the commands, or one command's options |

A day with itero, in four of them:

```sh
itero daemon                                   # in one terminal, left running
itero emit order.placed '{"order": 43}'        # from an app, a script, you
itero wf list                                  # what ran, and what waits
itero wf runs misty-lake                       # one run, step by step
```
