# Plan 8: workflow settings

Board ticket #10. A workflow shared from fons (or anywhere) is used as it
is, with nothing in it edited, so `git pull` stays clean. What changes
from one user to another goes in the workspace: the workflow's settings.

## Spec

- **Where:** `config/<workflow>.json` in the workspace, one JSON object per
  workflow, as `args/<action>.json` is for actions. Args are an action's
  settings, config is a workflow's.
- **What it holds:**
  - the user's values: `{"sites": ["https://example.com"], "min_free": 10}`;
  - the switches `off`, `from` and `until`, with the meanings they have in
    a workflow's `meta`. They win over the workflow's own, so a workflow
    from someone else is turned off or bounded in time without editing it.
    They are not values: they never reach the state.
- **Used as state:** a run starts with its workflow's settings in its
  state, then the message's data on top, so `itero emit` can override a
  setting for one run. A step names a setting as it names any state value,
  `"$sites"`, and an action gets it as `$sites` in its environment. A
  workflow's `on` conditions are checked over the same state.
- **Only the run's own workflow:** a workflow used as a step of another
  runs with the run's state; its own settings are not added. (Later, if
  needed.)
- **Read when a run starts:** editing `config/` changes the next runs, never
  one under way: the settings are in its state from its start.
- **`itero config <workflow> ['<json>' | --file f]`** writes
  `config/<workflow>.json`; without data it prints it. The JSON must be an
  object; `off` must be true or false, `from` and `until` a date or a time,
  each refused with the file and the key named.
- **`itero check`** reads every file in `config/` and refuses a bad one;
  a file for no workflow is a note, not an error.
- **Shown:** `itero runs <run>` shows the first state, settings included,
  as today.

Not in this plan: settings for actions beyond args, settings of a
workflow used as a step, secrets in settings (`$$NAME` stays in steps).

## Files

- `src/flow.rs`: `Flow.settings`; `configure(&mut Flow, Map)`, which takes
  the switches out of a config into `meta` and keeps the rest as settings
  (reusing `moment` and the flag check of `meta`).
- `src/workspace.rs`: `config(workflow)`, read like `args`; `flows()` calls
  `configure` for each workflow with a config; `configs()` lists the files
  for `check`.
- `src/engine/daemon.rs`: `route` and `fire_schedules` start a run with
  `settings` + data, and check `on` over it.
- `src/cli.rs`: `Cmd::Config`; `check` reads `config/`.
- `examples/beacon`: sites from `config/sites.json` (`$sites`), the disk's
  limit from `config/disk.json`; `sites.txt` goes.
- Docs: a "Settings" part in the workflows guide, `commands/config.md`, the
  workspace tree (README, concepts), the glossary, `design-simple.md`.

## Tasks

Each task: the failing test first, then the code, then `cargo test`.

1. **Config and switches** (`tests/workspace.rs`). A workflow with
   `config/<name>.json` has its `settings` and its switches applied; a
   config's `off` turns off a workflow whose `meta` says nothing, and
   `"off": false` turns one on. Mistakes named: not an object, `off` not a
   boolean, a bad `until`. Fixtures in Turkish and with `ü`, `ş`.
2. **A run's first state** (`tests/daemon.rs`). A message starts a run
   whose state has the settings and the message's data, the data winning
   on a shared name; a step's `"$sites"` and an action's `$sites` get the
   setting; a schedule's run gets the settings; a workflow off in its
   config starts nothing; `on` with `{"$region": "eu"}` holds over a
   setting.
3. **`itero config`** (`tests/cli.rs`). Writes and prints the file; a bad
   name, a list or a bad switch is refused; `itero check` refuses a bad
   config and notes one for no workflow.
4. **The beacon** (`tests/examples.rs`). `#check-sites` reads `$sites` (a
   JSON list); the example ships `config/sites.json` and `config/disk.json`;
   the test gives `sites` in the environment, as a run does.
5. **Docs**, then `make verify`, one PR.
