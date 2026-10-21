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
{ "on": ["greet.someone"], "steps": ["#hello", "#notify"] }
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

`itero check` reads every file and says what is wrong. Now send a message
and run one pass of the daemon:

```sh
itero emit greet.someone '{"name": "Ayşe"}'
itero daemon --once
itero runs
```

A notification says "Merhaba, Ayşe!", and `itero runs` shows the run as
`done`. Without `--once`, `itero daemon` keeps running and takes each
message as it comes; `itero service install` starts it at login.

## Next

- [Writing a workflow](/guides/workflows/): side by side, conditions,
  workflows as steps, the end lists.
- [Actions](/guides/actions/): what an action gets and how it answers.
- [Messages and sources](/guides/messages/): where messages come from.
- The examples in the repository: `mail-alert`, `mail-servers`, `order`
  and `page-digest`.
