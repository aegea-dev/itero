---
title: Quickstart
description: A workspace with one workflow and one action, a message, and a run.
---

A workspace is a folder with an `itero.toml`; an empty one will do.

```sh
mkdir hello && cd hello
touch itero.toml
mkdir workflows actions
```

A workflow, `workflows/greet.json`: on a `greet.someone` message, run the
action `#hello`, then show what it said.

```json
{ "on": [{ "event": "greet.someone" }], "steps": ["#hello", "#notify"] }
```

An action, `actions/hello`: any executable file. It gets the message's
fields as environment variables, and what it prints joins the run's
state, so `#notify` finds `$text` there.

```sh
#!/bin/sh
echo "{\"text\": \"Merhaba, $name!\"}"
```

```sh
chmod +x actions/hello
itero check
```

`itero check` reads every file and says what is wrong. Start the daemon
and leave it running; it takes each message as it comes:

```sh
itero daemon
```

In another terminal, in the same folder, send a message:

```sh
itero emit greet.someone '{"name": "Ayşe"}'
```

Within a second a notification says "Merhaba, Ayşe!". Then:

```sh
itero workflows        # each workflow, its runs, and whether the daemon runs
itero runs             # the run, done, with its name
itero runs <name>      # the run step by step
```

`.itero/itero.log` has a line for each step. Ctrl-C stops the daemon;
`itero service install` runs it from login on.

## Next

- [Writing a workflow](/guides/workflows/): side by side, conditions,
  workflows as steps, the end lists.
- [Actions](/guides/actions/): what an action gets and how it answers.
- [Messages and sources](/guides/messages/): where messages come from.
- [The log, IDs and names](/guides/log/): what happened, and how to find
  it.
- The examples in the repository: `mail-alert`, `mail-servers`, `order`,
  `page-digest`, `page-summary` (a local model), `photos` and `slug` (a
  Python action that brings its own dependency).
