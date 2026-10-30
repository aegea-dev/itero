---
title: Quickstart
description: A workspace with one workflow and one action, an event, and a run.
---

A workspace is a folder with an `itero.config.json`; `{}` will do.

```sh
mkdir hello && cd hello
echo '{}' > itero.config.json
mkdir workflows actions
```

A workflow, `workflows/greet.json`: on a `greet.someone` event, run the
action `#hello`, then show what it said.

```json
{ "on": [{ "event": "greet.someone" }],
  "steps": [{ "#hello": { "args": { "name": "$event.name" } } },
            { "#notify": { "args": { "text": "$output" } } }] }
```

An action, `actions/hello`: any executable file. It gets its args as
environment variables (`name`, the event's `name`: `$event.name`), and
what it prints is its result, which `#notify` is given as its text:
`$output`.

```sh
#!/bin/sh
echo "Merhaba, $name!"
```

```sh
chmod +x actions/hello
itero check
```

`itero check` reads every file and says what is wrong. Start the daemon
and leave it running; it takes each event as it comes:

```sh
itero daemon
```

In another terminal, in the same folder, send an event (a message):

```sh
itero emit greet.someone '{"name": "Ayşe"}'
```

Within a second a notification says "Merhaba, Ayşe!". Then:

```sh
itero wf list          # each workflow, its runs, and whether the daemon runs
itero wf runs          # the run, done, with its name
itero wf runs <name>   # the run step by step
```

`.itero/itero.log` has a line for each step. Ctrl-C stops the daemon. To
run it from login on: `itero workspace add`, then `itero service install`
([Start at login](/guides/service/)).

## Next

- [Writing your first workflow](/guides/first-workflow/): a website
  checked every five minutes, one step at a time.
- [Workflows](/guides/workflows/): side by side, conditions,
  workflows as steps, the end lists.
- [Actions](/guides/actions/): what an action gets and how it answers.
- [Connectors](/guides/connectors/): where events come from.
- [The log, IDs and names](/guides/log/): what happened, and how to find
  it.
- The examples in the repository: `mail-alert`, `mail-servers`, `gmail`
  (new mail pushed as it arrives), `notify-api` (an app's notification,
  its answer posted back), `order`,
  `page-digest`, `page-summary` (a local model), `photos`, `languages`
  (actions in Python, Node, Bun and Docker) and `terraform` (a k3d cluster
  with a web app, rolled back when a step fails).
