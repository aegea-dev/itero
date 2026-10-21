# Plan 7: on as objects, watched files, IDs, metadata, the log

> For agentic workers: execute task by task (superpowers:executing-plans).
> Steps use checkboxes. Kept short on purpose: the owner's rule is KISS.

**Goal:** itero does what the latest `docs/design-simple.md` says: `on`
entries as objects, files watched by the daemon, IDs and random names,
metadata, a JSON log, the daemon's own messages and workflows, and
`itero emit`, `itero info` and `itero runs <run>` that tell you what
happened.

**Architecture:** the engine stays (journal, daemon passes, pure manager,
workers). New: one file watcher thread in the daemon; a logger writing
JSON lines; built-in workflows carried in the binary and merged with the
workspace's; journal schema 6 for workspace, workflow and run identity.

**Tech:** Rust 1.96 (edition 2024), rusqlite, croner, uuid (v7), libc
(kqueue, inotify). No new crates.

**Spec:** `docs/design-simple.md`.

## Global constraints

- KISS: the smallest code that does what the spec says.
- No em dash and no AI branding in public text.
- Stage files by name, never `git add -A`.
- Crates: MIT or Apache-2.0 only; none expected here.
- Test fixtures use non-ASCII text (`ı`, `ş`, `ğ`, `ü`, `é`).
- Secrets never reach the journal, the log, the state or a command line.
- Each task ends with `make fmt-check`, `make lint-check`, `make test`
  green, and one commit; `make test-linux` before the PR.
- Branch `on-and-log` (it has the spec); one PR at the end.

---

### Task 1: `on` as objects

**Files:** `src/flow.rs`, `src/engine/daemon.rs` (schedules, routing),
`tests/flow.rs`, `tests/steps.rs`, `tests/daemon.rs`, every example's
workflows, `tests/*` fixtures that write workflows.

**Interfaces (produces):**
```rust
pub struct On {
    pub events: Vec<String>,          // {"event": "order.placed"}
    pub schedules: Vec<String>,       // {"schedule": "0 9 * * *"}
    pub paths: Vec<String>,           // {"path": "/var/mail/$$USER"}, as written
    pub has: Vec<String>,             // {"has": "$order"} or a list: "order", "$$API" kept with its $$
    pub equal: Map<String, Value>,    // {"$paid": true, "$stock": true}
}
pub fn holds(on: &On, state: &Map<String, Value>, defined: &dyn Fn(&str) -> bool) -> bool;
pub fn starts(on: &On) -> bool;       // has an event, a schedule or a path
```
`On.messages` becomes `On.events`; `present` and `defined` become `has`
(a `$$` name in `has` is checked with `defined`).

- [ ] Tests first (`tests/flow.rs`): each entry kind; several values in
  one object; `has` as a string and as a list; refused with the file
  named: a string entry (`"order.placed"`), an object with no known key,
  `{"event": "tek"}`, a bad cron, `{"$$X": 1}`, `{"has": 3}`, a path that
  is not text. Step `on` takes the same list.
- [ ] Run, fail; change `flow.rs`, then the daemon (schedules from
  `schedules`; a message starts the workflows with its type in
  `events`); `paths` parsed but not yet watched (task 6).
- [ ] Rewrite every workflow in `examples/`, `tests/` and the docs' code to
  the new form; `make test`, `make test-docker` pass.
- [ ] Commit: "on entries are objects: event, schedule, path, $name, has".

### Task 2: metadata in files

**Files:** `src/flow.rs`, `src/workspace.rs`, `src/engine/daemon.rs`,
`tests/flow.rs`, `tests/workspace.rs`, `tests/daemon.rs`.

**Interfaces (produces):**
```rust
// flow.rs
pub struct Meta { pub description: Option<String>, pub from: Option<String>,
                  pub until: Option<String>, pub off: bool, pub other: Map<String, Value> }
pub struct Flow { /* … */ pub meta: Meta }
pub fn active(meta: &Meta, now: DateTime<Utc>) -> bool;   // not off, within from/until
// workspace.rs
pub struct Limits { pub workflows: usize, pub runs: usize, pub steps: usize,
                    pub space: u64, pub keep: Duration, pub log: u64, pub logs: usize }
impl Workspace { pub meta: Map<String, Value>, pub limits: Limits }   // [meta], [limits]
```
Defaults: 100, 1000, 8, 1 GB, 1 day, 10 MB, 3.

- [ ] Tests first: `meta` with the known keys and others kept; `id`,
  `name`, `created`, `path` or `version` in `meta` refused; `from` and
  `until` as a date or an RFC 3339 time; `[meta]` free; `[limits]` sizes
  (`"1GB"`, `"10MB"`) and durations; an unknown limit refused.
- [ ] Daemon: a workflow that is `off`, or outside `from`/`until`, starts
  nothing (messages, schedules); more workflows than `limits.workflows` is
  an error at check and at start; with `limits.runs` runs under way, the
  pass leaves new messages unhandled for the next pass; `limits.steps`
  replaces `MAX_WORKERS`; `limits.keep` replaces `KEEP`.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "Metadata: a workflow's meta, the workspace's [meta] and [limits]".

### Task 3: IDs and names

**Files:** create `src/names.rs` (the word lists); modify
`src/engine/journal.rs` (migration 6), `src/engine/daemon.rs`,
`src/engine/http.rs`, `src/cli.rs`; tests `tests/journal.rs`,
`tests/cli.rs`, `tests/http.rs`.

**Interfaces (produces):**
```rust
// names.rs: an adjective and a noun, our own lists of 64 words each
pub fn random() -> String;                                  // "quiet-river"
// journal.rs
pub struct Identity { pub id: String, pub name: String, pub created: String, pub itero: String }
impl Journal {
    pub fn workspace(&self) -> Result<Identity>;            // made with the journal
    pub fn workflow_id(&self, name: &str, at: &str) -> Result<Identity>;  // made when first seen
    pub fn run_by(&self, id_or_name: &str) -> Result<Option<Run>>;
}
pub struct Run { /* … */ pub name: String }
// daemon.rs
pub struct Daemon { pub id: String, pub name: String, pub started: String, pub pid: u32 }
```
Migration 6: `meta` gets `workspace` (id, name, created, itero version);
table `workflow_ids (name PRIMARY KEY, id, created)`; `runs.name`.

- [ ] Tests first: a new journal has a workspace identity, the same on
  reopening; an old journal gets one in migration 6; a workflow's id stays
  across edits of its file; a run has a two-word name; `run_by` finds a
  run by id or name; two runs with one name: the newest, and `itero runs
  <name>` says there are two.
- [ ] `itero info`: the workspace (id, name, created, path, `[meta]`,
  limits) and, when one answers on its port, the daemon (id, name,
  started, pid, itero version).
- [ ] `itero runs <id|name>`: the run as a timeline from the journal: each
  step, how it went, what it printed; where it waits and since when; the
  state at the end.
- [ ] `/health` and `GET /runs/{id}` add the identities and the run's
  name; `itero check` prints each workflow's id, version, `meta`, its last
  run and how it ended, and its runs so far; `itero info` shows the
  daemon's host too.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "IDs and names for the workspace, daemon lives, workflows and runs".

### Task 4: the JSON log

**Files:** create `src/engine/log.rs`; modify `src/engine/daemon.rs`,
`src/engine/steps.rs` (structured notes), `src/sources/supervise.rs`,
`src/cli.rs`; test `tests/log.rs`.

**Interfaces (produces):**
```rust
// log.rs: one JSON object per line, opened for each line, never a secret
pub struct Log { path: PathBuf, workspace: String, daemon: String }
impl Log {
    pub fn open(ws: &Workspace, workspace: &str, daemon: &str) -> Log;
    pub fn write(&self, event: &str, fields: serde_json::Value);   // adds time, workspace, daemon
}
// steps.rs: StepAnswer gains
pub notes: Vec<Value>,   // {"event": "step.skipped", "step": "#ship"}, {"event": "run.ended", "status", "why"}
```

- [ ] Tests first (`tests/log.rs`, through `run_once` and a daemon
  process): after the order example, `.itero/itero.log` has, in order,
  `message` (with `started`), `run.started`, `step.started` twice,
  `step.done`, `step.waiting`, …, `run.ended` with `done`; each line has
  `time`, `workspace`, `daemon`, `event`; a skipped step is
  `step.skipped`; a failing step is `step.failed` with `why`; a
  `$$SECRET`'s value appears nowhere in the log; `daemon.started` and
  `daemon.stopped` from a daemon process, `daemon.started` carrying every
  field of the workspace and the daemon (ids, names, created, path,
  itero version); lines about a run carry its `run_name` too; a source's
  start and end.
- [ ] Run, fail, implement, pass. The daemon's pass lines go to stderr
  only when it is a terminal, so under the service `.itero/daemon.log`
  holds only what a crash leaves.
- [ ] Commit: "A JSON log: .itero/itero.log".

### Task 5: the daemon's own messages and workflows

**Files:** `src/engine/daemon.rs`, `src/engine/events.rs`,
`src/engine/worker.rs`, `src/workspace.rs`, `src/flow.rs`, `src/cli.rs`,
`src/engine/http.rs`; create `src/builtin/` (`keep.json`, `logs.json`,
both `"meta": {"internal": true}`, embedded with `include_str!`); tests
`tests/daemon.rs`, `tests/log.rs`, `tests/cli.rs`.

**Interfaces (produces):**
```rust
pub const OWN: &str = "itero.";                   // events.rs: types only the daemon sends
pub fn builtin_flows() -> Vec<(String, Value)>;    // workspace.rs: merged unless a file has the name
// flow.rs: Meta gains
pub internal: bool,
// worker.rs: built-in actions beside NOTIFY
pub const FORGET: &str = "forget";           // the journal's prune, by keep and space
pub const ROLL_LOG: &str = "roll-log";       // itero.log -> .1 -> … keeping limits.logs
```
No name says a workflow is internal: only its `meta` does.

- [ ] Tests first:
  - any workflow with `"meta": {"internal": true}`: listed apart by
    `itero check`; its runs left out of `itero runs` and `GET /runs`
    unless `--all` / `?all=true`, but in `itero errors`; not counted
    against `limits.runs` or `limits.workflows`; its log lines carry
    `"internal": true`;
  - `itero.daemon.started`, `itero.run.error`, `itero.run.failed`,
    `itero.source.gave-up`, `itero.space.full`, `itero.log.full` are sent
    when their spec says, with their data;
  - `itero emit itero.x.y`, `/events` and a source line of an `itero.`
    type are refused;
  - a workflow on `itero.run.error` runs when another workflow's run ends
    in error, and never for its own runs;
  - `keep` runs hourly and forgets ended runs past `keep`, with
    their messages; `itero.space.full` makes it forget sooner; still past
    `space` after that, the daemon starts no new run and logs an `error`
    line until there is room;
  - past `limits.log`, `itero.log.full` is sent once, and `logs`
    rolls the log keeping `limits.logs`; `workflows/logs.json`
    replaces it; `"meta": {"off": true}` there turns it off.
- [ ] Run, fail, implement, pass. The prune at the end of each pass goes:
  `keep` does it.
- [ ] Commit: "The daemon's own messages and built-in workflows".

### Task 6: watching files in the daemon

**Files:** `src/watch/mod.rs`, `src/watch/kqueue.rs`,
`src/watch/inotify.rs`, `src/engine/daemon.rs`, `src/cli.rs`; tests
`tests/watch.rs`, `tests/daemon.rs`.

**Interfaces (produces):**
```rust
// watch: one backend for many paths
pub fn open_many(paths: &[PathBuf]) -> std::io::Result<Box<dyn Backend>>;
pub fn open(path: &Path) -> std::io::Result<Box<dyn Backend>>;   // open_many(&[path])
// daemon: a thread holding the backend; each debounced change of a path a
// workflow names is published as an `itero.path.changed` message
// {"path", "kind"}, routed to the workflows with that path in `on`
pub fn expand(path: &str) -> Result<PathBuf, String>;            // ~/ and $$NAME from the environment
```
The set of paths is read again at each pass; the thread adds and drops
watches when workflows change.

- [ ] Tests first (`tests/watch.rs`): `open_many` reports changes to
  three files from one backend; a path that comes later is seen.
  (`tests/daemon.rs`, with a daemon process): a workflow on `{"path":
  "<temp>/gelen kutusu"}` runs once for a burst of appends; editing the
  workflow to another path moves the watch; `$$` in a path is filled from
  the environment; one daemon thread, however many paths (checked on
  Linux by counting `/proc/self/task`, on macOS by the thread count).
- [ ] Run on macOS and with `make test-linux`; fail; implement; pass.
- [ ] Commit: "Files are watched by the daemon: one watcher for every path".

### Task 7: `itero emit` says what happens

**Files:** `src/cli.rs`, `src/engine/events.rs`, `tests/cli.rs`.

**Interfaces (produces):**
```rust
pub struct Fate { pub starts: Vec<String>, pub nearest: Option<String> }
pub fn fate(ws: &Workspace, kind: &str, data: &Map<String, Value>) -> Result<Fate>;
```

- [ ] Tests first: emitting a type a workflow is on prints its name
  (`starts: order`); a typo prints the nearest type (`order.places`:
  `did you mean order.placed?`); with no daemon on the port, it adds
  `no daemon is running: it waits for itero daemon`; the message is saved
  in every case; exit 0.
- [ ] Run, fail, implement (nearest by edit distance among the `event`
  types of all workflows), pass.
- [ ] Commit: "itero emit says what a message starts".

### Task 8: examples and docs

**Files:** `examples/**`, `docs/src/content/docs/**`, `README.md`.

- [ ] Examples write their output into `.itero/` (`order`: `.itero/sent.log`,
  `.itero/results.log`); add the photos example (`{"path": …}`) if it
  stays a few lines; `tests/examples.rs` follows.
- [ ] Docs: the `on` objects; Watching files; How it runs (the picture
  from the spec); IDs and names; Metadata; The log with the `jq` recipes;
  The daemon's own workflows; `itero info`, `itero runs <run>`, `emit`'s
  answer. "Manager" and "worker" leave the guides and glossary.
- [ ] `cd docs && yarn build`; every link resolves; the quickstart works
  as written.
- [ ] Commit: "Examples and docs for on objects, the log, IDs and metadata".

### Task 9: wrap up

- [ ] `make fmt-check lint-check test test-docker test-linux`; `yarn build`.
- [ ] Push `on-and-log` and open the PR, when the owner says so.
- [ ] After the merge, with the owner's yes: tag `v0.2.0`.
