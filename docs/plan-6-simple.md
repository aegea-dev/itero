# Plan 6: itero, simple

> For agentic workers: execute task by task (superpowers:executing-plans).
> Steps use checkboxes. Kept short on purpose: the owner's rule is KISS.

**Goal:** itero reads the simple files of `docs/design-simple.md`: a workflow
is `on` and `steps`; an action is one executable file; values are found by
name; exit codes 0, 75 and others say how a step went.

**Architecture:** the engine underneath stays: the journal of events, the
daemon that routes and saves, a pure manager that decides, workers that run
one job each. What changes is what the manager decides over: a run becomes
a step number and a state (a JSON object), instead of a state machine. The
new pieces are built next to the old ones (tasks 1 to 3), the daemon
switches to them and the old ones are deleted (task 4), then the commands,
sources, examples and docs follow.

**Tech:** Rust 1.96 (edition 2024), SQLite (rusqlite), croner, keyring,
axum. Shell, `curl` and `jq` in the examples.

**Spec:** `docs/design-simple.md`.

## Global constraints

- KISS: the smallest code that does what the spec says; nothing it does
  not say.
- No em dash and no AI branding in public text (commits, PRs, docs, code
  comments).
- Stage files by name, never `git add -A`.
- Crates: MIT or Apache-2.0 only, `~x.y.z` requirements.
- Test fixtures use non-ASCII text (`ı`, `ş`, `ğ`, `ü`, `é`) in neutral
  words.
- Secrets never reach the journal, a log, the state or a command line.
- Every task ends with `make fmt-check`, `make lint-check` and `make test`
  green, then one commit.
- Branch `simple` from `main`; one PR when the plan is done.

---

### Task 1: the files

Read the new workspace files, next to the old reader.

**Files:**
- Create: `src/flow.rs` (a workflow file), `tests/flow.rs`
- Modify: `src/workspace.rs` (new readers), `src/lib.rs`, `tests/workspace.rs`

**Interfaces (produces):**
```rust
// src/flow.rs
pub struct Flow { pub on: Vec<On>, pub steps: Vec<Step>, pub timeout: Option<std::time::Duration> }
pub enum On { Message(String), Schedule(String) }        // a schedule has spaces
pub struct Step { pub action: String, pub version: Option<String>, pub data: serde_json::Map<String, Value> }
pub fn parse(text: &str) -> Result<Flow, String>;
pub const WAIT: std::time::Duration;                      // 7 days
// src/workspace.rs
impl Workspace {
    pub fn flows(&self) -> Result<Vec<(String, Flow)>>;           // name = file stem
    pub fn action_file(&self, name: &str, version: Option<&str>) -> Result<PathBuf>;
    pub fn source_files(&self) -> Result<Vec<(String, PathBuf)>>; // type = file name
    pub fn args(&self, action: &str) -> Result<Option<Value>>;    // args/<action>.json
}
```

- [ ] Tests first (`tests/flow.rs`, `tests/workspace.rs`):
  - `{"on": ["mail.changed", "0 * * * *"], "steps": ["mail-check", "notify"]}`
    reads as one message, one schedule, two steps.
  - A step `{"imap-check@0.5": {"host": "posta.örnek.test"}}` has its
    action, version and data.
  - Errors, each naming the file: an unknown key, no `steps`, an empty
    `on`, a bad cron line, a step object with two keys, a bad `timeout`.
  - `action_file("notify", None)` picks `notify@0.10` over `notify@0.2`
    (versions compare as numbers) and a plain `notify` file when it is the
    only one; a file that is not executable is an error.
  - `source_files` lists `sources/mail.changed` as type `mail.changed`.
  - `args` reads an object, or a list of objects; anything else is an
    error.
- [ ] Run them, see them fail; write `flow.rs` and the readers; see them
  pass.
- [ ] Commit: "The simple workspace files: workflows, actions, sources, args".

### Task 2: the manager, over steps

A new pure manager for runs of steps. Same contract as today's: JSON in,
JSON out, no clock, no files.

**Files:**
- Create: `src/engine/steps.rs`, `tests/steps.rs`
- Modify: `src/engine/mod.rs` (new types)

**Interfaces (produces):**
```rust
pub struct Runtime {                       // a run, as the journal keeps it
    pub step: usize,                       // the step running or waiting
    pub state: Map<String, Value>,
    pub waiting: bool,
    pub ended: Option<Ended>,              // Done | Failed { why }
}
pub struct StepInput {
    pub now: String,
    pub flow: Flow,                        // the run's pinned workflow
    pub runtime: Runtime,
    pub args: Option<Value>,               // the current step's args file
    pub event: Event,                      // see below
}
pub struct StepAnswer {
    pub runtime: Runtime,
    pub job: Option<StepJob>,              // the step to run now
    pub split: Vec<Value>,                 // args items: one new run each
    pub timer: Option<String>,             // waiting: when it gives up
    pub log: Vec<String>,
}
pub struct StepJob { pub step: usize, pub action: String, pub version: Option<String>,
                     pub data: Map<String, Value>, pub item: Option<Value> }
pub fn decide(input: &StepInput) -> Result<StepAnswer, String>;
```
Events it handles: `run.started` (data: the message), `action.finished`
(`{"output": {...} | null}`), `action.waiting`, `action.failed`
(`{"error"}`), `message` (a workspace message copied to a waiting run),
`timer.fired`.

- [ ] Tests first (`tests/steps.rs`), one per rule of the spec:
  - started with `{"sipariş": 43}`: job for step 0, state is the message.
  - output `{"text": "2 yeni"}` joins the state, job for step 1; a field
    set to `null` is removed.
  - output `null` (nothing printed): the run ends, done.
  - after the last step: done.
  - `action.waiting`: `waiting`, no job, timer at now + 7 days (or the
    workflow's `timeout`).
  - a `message` while waiting: `{"order": 43, "amount": 120}` with state
    `{"order": 43}` joins the state and runs the step again; with state
    `{"order": 42}`, or with no field in common, it is ignored.
  - `timer.fired` while waiting: failed, "waited 7d for data".
  - args a list of two objects: `split` has both items, the run itself
    ends as split; a run started with an `item` runs its step with it.
  - `action.failed`: failed, with the error as why.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "A manager over steps: state, waiting, matching, split".

### Task 3: the worker

Run one action file the new way.

**Files:**
- Modify: `src/engine/worker.rs`, `tests/worker.rs`

**Interfaces (produces):**
```rust
pub struct StepRun {                        // what `itero worker` reads
    pub file: PathBuf,                      // the action, or "notify"
    pub env: BTreeMap<String, String>,      // the bound values
    pub state: Map<String, Value>,          // stdin
    pub memory: PathBuf,                    // $ITERO_MEMORY
    pub timeout: u64,                       // 600
}
pub enum Outcome { Output(Option<Map<String, Value>>), NotYet }
pub fn run_step(r: &StepRun) -> Result<Outcome, Failure>;
```
The worker prints `{"output": ...}` or `{"waiting": true}` and exits 0;
its own exit 75 (`LOST_EXIT`) still means a lost job. The action's 75 is
read by the worker, never passed through.

- [ ] Tests first (`tests/worker.rs`, with small `#!/bin/sh` actions written
  into a temp dir):
  - the action sees `$host` from env and the state as JSON on stdin.
  - prints `{"metin": "çay"}`: `Output(Some(..))`; prints nothing:
    `Output(None)`; prints `[1]` or text: an error saying it is not a JSON
    object.
  - exits 75: `NotYet`; exits 3 with stderr "bağlantı yok": an error ending
    with that text.
  - writes then reads `$ITERO_MEMORY` across two runs.
  - `notify` with `title` and `text` writes to `ITERO_NOTIFY_LOG`.
- [ ] Run, fail, implement, pass. Leave the old `run()` in place until task 4.
- [ ] Commit: "The worker runs an action file: env, state, exit codes".

### Task 4: the switch

The daemon and the journal use tasks 1 to 3; the old format goes.

**Files:**
- Modify: `src/engine/daemon.rs`, `src/engine/journal.rs`,
  `src/engine/mod.rs`, `src/engine/events.rs`, `src/engine/schedule.rs`,
  `src/cli.rs` (`itero manager` and `itero worker` use the new types)
- Delete: `src/workflow/` (format, validate, actions, expr),
  `src/engine/manager.rs`, `src/engine/eval.rs`, `src/engine/builtins.rs`;
  `cel-interpreter` and `reqwest` from `Cargo.toml`
- Tests: rewrite `tests/daemon.rs`, `tests/journal.rs`, `tests/breaker.rs`,
  `tests/events.rs`, `tests/schedule.rs`; delete `tests/manager.rs`,
  `tests/pubsub.rs`, `tests/limits.rs`, `tests/containers.rs`,
  `tests/container_clock.rs`

What the daemon does:
- **Binding** (in `job()`): a value is the first found in the step's data,
  the args (its item, after a split), the state, then secrets. Args and
  secrets are keyed by the action's name without its version. Every name
  that is a valid environment variable becomes one; strings as they are,
  other values as JSON text. `$ITERO_MEMORY` is
  `.itero/memory/<workflow>`, one per workflow. A workspace action named
  `notify` is used instead of the built-in one.
- **Secrets** (in `secrets.rs`, used here, extended in task 5):
  ```rust
  pub fn names_for(action: &str) -> Vec<String>;   // from the index and the environment
  pub fn get_for(action: &str, name: &str, user: Option<&str>) -> Option<String>;
  ```
  Here `get_for` looks up the shared `<action>.<name>`, then the daemon's
  environment as `IMAP_CHECK_PASSWORD`; task 5 adds accounts.
- **Messages**: a message for the workspace starts a run of each workflow
  with its type in `on` (one run at a time per workflow, the rest queued),
  and is copied as `message` to each waiting run; the manager decides
  whether it matches.
- **Schedules**: the `on` entries with spaces, through `schedule.rs`.
- **Defaults**: a step may take 600 s; a lost job runs again up to 3 times;
  5 failed runs in a row pause the workflow.
- **Journal**: `runs.runtime` holds the new `Runtime`. Migration 5 ends
  the runs from the old format: they are deleted with their timers, jobs and
  queued starts, and unhandled events of theirs are marked handled. Events
  stay as history.
- **Splits**: each item starts a new run at the same step, with a copy of
  the state and the item.

- [ ] Tests first (`tests/daemon.rs`), through `run_once` and shell actions:
  - mail-alert: `mail.changed` starts a run; `mail-check` prints
    `{"text": "1 yeni: Toplantı"}`, `notify` shows it; nothing new ends the
    run with no notification.
  - order: `order.placed` `{"order": 43, "total": 120}`; `check-payment`
    exits 75; `payment.received` `{"order": 42}` changes nothing;
    `{"order": 43, "amount": 120}` lets it finish, and `ship` and
    `send-mail` run.
  - a stock message matching two waiting runs moves both.
  - binding order: step data wins over args, args over state, state over a
    secret.
  - a list of two args items: two runs from that step, one after another.
  - waiting past `"timeout": "1m"`: the run fails and `itero errors` says
    so.
  - schedule `"0 * * * *"` starts a run once an hour.
  - the kill tests still hold: the daemon killed at each `die_at` point
    loses and repeats no step.
  - a journal from schema 4 opens, its old runs gone, its events kept.
- [ ] Run, fail, switch, delete, pass.
- [ ] Commit: "The daemon runs the simple workflows; the old format goes".

### Task 5: args and secrets

**Files:**
- Modify: `src/cli.rs`, `src/engine/secrets.rs`; `Cargo.toml` (`rpassword`,
  Apache-2.0, for reading a secret without echo)
- Test: `tests/cli.rs` (new)

Commands:
```
itero args <action> --json '<object or list>' | --file <path>
itero secret <action> <name> [--for <account>] [--file <path> | --json -]
```
- Secrets are kept in the OS store as `<action>.<name>`, or
  `<action>.<name>/<account>` with `--for` or a JSON object of accounts.
  The index of names (`itero.names`) lists them, so the daemon knows which
  to bind for an action.
- `get_for` (from task 4) gains accounts. For a step with `$user`: the
  account's secret, then the shared one, then the daemon's environment as
  `IMAP_CHECK_PASSWORD` (letters upper-cased, `-` and `.` as `_`), for
  machines without a store, CI among them.

- [ ] Tests first: `itero args` writes `args/imap-check.json` and refuses
  text that is not an object or a list of objects; `itero secret` with
  `ITERO_NO_SECRET_STORE` says the store is off; the lookup order (account,
  shared, environment) as a unit test over a fake store; a value never
  appears in `ps`-visible arguments (the command takes none).
- [ ] Run, fail, implement, pass.
- [ ] Commit: "itero args and itero secret".

### Task 6: sources and `itero watch`

**Files:**
- Modify: `src/sources/mod.rs`, `src/sources/supervise.rs`, `src/cli.rs`
- Tests: rewrite `tests/sources.rs`, `tests/watch.rs`

- A source is an executable in `sources/`; each line it prints is the data
  of one message of its file name's type. A line that is not a JSON object
  is logged and skipped. No settings line on stdin, no cursors, no
  container sources (the `cursors` table stays, unused).
- `itero watch <path>` prints `{"path": "<path>"}` each time it changes,
  with the existing kqueue/inotify backend and debouncer.
- The supervisor's restart policy stays.

- [ ] Tests first: a `sources/mail.changed` script printing two lines
  starts two runs; a bad line is skipped; a source that keeps exiting is
  given up on; `itero watch` prints a line when the file changes.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "Sources are executables; itero watch".

### Task 7: commands and HTTP, tidied

**Files:** `src/cli.rs`, `src/engine/http.rs`, `tests/http.rs`, `tests/cli.rs`

- `itero emit <type> --json '<object>'` (was `--data`).
- `itero runs`: id, workflow, step, waiting or not, started, ended.
- `itero errors`: when, workflow, run, the step's action, why.
- `itero check`: every workflow, action and source; what is wrong, with
  the file.
- `itero resume <workflow>` stays as it is.
- `itero memory` goes (memory is a file now); `GET /workflows` shows each
  workflow's `on`; `GET /workflows/{name}/memory` goes.

- [ ] Tests first for each line above; run, fail, implement, pass.
- [ ] Commit: "Commands and HTTP for the simple files".

### Task 8: the examples

**Files:**
- `examples/mail-check/` becomes `examples/mail-alert/`: `workflows/mail-alert.json`,
  `actions/mail-check` (the shell reader of today, reading `$mailbox` and
  `$ITERO_MEMORY`), `sources/mail.changed` (`itero watch`).
- `examples/mail-servers/`: `workflows/imap-alert.json`,
  `workflows/pop3-alert.json`, `actions/imap-check` and `actions/pop3-check`
  (shell around `curl`, password to `curl` on stdin with `-K -`);
  the Python files, `sources/` and the containers go.
- `examples/order/` and `examples/page-digest/`, as in the spec.
- Tests: `tests/mail.rs` (mail-alert), `tests/mail_servers.rs` (curl
  against GreenMail; Docker only for GreenMail), `tests/examples.rs`
  (`itero check` passes on every example; the order example runs end to
  end); `scripts/test-docker.sh` runs `mail_servers` only.

- [ ] Tests first; run, fail, write the examples, pass (`make test` and
  `make test-docker`).
- [ ] Commit: "The examples in the simple form; mail over curl".

### Task 9: the docs

**Files:** `docs/src/content/docs/**`, `README.md`, `docs/design.md` (a
note at the top: the formats are in `design-simple.md`).

- Guides: getting started, workflows, actions, messages and sources,
  secrets and args, service. Reference: workflow file, action, commands,
  HTTP, glossary. Pages for removed things go (triggers, containers,
  the old action file).
- `cd docs && yarn build` builds, every link resolves.

- [ ] Rewrite, build, read every page once as a newcomer would.
- [ ] Commit: "Docs for the simple files".

### Task 10: wrap up

- [ ] `make fmt-check lint-check test test-docker`, `yarn build`.
- [ ] Push `simple`, open the PR (only when the owner says so).
- [ ] With the owner's yes: reinstall itero at `~/itero` and move its
  mail check to the new form (`workflows/mail-alert.json`,
  `actions/mail-check`, `sources/mail.changed`), then restart its service.
