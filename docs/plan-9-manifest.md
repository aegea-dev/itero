# Plan 9: itero.config.json

Board ticket #11. One JSON file at the workspace's root, like
`package.json`, says what the workspace is, how it runs, and everything
about its workflows, actions and sources. It replaces `itero.toml`,
`args/`, `config/` and a workflow's `meta`. The folders keep only the
things themselves.

## Spec

```json
{
  "name": "beacon",
  "version": "0.1.0",
  "description": "Health checks on a schedule, told when something changes",
  "port": 8796,
  "folders": { "workflows": ["workflows"], "actions": ["actions"], "sources": ["sources"] },
  "limits": { "runs": 1000, "quiet": "10m" },
  "workflow": {
    "sites": { "description": "Each site, every minute", "settings": { "sites": ["https://example.com"] } },
    "keep": { "off": true }
  },
  "action": {
    "imap-check": { "description": "New mail on the server", "args": { "host": "imap.example.com" } }
  },
  "source": {
    "mail.received": { "description": "New mail in the local mailbox" }
  }
}
```

- **The workspace:** the nearest folder upwards with `itero.config.json`.
  `{}` is a whole workspace: every key has its default. A folder with an
  `itero.toml` and no `itero.config.json` is an error saying to move it.
- **About it:** `name`, `version`, `description`, `tags` and any keys of
  the person's own, kept and shown by `itero info` (today's `[meta]`). `id`,
  `created` and `path` are given by itero. `name` here is the person's; the
  journal's random name stays the workspace's name until one is given.
- **How it runs:** `port`, `folders` (`workflows`, `actions`, `sources`,
  each a list, today's defaults) and `limits` (today's keys and defaults).
- **`workflow.<name>`:** `description`, `tags`, `off`, `from`, `until`,
  `internal`, own keys (today's `meta`), and `settings` (plan 8's values: a
  run's state starts with them). A workflow file has no `meta` any more:
  one is an error saying where it goes. The built-in workflows carry their
  description and `internal` in code; `workflow.keep.off` turns one off.
- **`action.<name>`:** `description`, `tags`, own keys, and `args` (today's
  `args/<name>.json`: an object, or a list of objects).
- **`source.<name>`:** `description`, `tags`, own keys.
- **Entries describe, never create:** `itero check` refuses an entry that
  names no workflow, action or source. An entry may not set `name`, `id`,
  `version` or `path`.
- **Commands:** `itero args <action> '<json>'` and `itero config <workflow>
  '<json>'` write `action.<name>.args` and `workflow.<name>.settings` in
  the file, keeping the rest of it as it was (key order kept:
  serde_json's `preserve_order`), written through a temporary file.
  `itero config <workflow>` with no data shows the settings.
- **Gone, no shim:** `itero.toml`, `args/`, `config/`. `itero check` names
  an `args/` or `config/` folder it finds, saying where it went.
- **Not in this plan:** `run` for folder actions and sources (ticket #5),
  tools inside a workspace (ticket #9). An entry's other keys are kept as
  the person's own.

## Files

- `Cargo.toml`: serde_json `preserve_order`; `toml` goes.
- `src/workspace.rs`: `Manifest` (read from JSON, unknown keys kept as the
  person's own at the top and in entries); `find` reads it; `args` and
  `config` read entries; `write_entry` writes one key of one entry back.
- `src/flow.rs`: a workflow file's `meta` refused; `entry(v)` reads a
  `workflow.<name>` entry into `Meta` and settings (today's `meta` reader
  and `configure`).
- `src/cli.rs`: `args`, `config`, `check` (entries naming nothing, old
  folders), `info`.
- `src/builtin/*.json`: no `meta`; their meta in `workspace.rs`.
- `src/engine/http.rs`, `service.rs`, `install.sh`: the wording.
- Tests: every `itero.toml` written by a test becomes `itero.config.json`;
  new tests per task.
- Examples: `itero.config.json` each, their `itero.toml` comments into a
  `README.md`, beacon's `config/` into its manifest.
- Docs: `reference/itero-toml.md` becomes `reference/itero-config.md`; the
  workflows guide's meta part, args, settings, the tree, every
  `itero.toml` mention.

## Tasks

1. **The manifest replaces itero.toml** (`tests/workspace.rs`): `{}` gives
   the defaults; port, folders, limits and the about keys read; an
   `itero.toml` alone is an error naming the new file; a bad key named.
   Then every test and example moves to `itero.config.json`.
2. **Workflow entries** (`tests/workspace.rs`, `tests/flow.rs`): meta and
   settings from `workflow.<name>`; a file's `meta` refused; built-ins
   internal and turned off by an entry.
3. **Action args** (`tests/daemon.rs`, `tests/cli.rs`): `action.<name>.args`
   as today's args file, lists splitting the run; `itero args` and
   `itero config` write the file and keep its other keys and order.
4. **Check** (`tests/cli.rs`): an entry naming nothing, an old `args/` or
   `config/` folder.
5. **Examples, docs**, then `make verify`, one PR.
