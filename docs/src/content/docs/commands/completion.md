---
title: completion
description: Tab completion for itero's commands and your workspace's names.
---

`itero completion` lets your shell finish itero commands when you press
Tab: the commands, their options, and the names of your workflows, actions,
connectors and runs.

```sh
itero completion <bash | zsh | fish>
```

Add one line to your shell's startup file:

| Shell | Line | File |
|---|---|---|
| zsh | `source <(itero completion zsh)` | `~/.zshrc` |
| bash | `source <(itero completion bash)` | `~/.bashrc` |
| fish | `itero completion fish \| source` | `~/.config/fish/config.fish` |

Then, in a workspace, `itero wf runs <Tab>` offers the latest runs' names,
and `itero action errors <Tab>` your actions. The names come from the
workspace you are in, as it is now.
