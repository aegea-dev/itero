# Plan 25: commands by scope

## Why

itero runs one daemon per machine, for every workspace on the machine's
list, yet most of its commands sit at the top level and act on the
workspace of the folder they run in. `itero info` outside a workspace
fails, though the question after an upgrade ("which itero does the
daemon run?") is about the machine. A reader cannot tell from a command
whether it is about the machine or about one workspace.

## The rule

- **Top level: the machine and its daemon.** A top-level command works
  from any folder.
- **`itero workspace` (`itero ws`): one workspace.** Every command about
  one workspace, and its workflows, actions and connectors, goes here.

## The commands

| Top level | Does |
|---|---|
| `itero daemon` | runs the daemon (`--all`: the machine's list), as today |
| `itero service ...` | the service that runs the machine's daemon, as today |
| `itero info` | the machine's daemon: its version, pid, port, the workspaces it runs, itero-ai, notifications |
| `itero emit` | sends an event to every workspace on the machine's list |
| `itero secret` | a secret for every workspace, as today |
| `itero ai ...` | the machine's models, as today |
| `itero watch` | prints a line each time a path changes, for connectors, as today |
| `itero completion`, `itero help` | as today |

| Under `itero ws` | Does |
|---|---|
| `add`, `list`, `remove` | the machine's list, as today |
| `info` | today's `itero info`: the workspace's identity, meta, limits, and the daemon running it |
| `check` | today's `itero check` |
| `which` | today's `itero which` |
| `emit` | sends an event to one workspace (today's `itero emit`) |
| `secret` | a secret of one workspace only |
| `wf list`, `wf runs` | today's `itero wf ...` |
| `ac list`, `ac errors`, `ac repeat get/set` | today's `itero action ...` |
| `cn list`, `cn args get/set` | today's `itero connector ...` |

Names and aliases: `workspace` (`ws`), `workflow` (`wf`), `action`
(`ac`), `connector` (`cn`). `connector` keeps its name: renaming it to
`event` would clash with the events it prints. Help shows each alias after its
command's name, and clap's own note after the text, as both:
`action (ac)  The actions, the ones that break, and their repeat
[aliases: ac]`.

## Which workspace

- A `ws` command acts on the workspace of the folder it runs in, as
  today.
- `--ws <name>`, a name from `itero ws list`, picks another, from any
  folder, anywhere after `ws`: `itero ws wf runs --ws pale-otter`. A
  name not on the list is an error naming `itero ws list`.
- `itero ws add` takes no `--ws`: it adds the folder it runs in.
  `itero ws remove [name]` and `itero ws list` are about the list, and
  take none either.

## `itero info`

- From any folder: this itero's version, and the machine's daemon from
  its `/health`: version, pid, port, started, each workspace (passes,
  last error), itero-ai's state, and whether Itero may notify.
- No daemon answering: says so, with the port it looked on and `itero
  service status` to see why.
- When the daemon's version is not this itero's, it says so, and that
  `itero service restart` runs this one.
- `--json` as everywhere.

## `itero emit` and `itero ws emit`

- `itero ws emit` is today's `itero emit`, on one workspace: written to
  its journal, so it waits for a daemon that is not running; `--wait`,
  `--timeout`, `--json` as today.
- `itero emit` writes the event into the journal of every workspace on
  the machine's list, the same way, one id per workspace. It prints each
  workspace's name and the event's id there (`--json`: a list). With
  `--wait`, it waits for the runs it started in all of them, and prints
  them by workspace; it exits 1 when one failed or broke, 2 when time ran
  out, as today.
- An empty list: an error saying `itero ws add` puts a workspace on it,
  and `itero ws emit` sends to this one.
- An action that runs `itero emit` now reaches every workspace; one that
  means its own workspace runs `itero ws emit` (it runs in its workspace,
  so the folder picks it). The docs say which to use.
- The machine's API gets `POST /events`, sending to every workspace, as
  `itero emit` does: one id per workspace in its answer. `POST
  /ws/<name>/events` stays the one workspace's.

## `itero secret` and `itero ws secret`

- `itero secret NAME`: kept in the OS secret store for every workspace,
  as today.
- `itero ws secret NAME`: kept for one workspace only, in the same store
  under a name of its own (the workspace's id with the secret's name, so
  a renamed workspace keeps its secrets).
- `$$NAME` is looked up in the workspace's own secrets, then the
  machine's, then the workspace's `.env`, then the environment: the first
  found. The workspace's own secret comes before the machine's, so one
  workspace can use another account without changing the others.
- `itero check`'s warning (a name both a secret and an environment
  variable) covers both kinds of secret.

## The commands that moved

A command that moved is an error naming its new form, for the release that moves it: `itero wf runs` says "itero ws wf
runs". They are hidden from `itero help` and completion. Nothing else
of the old forms stays (no aliases).

## What stays

- The HTTP API: `/health`, `/workspaces`, `/ws/<name>/...` as they are;
  `POST /events` is new on the machine's API.
- What each command prints, but for `itero info` (new) and `itero emit`
  (one line per workspace).
- Completion: names (workflows, actions, runs) come from the workspace of
  the folder, or from `--ws`.

## Where in the code

- `src/cli.rs`: the `Cmd` enum (top level), `WorkspaceCmd` (`info`,
  `check`, `which`, `emit`, `secret`, `wf`, `ac`, `cn`, with `--ws`), the
  hidden moved commands; the dispatch finds the workspace from `--ws`
  (through the machine's list) or the folder. `ActionArgsCmd`, unused
  since plan 24, goes.
- `src/cli/top.rs`: `info` for the machine (new), today's `info` as `ws
  info`; `emit` for every workspace.
- `src/engine/secrets.rs`: a workspace's secrets (set, get, the index of
  names), and the lookup order in `get_in`.
- `src/cli/complete.rs`: names from `--ws` when given.
- `src/engine/http.rs`: the machine's `POST /events`, through the same
  function as `itero emit`.
- Every message that names a command (`itero workflow list lists them`,
  `itero wf runs`, `itero action errors`, `itero check`, ...).
- Tests: `tests/cli.rs` and every test that runs a moved command; the
  examples' `test/workspace.js` helpers.
- Docs: the command pages (a page per top-level command; `workspace`
  with `info`, `check`, `which`, `emit`, `secret`; the `wf`, `ac`, `cn`
  pages under it in the menu), and every page and README that shows a
  moved command. `AGENTS.md` if it names one.

## Tests

- `tests/cli.rs`: each moved command under `ws`, with its alias; `--ws
  <name>` from another folder; `--ws` with a name not on the list is an
  error naming `itero ws list`; each old form an error naming the new;
  `itero info` with no daemon, and with a fake `/health`; `itero emit` to
  two workspaces on a list (the list in a temporary `XDG_STATE_HOME`), and
  to an empty list.
- `src/engine/secrets.rs` or `tests/secrets_bind.rs`: a workspace's
  secret before the machine's, the machine's before `.env`, with the
  store off (`ITERO_NO_SECRET_STORE`) the same order without the store.
- The examples' bun tests, `make verify`, `make test-linux`, `make
  test-docker` (the mail tests run `itero` commands).

## Shipping

One PR, commits by topic (the commands and their tests; secrets; docs
and examples), each under 30 files. A minor release, with a Breaking
paragraph: the commands that moved, `itero info` and `itero emit` now
the machine's, and an action's `itero emit` reaching every workspace.
