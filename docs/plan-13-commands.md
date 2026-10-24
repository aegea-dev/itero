# Plan 13: commands by kind, and docs a newcomer can read

The commands grew one at a time: `runs` and `errors` show the same runs,
`config` and `args` both write `itero.config.json` for different kinds,
nothing prints JSON for scripts, and the pages say what a command does
only after its options. This plan groups the commands by what they act
on, adds `--json` and tab completion, and rewrites the docs' openings in
plain words.

## Spec

```
itero daemon | emit | check | which | info | secret | service | watch | completion | help
itero workflow (wf)  list | runs | config get | config set
itero action         list | args get | args set
itero connector      list | args get | args set
```

- **The daily commands stay at the top:** `daemon`, `emit`, `check`,
  `which`, `info`, `secret`, `service`, `watch`, and `completion` (new).
- **`itero workflow`, also `itero wf`:**
  - `list`: each workflow, what starts it, its runs and the last
    (was `itero workflows`).
  - `runs [RUN]`: the runs, newest first, or one run step by step (was
    `itero runs`); `--errors` shows only runs that ended in error (was
    `itero errors`), `--workflow W` and `--action A` narrow them,
    `--limit N` and `--all` (internal workflows too).
  - `config get <workflow>` and `config set <workflow> <json | --file F>`:
    a workflow's settings (was `itero config`).
- **`itero action`:** `list`, and `args get <name>` / `args set <name>
  <json | --file F>` (was `itero args` for an action).
- **`itero connector`:** `list`, and `args get` / `args set` (was `itero
  args` for a connector).
- **Gone, with no aliases:** `itero workflows`, `runs`, `errors`, `config`,
  `args` (nobody depends on them yet). A breaking change: a minor
  version, with a Breaking note.
- **`--json`** on every command that prints something: `info`, `check`,
  `which`, `emit` (its id), `workflow list`, `workflow runs`, `workflow
  config get`, `action list`, `action args get`, `connector list`,
  `connector args get`, `service status`. Tables stay the default for
  people.
- **Tab completion:** `itero completion <bash | zsh | fish>` prints the
  script to install, as videre's does (clap_complete); commands, flags,
  and the names of workflows, actions, connectors and runs complete.
- **A secret and an environment variable of one name:** `$$NAME` takes
  the secret (the store first, then the environment). When both exist,
  `itero check` warns, and the daemon writes a warning to its log once
  per name, naming which one is used, so a workflow does not fail
  unnoticed on the wrong value.
- **`itero help`** opens with the motto's line: "Build the harness you
  need, from anything, for anything."

## Docs

- **Each command page opens** with one or two plain sentences: what it
  does and when to use it. Commands that could be confused say how they
  differ in that first sentence (`check` reads your files before anything
  runs; `workflow runs --errors` shows runs that already failed).
- **The Commands menu** names commands without `itero ` (`daemon`, `emit`,
  `workflow`, ...).
- **The three kinds carry their symbol and color** where they are named in
  the guides and command pages: ● workflow (green), ■ action (blue), ▲
  connector (red), in the logo's colors, light and dark.
- **One word, "event":** what starts or reaches a run is an event; its
  first mention on a page reads "events (messages)". The docs and the
  commands' help say event; the HTTP API (`POST /events`) and `itero emit`
  already do. The log's own line names stay as they are.
- **"Messages and connectors" becomes "Connectors"**, and opens with what
  an event can be: one an app sends (`itero emit`, `POST /events`), a line
  from a connector, a schedule, a file that changed, itero's own.
- **"Writing a workflow"** gains examples from `examples/` (the beacon,
  order, mail-alert, page-digest), each step explained in a sentence.
- **"Args and secrets"** says where `$$NAME` comes from (the store, else
  the environment), which wins when both exist, and that itero warns.

## Not in this plan

- Removing `--reply` (workflows post their results themselves): its own
  item, after this one, as `emit` changes again there.
- `itero action errors`: its own item, after this one, in the `action`
  group this plan makes.

## Tasks

1. The command tree in `src/cli.rs`: the groups, `wf` as an alias, the
   moves, `--errors`/`--workflow`/`--action`/`--limit` on `workflow runs`.
   Tests of each command's output move to the new names.
2. `--json` on each command above, one shared way to print it; a test per
   command that the JSON parses and holds what the table shows.
3. `itero completion`, with names completed from the workspace; a test
   that each shell's script is printed.
4. The warning for `$$NAME` in both places: `itero check` and the daemon's
   log, once per name. Tests with a secret store turned off and on.
5. Docs: the command pages (one per group, plus the top-level ones), the
   menu, the symbols and colors (one CSS class per kind), "event", the
   Connectors guide, the workflow guide's examples, the secrets guide.
6. `make verify`, `make test-linux`, `make test-docker`.

## Decided

- Groups by kind (`workflow`, `action`, `connector`), with the daily
  commands at the top, not a group for everything (`itero run list`
  would make the common commands longer).
- `runs` and `errors` are one command with a filter.
- A workflow's settings sit under `workflow config`, beside an action's
  `args` under `action args`: each kind keeps its own data. (The owner
  asked for `itero config set/get`; under `workflow` is my suggestion, for
  the same shape as `args`.)
- "event" over "message"; "(messages)" once per page for those who know
  the other word.
