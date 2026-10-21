# Plan 6: itero, simple

> For agentic workers: execute task by task (superpowers:executing-plans).
> Steps use checkboxes. Kept short on purpose: the owner's rule is KISS.

**Goal:** itero reads the simple files of `docs/design-simple.md`: `#`
actions, workflows as steps, side-by-side lists, `on` conditions with `$`
and `$$`, `success`/`failure`/`error`, exit codes 0 and 75.

**Architecture:** the engine underneath stays: the journal of events, the
daemon that routes and saves, a pure manager that decides, workers that run
one job each. A run becomes a position in its steps plus a state (a JSON
object) instead of a state machine. The new pieces are built next to the
old ones (tasks 1 to 3); the daemon switches to them and the old ones are
deleted (task 4); commands, sources, examples and docs follow.

**Tech:** Rust 1.96 (edition 2024), SQLite (rusqlite), croner, keyring,
axum, uuid (v7). Shell, `curl` and `jq` in the examples.

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
- Kept small on purpose (the spec's "Later"): a side-by-side list holds
  actions only, no workflows; no pausing; no settings for lists.

---

### Task 1: the files and the names

**Files:**
- Create: `src/flow.rs`, `tests/flow.rs`
- Modify: `src/workspace.rs`, `src/lib.rs`, `tests/workspace.rs`

**Interfaces (produces):**
```rust
// src/flow.rs
pub struct Flow {
    pub on: On,
    pub steps: Vec<Step>,
    pub success: Vec<Step>, pub failure: Vec<Step>, pub error: Vec<Step>,
    pub timeout: Option<std::time::Duration>,       // None: 1 day
}
pub struct On {
    pub messages: Vec<String>,                       // "order.placed"
    pub schedules: Vec<String>,                      // "0 9 * * *"
    pub present: Vec<String>,                        // "$order" -> "order"
    pub defined: Vec<String>,                        // "$$API" -> "API"
    pub equal: serde_json::Map<String, Value>,       // {"$order": 43}
}
pub enum Step {
    One { name: Name, data: Map<String, Value>, on: Option<On> },
    Side(Vec<Name>),                                 // actions only
}
pub enum Name { Action { name: String, version: Option<String> }, Workflow(String) }
pub fn parse(text: &str) -> Result<Flow, String>;
pub fn holds(on: &On, state: &Map<String, Value>, defined: &dyn Fn(&str) -> bool) -> bool;
pub fn is_message_type(s: &str) -> bool;             // two or more words joined by "."
// src/workspace.rs
impl Workspace {
    pub fn flows(&self) -> Result<Vec<(String, Flow)>>;          // name = file stem
    pub fn action_file(&self, name: &str, version: Option<&str>) -> Result<PathBuf>;
    pub fn source_files(&self) -> Result<Vec<(String, PathBuf)>>;
    pub fn args(&self, action: &str) -> Result<Option<Value>>;
}
```

- [ ] Tests first (`tests/flow.rs`, `tests/workspace.rs`):
  - `on: ["mail.received", "0 * * * *"]`: one message, one schedule.
  - `on: ["order.received", "$order", "$$API"]`: a message, a present
    value, a defined name. `on: {"$order": 43}`: an equal value.
  - `holds`: exact equality (`0` does not equal `false`); a present value;
    a defined name; all conditions together.
  - steps: `"#check-payment"`, `"order-process"`, `["#a", "#b"]`,
    `{"#ship": {"carrier": "PTT", "on": {"$paid": true}}}` with its data
    and condition; `"#notify@0.2"` with its version.
  - errors naming the file: an unknown key; no `steps`; a workflow inside
    a side-by-side list; a message type of one word; a bad cron line; a
    bad `timeout`; a step object with two keys.
  - `action_file("notify", None)` picks `notify@0.10` over `notify@0.2`
    (numbers, not text) and a plain `notify` when it is the only one; a
    file that is not executable is an error.
  - `source_files` lists `sources/mail.received` as type `mail.received`;
    a source whose name is not a message type is an error.
  - `args` reads an object or a list of objects; anything else is an
    error.
- [ ] Run, fail; write `flow.rs` and the readers; pass.
- [ ] Commit: "The simple files: workflows, actions, sources, args".

### Task 2: the manager, over steps

A new pure manager. Same contract as today's: JSON in, JSON out, no
clock, no files, no environment.

**Files:** create `src/engine/steps.rs`, `tests/steps.rs`; modify
`src/engine/mod.rs`.

**Interfaces (produces):**
```rust
pub struct Runtime {
    pub frames: Vec<Frame>,                 // [0] the run's workflow, then workflows used as steps
    pub branches: Vec<Branch>,              // the step running now: one, or a side-by-side list
    pub phase: Phase,                       // Steps | Success | Failure | Error
    pub state: Map<String, Value>,
    pub item: Option<Value>,                // after a split: this run's args item
    pub waiting_since: Option<String>,
    pub ended: Option<Ended>,               // Done | Failed | Error { why }
}
pub struct Frame { pub workflow: String, pub step: usize }
pub struct Branch { pub action: String, pub output: Option<Map<String, Value>>, pub waiting: bool }
pub struct StepInput {
    pub now: String,
    pub flows: BTreeMap<String, Flow>,      // the run's pinned workflows, by name
    pub args: BTreeMap<String, Value>,      // args files of the actions they use
    pub defined: Vec<String>,               // the $$ names that are defined
    pub runtime: Runtime,
    pub event: Event,
}
pub struct StepAnswer {
    pub runtime: Runtime,
    pub jobs: Vec<StepJob>,
    pub split: Vec<Value>,                  // args items: a new run each
    pub timers: Vec<String>,                // the hourly retry and the deadline
    pub log: Vec<String>,
}
pub struct StepJob { pub action: String, pub version: Option<String>,
                     pub data: Map<String, Value>, pub item: Option<Value> }
pub fn decide(input: &StepInput) -> Result<StepAnswer, String>;
```
Events: `run.started` (data: the message), `action.finished`
(`{"action", "output"}`), `action.waiting` (`{"action", "output"}`),
`action.failed` (`{"action", "error"}`), `message` (a workspace message
copied to the run), `timer.fired`.

- [ ] Tests first (`tests/steps.rs`), one per rule:
  - started with `{"sipariş": 43}`: a job for step 0; the state is the
    message.
  - an output joins the state; `null` removes a field; printing nothing
    adds nothing; the next step's job follows.
  - a side-by-side list: two jobs at once; the next step only after both;
    a field printed by both becomes `{"#a": …, "#b": …}`, one printed by
    one stays plain.
  - a step whose `on` does not hold is skipped; a workflow used as a step
    whose `on` does not hold is skipped; one that holds runs its steps with
    the same state, then the outer workflow goes on.
  - `action.waiting`: its output joins the state, the run waits; timers at
    now + 1 hour and now + 1 day (or `timeout`). A matching `message`
    joins the state and runs the step again; one with no shared field, or
    one that disagrees on a shared field, is ignored. The hourly timer runs
    it again with `ITERO_WAITING_SINCE`; the deadline ends the run in
    `error`.
  - an action error stops the steps: the `error` list runs, then the run
    ends as `error` with `why` and `failed_step` in the state.
  - all steps done with `"success": false`: `failure` runs, the run ends
    `failed`; otherwise `success` runs, the run ends `done`; a step in
    those lists that breaks ends the run as `error`, with no other list.
  - args a list of two objects: `split` has both items; a run with an
    `item` runs its step with it.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "A manager over steps".

### Task 3: the worker

**Files:** modify `src/engine/worker.rs`, `tests/worker.rs`.

**Interfaces (produces):**
```rust
pub struct StepRun {
    pub file: PathBuf,                      // the action file, or "notify" for the built-in
    pub env: BTreeMap<String, String>,      // the bound values
    pub state: Map<String, Value>,          // on stdin
    pub memory: PathBuf,                    // $ITERO_MEMORY
    pub waiting_since: Option<String>,      // $ITERO_WAITING_SINCE
    pub timeout: u64,                       // 600
}
pub enum Outcome { Done(Map<String, Value>), NotYet(Map<String, Value>) }
pub fn run_step(r: &StepRun) -> Result<Outcome, Failure>;
```
The worker prints `{"done": {...}}` or `{"not_yet": {...}}` and exits 0;
its own exit 75 (`LOST_EXIT`) still means a lost job. The action's 75 is
read by the worker, never passed through.

- [ ] Tests first, with small `#!/bin/sh` actions in a temp dir:
  - the action sees `$host` and the state on stdin.
  - prints `{"metin": "çay"}`: `Done`; prints nothing: `Done({})`; prints
    `[1]` or text: an error saying it is not a JSON object.
  - exits 75 after printing `{"ref": "ödeme-1"}`: `NotYet` with it.
  - exits 3 with stderr "bağlantı yok": an error ending with that text.
  - writes then reads `$ITERO_MEMORY` across two runs; sees
    `$ITERO_WAITING_SINCE` when given.
  - `#notify` with `text` writes to `ITERO_NOTIFY_LOG`.
- [ ] Run, fail, implement, pass. The old `run()` stays until task 4.
- [ ] Commit: "The worker runs an action file".

### Task 4: the switch

The daemon and the journal use tasks 1 to 3; the old format goes.

**Files:**
- Modify: `src/engine/daemon.rs`, `src/engine/journal.rs`,
  `src/engine/mod.rs`, `src/engine/events.rs`, `src/engine/schedule.rs`,
  `src/engine/secrets.rs`, `src/cli.rs`
- Delete: `src/workflow/`, `src/engine/manager.rs`, `src/engine/eval.rs`,
  `src/engine/builtins.rs`; `cel-interpreter` and `reqwest` from
  `Cargo.toml`
- Tests: rewrite `tests/daemon.rs`, `tests/journal.rs`, `tests/events.rs`,
  `tests/schedule.rs`; delete `tests/manager.rs`, `tests/pubsub.rs`,
  `tests/limits.rs`, `tests/breaker.rs`, `tests/containers.rs`,
  `tests/container_clock.rs`

What the daemon does:
- **Starting runs:** a workspace message starts a run of each workflow
  whose `on` lists its type and whose conditions hold (`flow::holds` over
  the message's data and the defined `$$` names), in parallel. A schedule
  does the same at its times.
- **Pinning:** at its start, a run pins its workflow and every workflow it
  uses as a step, under one UUIDv7.
- **Waiting runs:** each workspace message is copied as `message` to every
  waiting run; the manager decides whether it matches. When a run starts
  waiting, the workspace messages since its start are copied to it too.
- **Binding** (building a `StepRun`): a value is the first found in the
  step's data, the args (its item after a split), then the state. A data
  or args value that is exactly `"$name"` takes the state's value, and
  `"$$NAME"` the secret's: `secrets::get(name)` (the store, then the
  daemon's environment). Every name that is a valid environment variable
  becomes one; strings as they are, other values as JSON text.
  `$ITERO_MEMORY` is `.itero/memory/<workflow>`. `#notify` runs
  `actions/notify` when there is one, else the built-in.
- **Defaults:** a step may take 600 s; a lost or timed-out job runs again
  up to 3 times, then it is an error; no pausing.
- **Splits:** each item starts a new run at the same step, with a copy of
  the state and the item.
- **Keeping:** each pass deletes runs ended more than a day ago, and
  messages handled more than a day ago whose runs have all ended.
- **Journal:** `runs.runtime` holds the new `Runtime`. Migration 5 deletes
  the runs of the old format, with their timers, jobs and queued starts,
  and marks their unhandled events handled.

- [ ] Tests first (`tests/daemon.rs`, through `run_once` and shell
  actions):
  - mail: `mail.received` `{"text": "Ayşe: Toplantı"}` starts
    `mail-alert`; `#notify` shows it.
  - order, end to end: `#check-payment` and `#check-stock` side by side;
    `#check-payment` exits 75 with `{"payment_ref": "ödeme-1"}`;
    `payment.received` `{"payment_ref": "ödeme-2"}` changes nothing;
    `{"payment_ref": "ödeme-1", "paid": true}` lets it finish;
    `order-process` runs `#ship`; `success` runs; the run is done.
  - the same with no stock: `#remind-stock` prints `{"success": false}`;
    `failure` runs; the run is failed.
  - an early message: `payment.received` arrives before
    `#check-payment` exits 75, and still reaches the run.
  - one message matching two waiting runs moves both; five workflows on
    one message start five runs.
  - `on: ["order.received", "$$API"]` starts nothing until `API` is
    defined.
  - binding: step data over args over state; `"$$TOKEN"` filled from the
    environment, and absent from the journal.
  - an action exiting 3: `error` runs, `itero errors` lists the step and
    why.
  - waiting past `"timeout": "1m"`: error.
  - a list of two args items: two runs from that step.
  - kill tests: the daemon killed at each `die_at` point loses and repeats
    no step.
  - a journal from schema 4 opens; its old runs gone, its events kept.
  - a run ended two days ago is deleted, with its handled messages.
- [ ] Run, fail, switch, delete, pass.
- [ ] Commit: "The daemon runs the simple workflows; the old format goes".

### Task 5: args and secrets

**Files:** `src/cli.rs`, `src/engine/secrets.rs`, `Cargo.toml`
(`rpassword`, Apache-2.0, to read a secret without echo), `tests/cli.rs`.

```
itero args <action> '<object or list>'     (or --file <path>)
itero secret <NAME>                        (asks; or --file <path>)
```
- `itero args` writes `args/<action>.json`, refusing what is not an
  object or a list of objects.
- `itero secret` keeps the value in the OS store under the service
  `itero`; the value is never an argument.

- [ ] Tests first: args written and refused; `itero secret` with
  `ITERO_NO_SECRET_STORE` says the store is off; `--file` reads the value
  and trims one trailing newline.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "itero args and itero secret".

### Task 6: sources and `itero watch`

**Files:** `src/sources/mod.rs`, `src/sources/supervise.rs`, `src/cli.rs`,
`tests/sources.rs`, `tests/watch.rs`.

- A source is an executable in `sources/`; each line it prints is the data
  of one message of its file name's type. A line that is not a JSON object
  is logged and skipped. No settings line, no cursors, no container
  sources.
- `itero watch <path>` prints `{"path": "<path>"}` each time it changes
  (the existing kqueue/inotify backend and debouncer).
- The supervisor's restart policy stays.

- [ ] Tests first: a source printing two lines starts two runs; a bad line
  is skipped; a source that keeps exiting is given up on; `itero watch`
  prints a line when the file changes.
- [ ] Run, fail, implement, pass.
- [ ] Commit: "Sources are executables; itero watch".

### Task 7: commands and HTTP

**Files:** `src/cli.rs`, `src/engine/http.rs`, `tests/http.rs`,
`tests/cli.rs`.

- `itero emit <type> '<data>'`: the data as the second argument; a type
  of one word is refused.
- `itero runs`: id, workflow, where it is (or how it ended), started,
  ended.
- `itero errors`: when, workflow, run, the step, why.
- `itero check`: every workflow, action and source; each `#action` and
  workflow a step names exists; what is wrong, with the file.
- `itero memory` and `itero resume` go; `GET /workflows` shows each `on`;
  `GET /workflows/{name}/memory` goes.

- [ ] Tests first for each line; run, fail, implement, pass.
- [ ] Commit: "Commands and HTTP for the simple files".

### Task 8: the examples

- `examples/mail-alert/`: `sources/mail.received` (`itero watch` on the
  mailbox, one line per new mail, its own "last seen"),
  `workflows/mail-alert.json`.
- `examples/mail-servers/`: `workflows/imap-alert.json`,
  `workflows/pop3-alert.json`, `actions/imap-check`, `actions/pop3-check`
  (shell around `curl`, the password to `curl` on stdin with `-K -`,
  `$ITERO_MEMORY` for the last seen). The Python files and containers go.
- `examples/order/` and `examples/page-digest/` as in the spec.
- Tests: `tests/mail.rs` (mail-alert), `tests/mail_servers.rs` (curl
  against GreenMail; Docker only for GreenMail), `tests/examples.rs`
  (`itero check` passes on every example; the order example runs end to
  end); `scripts/test-docker.sh` runs `mail_servers` only.

- [ ] Tests first; run, fail, write the examples, pass (`make test`,
  `make test-docker`).
- [ ] Commit: "The examples in the simple form; mail over curl".

### Task 9: the docs

**Files:** `docs/src/content/docs/**`, `README.md`, `docs/design.md` (a
note at the top: the formats are in `design-simple.md`).

- Guides: getting started, workflows, actions, messages and sources,
  args and secrets, service. Reference: workflow file, action, commands,
  HTTP, glossary. Pages for removed things go.
- `cd docs && yarn build` builds; every link resolves.

- [ ] Rewrite, build, read every page once as a newcomer would.
- [ ] Commit: "Docs for the simple files".

### Task 10: wrap up

- [ ] `make fmt-check lint-check test test-docker`, `yarn build`.
- [ ] Push `simple` and open the PR, when the owner says so.
- [ ] With the owner's yes: reinstall itero at `~/itero`, move its mail
  check to `sources/mail.received` and `workflows/mail-alert.json`, and
  restart its service.
