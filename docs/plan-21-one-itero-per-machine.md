# Plan 21: one itero per machine

Today each workspace runs a daemon of its own: its own port, its own
login service (`itero service install` in each folder), its own
watchdog and its own itero-ai, which loads its own copy of the model
(three workspaces with Qwen3.5-4B hold three copies, about 5.5 GB
each). Installing itero once should serve every workspace.

## Goals

- **One service:** installed once; a new workspace needs no service of
  its own.
- **One API:** one port for the machine, the workspace in the path.
- **One itero-ai:** one model in memory for the machine, and one scan
  for models.
- **See everything:** one list of the machine's workspaces and how each
  is doing.

## Decided (2026-10-29)

- **One `itero daemon` runs every workspace on the machine's list**,
  each on threads of its own. Not chosen: a machine daemon starting a
  daemon process per workspace (stronger isolation, but a forwarding
  layer, a socket per workspace and supervision of children); keeping a
  daemon per workspace with only the service shared (no single API).
  Steps, managers, workers and connectors already run as processes of
  their own, so a workspace's code cannot crash the daemon; what all
  workspaces share is itero's own code.
- **A workspace is named by its journal's two random words**
  (`quiet-river`, made with the journal, as runs are named), unique on
  the machine.
- **A folder runs only once someone adds it**: nothing is found by
  scanning, and no command adds a folder by being run in it.
- The names below (`itero workspace add|remove|list`,
  `itero daemon --all`, `/w/<name>/`, `--port`, `~/.local/state/itero`)
  were proposed and accepted in the brainstorm.

## The machine's list

- A JSON file, `~/.local/state/itero/workspaces.json`
  (`$XDG_STATE_HOME/itero/` when that is set), on every OS: for each
  workspace its journal id, name and folder. State the commands write,
  not configuration.
- `itero workspace add` (in the workspace's folder) adds it. Its journal
  is made if there is none. A name another listed workspace has is
  replaced in this workspace's journal by a new random name, and `add`
  says so. A folder whose journal id is listed already (the folder
  moved) updates that entry's folder.
- `itero workspace remove` (in the folder, or by name) takes it off the
  list; the folder and its journal stay.
- `itero workspace list [--json]`: each workspace's name, folder, state
  (`running`, `stuck`, `failed`, `stopped`, `missing`) and its last run.
- A listed folder that is gone, or no longer a workspace, is `missing`:
  not started, and shown so; it is never taken off the list by itself.

## The daemon

- **`itero daemon --all`** runs every workspace on the list. The
  service runs it, through the watchdog, as today.
- **`itero daemon`** in a folder runs only that workspace, in the
  foreground, as today (development, tests, the examples).
- **One daemon per journal.** Whichever daemon runs a workspace holds an
  exclusive lock (`flock`) on its `.itero/daemon.lock` for as long as it
  does. A second daemon on the same workspace refuses at once, naming
  the daemon that holds it (its name and pid, written in the lock file).
  `--all` skips a workspace locked by a foreground daemon and shows it
  as `stopped`, with why.
- **Each workspace has its own:** pass loop, alarm, path watcher
  (including its `.itero/wake`, so `itero emit` works unchanged),
  connectors, health and log. Its journal, `.itero/` and limits stay its
  own.
- **The list file is watched** (the existing path watching, no
  polling): a workspace added is started, one removed is stopped (its
  connectors ended, its threads joined, its lock freed).
- **A workspace in trouble does not stop the others:**
  - its pass running past the stale limit makes it `stuck`: told as the
    event `itero.workspace.stuck` (`workspace`, `since`) to every running
    workspace, and in `/health`; the daemon is not restarted for it;
  - its loop thread panicking is started again after a pause that
    doubles (`connectors::supervise::Policy`: 1 s up to 60 s, calm after
    5 minutes), and given up on after 10 times in 10 minutes: `failed`,
    and the event `itero.workspace.failed` (`workspace`, `why`).
- **The watchdog** restarts the whole daemon only when it exits or its
  `/health` stops answering.
- The daemon's own life (`Daemon::current()`, its name and id) is the
  machine's; each workspace's `daemon.started` log line names it.

## The API

- One port for the machine: `itero service install --port <n>`, else
  one the OS gives, kept in `~/.local/state/itero/port`.
- Routes:
  - `GET /health`: the daemon, each workspace's health by name, and
    itero-ai's;
  - `GET /workspaces`: the list, with each one's state;
  - `/w/<name>/...`: the routes a workspace's daemon serves today
    (`/health`, `/events`, `/runs`, `/runs/{id}`, `/workflows`),
    unchanged.
- The `Host` check stays: only `127.0.0.1:<port>` or `localhost:<port>`.
- A foreground `itero daemon` serves today's routes on the workspace's
  own port (`port` in `itero.config.json`, else `.itero/port`), as now.

## itero-ai and models

- **One itero-ai for the daemon**, on one socket
  (`/tmp/itero-ai-<uid>/itero.sock`, or `$XDG_RUNTIME_DIR/itero-ai/` on
  Linux). The keeper is already process-wide; it stops being keyed by
  the workspace.
- `itero.ai.started`, `.stopped`, `.failed`, `.model.loaded` go to every
  running workspace; `.model.missing` to the workspace whose step asked.
- **One scan for models.** Under `--all`, the built-in `models` workflow
  does not run in each workspace: the daemon scans at its start, after
  any `itero.ai.model.missing`, and every hour on its own alarm. A
  foreground daemon keeps the `models` workflow as today.

## The commands

- A command run in a folder finds the daemon running it: the machine's
  (through `/w/<name>/`) when the workspace is listed and the daemon
  answers, else the workspace's own port. `itero info` says which, with
  the API's address.
- `itero service install [--port <n>]`, `uninstall`, `status`, and
  `start`, `stop`, `restart`: one service for the machine, run from any
  folder. After `brew upgrade`, `itero service restart` moves every
  workspace to the new version.
- **The old services:** `install` finds the per-workspace services
  (`dev.aegea.itero.daemon-*`), lists them, asks, then uninstalls them
  and adds their workspaces to the list. `--yes` skips asking.
- `itero workspace add` with no daemon answering adds the workspace and
  says `itero service install` starts it.

## Docs

- A guide page on one itero per machine (what runs, the list, the
  states, the events); `itero workspace` and `itero service` command
  pages; the HTTP API reference with `/workspaces` and `/w/<name>/`;
  the itero-ai pages (one per machine); the glossary.
- The two new events in "The daemon's own workflows".

## Testing

- Integration tests run `itero daemon --all` with `XDG_STATE_HOME` and
  `HOME` in a temporary folder and two or three temporary workspaces:
  each runs its events; the API answers under `/w/<name>/`; adding and
  removing a workspace takes effect while it runs; a foreground daemon
  on a listed workspace is refused; a missing folder is `missing`;
  a name collision is renamed.
- A workspace that gets stuck (a test-only pass hook or a step that
  never ends inside the daemon) is reported while another keeps running.
- `itero service` against the OS is not run in CI: the migration is a
  pure decision (old labels found, their folders) tested on its own,
  and checked by hand on macOS; Linux (systemd user units) is said to
  be unchecked unless run in Docker.
- `make verify`; `make test-linux` (shared code and Linux paths change).

## Pull requests

1. The list, `itero workspace`, `itero daemon --all`, the lock, the
   API under `/w/<name>/`, the two events; their docs.
2. One itero-ai and one model scan for `--all`; the itero-ai docs.
3. The machine-wide `itero service` and moving the old services over;
   the service docs.

Each is released as it merges into a minor version (the service
changes break `itero service install`'s meaning).

## To confirm

Names that came with writing this down, not in the brainstorm:

- the states `running`, `stuck`, `failed`, `stopped`, `missing`;
- the events `itero.workspace.stuck` and `itero.workspace.failed`;
- `itero workspace list --json`, `itero service install --yes`;
- the lock file `.itero/daemon.lock`.

## Not in this plan

- Moving one workspace into a process of its own if it needs isolating
  (possible later; nothing here prevents it).
- Several users on one machine, or itero as a system service.
- Showing the machine in an app (the list and `/workspaces` feed it).
