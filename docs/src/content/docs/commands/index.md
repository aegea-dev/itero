---
title: Commands
description: Every itero command, at a glance.
---

The commands at the top level are about the machine and its daemon, and
work from any folder. Everything about one workspace is under
[`itero ws`](/commands/workspace/): it acts on the workspace of the folder
it runs in (the nearest folder upwards with an `itero.config.json`), or the
one `--ws <name>` names. `itero help <command>` shows a command's options,
and every command that prints something takes `--json`.

## The machine

| Command | Does |
|---|---|
| [`daemon`](/commands/daemon/) | runs your workflows; leave it running |
| [`service`](/commands/service/) | runs the daemon from login on |
| [`info`](/commands/info/) | the machine's daemon: its version, port, workspaces |
| [`emit`](/commands/emit/) | sends an event to every workspace on the machine's list |
| [`secret`](/commands/secret/) | keeps a secret for every workspace, in the OS secret store |
| [`context`](/commands/context/) | the itero servers you work with: dev, staging, production |
| [`ai`](/commands/ai/) | the local models `#ai` steps use |
| [`watch`](/commands/watch/) | for connectors: a line per change to a file or folder |
| [`browser watch`](/commands/browser/) | for connectors: a line per change on a page in Chrome |
| [`webhook`](/commands/webhook/) | for connectors: takes webhooks from outside, each checked against a secret |
| [`completion`](/commands/completion/) | Tab completion for your shell |
| [`help`](/commands/help/) | the commands, or one command's options |

## One workspace

| Command | Does |
|---|---|
| [`ws add`, `list`, `remove`](/commands/workspace/) | the machine's list of workspaces, which `itero daemon --all` runs |
| [`ws info`](/commands/workspace/#info) | the workspace and its daemon: IDs, names, limits |
| [`ws check`](/commands/check/) | reads your files before anything runs, and says what is wrong |
| [`ws which`](/commands/which/) | where a workflow, action or connector of a name is |
| [`ws emit`](/commands/emit/) | sends an event to this workspace, and says what it starts; `--wait` for the answer |
| [`ws secret`](/commands/secret/#one-workspaces-secret) | keeps a secret for this workspace alone |
| [`ws workflow`](/commands/workflow/) <span class="wf">●</span> (`wf`) | your workflows, their runs (and why one broke), stopping one |
| [`ws action`](/commands/action/) <span class="act">■</span> (`ac`) | your actions, which ones break and why |
| [`ws connector`](/commands/connector/) <span class="con">▲</span> (`cn`) | your connectors, each with the args the workflows give it |
| [`ws event`](/commands/event/) (`ev`) | the events no daemon has handled yet, and cancelling one |

A day with itero, in four of them:

```sh
itero daemon                                      # in one terminal, left running
itero ws emit order.placed '{"order": 43}'        # from an app, a script, you
itero ws wf list                                  # what ran, and what waits
itero ws wf runs misty-lake                       # one run, step by step
```

A command that moved under `itero ws` says its new form: `itero wf runs`
answers `moved: itero ws wf runs`.
