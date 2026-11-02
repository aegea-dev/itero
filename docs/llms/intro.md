# itero

> itero runs workflows again and again: on a schedule, when an app sends an event, or when a file changes, with the command line tools already on the machine. A workspace is a folder, kept in git: its workflows are JSON files and its actions are executable scripts.

This file is for an AI assistant helping someone with itero. People write the workflows; you mostly write the actions a workflow names. Write each action from the workflow you are given.

## The parts

- **Workspace**: a folder with `itero.config.json`, its manifest; `workflows/`, `actions/` and `connectors/` are inside it.
- **Workflow**: `workflows/<name>.json`. `on` says what starts a run (an event, a schedule, a changed path); `steps` run in order.
- **Action**: an executable file in `actions/`, named by its file name; a step names it `#<name>`. Some are built in, such as `#notify` and `#ai`.
- **Connector**: an executable file in `connectors/` that brings events in, one JSON line each.

## Writing an action

- Write POSIX sh unless the person asks for another language. The first line names it (`#!/bin/sh`); the file has no extension and is executable (`chmod +x`).
- Do one piece of work, in a few lines.
- Its values are environment variables named after its step's args: the step `{ "#ship": { "args": { "order": "$event.order" } } }` gives `actions/ship` the variable `$order`. The same args come as JSON on stdin, `{"args": {...}}`, with their types. It gets nothing else: not the event and not another step's result, unless its step gives them as args.
- What it prints is its result, the next step's `$output`: read as JSON when it is JSON (an object carries several values), else as text, trimmed.
- Exit 0 when it is done; any other exit code is an error, and the end of its stderr says why. Never hide a failure: no `|| true`, no `|| exit 0`, and no pipe that ends in a command that succeeds when the one before it failed.
- A secret comes as an arg whose value is `"$$NAME"` in the workflow. The action reads it like any other arg and never prints it.

## Reading the workflow

Before writing an action, find its step in the workflow. The step's `args` are the action's variables, by those names. In them, `$event.<field>` is a field of the event that started the run and `$output` is what the step before printed. Use the names and the shape of data the workflow gives; when the shape is not shown, ask rather than guess.

## An example

A person wrote this workflow: when a report is saved, tell me how many lines it has.

```json workflows/report.json
{ "on": [{ "event": "report.saved" }],
  "steps": [
    { "#count-lines": { "args": { "file": "$event.path" } } },
    { "#notify": { "args": { "title": "Report saved", "text": "$output lines" } } }
  ] }
```

The action it names gets `$file`, the event's `path`, and prints a number:

```sh actions/count-lines
#!/bin/sh
# prints how many lines $file (its arg) has; fails when it cannot read it
wc -l < "$file"
```

When the file cannot be read, `wc` fails, so the action fails with it and the run says why.

## Checking the work

- `itero ws check`, in the workspace, reads every workflow, action and connector and says what is wrong; it exits 1 on an error.
- Run an action by hand, with its args as environment variables: `file=report.txt ./actions/count-lines`.
- `itero ws emit --wait report.saved '{"path": "report.txt"}'` sends one event and waits for its run to end, when a daemon runs the workspace.
