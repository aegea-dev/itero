# itero, iteration 1: the mail check. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `itero daemon` runs the `mail-check` workflow every 5 minutes: it reads a mailbox with `mail`, shows a desktop notification for mail it has not seen, and remembers what it has seen.

**Architecture:** Start from primum's engine (daemon pass, SQLite journal, pure manager with CEL, worker), copied from primum commit f20debf, and cut it free of primum's ledger. The unit becomes a **run** of a workflow instead of a ticket; schedules start runs; a per-workflow memory replaces ticket changes; `notify@1` and `sh@1` join the built-ins.

**Tech Stack:** Rust 2024 (toolchain 1.96.0), clap, serde/serde_json, toml, rusqlite (bundled), cel-interpreter, chrono, uuid, wait-timeout, libc, a cron crate (task 4), tempfile for tests.

**Spec:** `docs/design.md` (this plan builds section 9; sections 1 to 6 say what the parts are).

## Global Constraints

- Crates: MIT or Apache-2.0 only (check with `cargo info <crate>`); requirements written `~x.y.z`.
- Edition 2024, toolchain pinned in `rust-toolchain.toml` (`channel = "1.96.0"`, components rustfmt and clippy), as primum does.
- Test first; run the tests and read the output before saying something works.
- Fixtures use non-ASCII text (ı, ş, ğ, ü, é) in neutral words, not personal names.
- Public text (comments, README, docs, commits): no AI attribution (no Co-Authored-By trailer, no "Generated with" line), no em dash.
- Stage files by name, never `git add -A`. Commit locally; do not create a GitHub repo or push (the owner will create it under their org).
- The manager stays pure (no IO, clock or randomness); the daemon decides nothing; one transaction per answer; every handler copes with the same event twice; secrets never reach events, the journal, logs or command lines.
- Values reach a command only as environment variables, never pasted into its text.
- Source of copied code: `git -C ~/projects/primum show f20debf:<path>` or `git -C ~/projects/primum archive f20debf <paths> | tar -x -C <dir>`. Never change the primum repo.

## File structure (at the end of this iteration)

```
Cargo.toml, rust-toolchain.toml, Makefile, README.md, .gitignore
docs/design.md, docs/plan-1-mail-check.md
src/main.rs            the binary: calls cli::run
src/lib.rs             pub mod cli, workspace, workflow, engine
src/cli.rs             clap: daemon, manager, worker, check, runs, memory
src/workspace.rs       itero.toml: port, workflows and actions folders; finding the workspace
src/workflow/          the workflow format (from primum's src/lifecycle/):
  mod.rs               load, version (pin), list
  format.rs            File, Transition, Action (+ id, when), Trigger
  validate.rs          rules (+ no template in sh@1 `run`, trigger checks)
  expr.rs              durations (from primum)
  actions.rs           project actions: actions/<name>/action.json (from actionables.rs, definitions.rs)
src/engine/
  mod.rs               Input, Answer, Runtime, Job, die_at (from primum)
  manager.rs           the pure decision (from primum, run instead of ticket)
  eval.rs              CEL and templates (+ run, memory, steps)
  builtins.rs          pure built-ins: memory.set@1, json.pick@1, event.emit@1
  journal.rs           SQLite: events, runs, workflows, timers, schedules, memory, jobs
  schedule.rs          cron: is a trigger due
  daemon.rs            the pass and the loop (no ledger)
  worker.rs            http.request@1, command@1, sh@1, notify@1, programs
  secrets.rs           from primum, service name "itero"
tests/                 manager.rs, schedule.rs, memory.rs, worker.rs, daemon.rs, mail.rs, common/mod.rs
examples/mail-check/   itero.toml, workflows/mail-check.json, actions/mail/{action.json,check.sh}
```

Not in this iteration: the HTTP API (port in `itero.toml` is read but nothing listens yet; serving comes in iteration 2), watchdog, service, inbox, NATS, primum.

---

### Task 1: The crate, with primum's engine cut free of the ledger

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `Makefile`, `.gitignore`, `src/main.rs`, `src/lib.rs`, `src/cli.rs`
- Create (copied, then edited): `src/engine/{mod,manager,eval,builtins,journal,daemon,worker,secrets}.rs`, `src/workflow/{mod,format,validate,expr,actions}.rs`
- Test: `tests/manager.rs`, `tests/common/mod.rs`

**Interfaces:**
- Produces: `itero::workflow::parse(text: &str) -> Result<File, WorkflowError>` and `itero::workflow::load_file(path: &Path) -> Result<File, WorkflowError>` (primum's lifecycle parsing and validation, renamed; the error names the file and where); `itero::engine::manager::decide(&Input) -> Result<Answer, String>`; `Input { now: String, workflow: workflow::File, run: Map<String, Value>, args: Map<String, Value>, memory: Map<String, Value>, runtime: Runtime, event: Event }`; `Answer { runtime: Runtime, memory: Map<String, Value>, events: Vec<OutEvent>, timers: Option<Vec<Timer>>, log: Vec<String> }`; `Runtime { state, chain, failed }` as in primum.

- [ ] **Step 1: Make the folder a repo and copy the code**

```bash
cd ~/projects/itero
git init
mkdir -p src/engine src/workflow tests/common
git -C ~/projects/primum show f20debf:rust-toolchain.toml > rust-toolchain.toml
for f in mod manager eval builtins journal daemon worker secrets; do git -C ~/projects/primum show f20debf:src/engine/$f.rs > src/engine/$f.rs; done
for f in format validate expr version; do git -C ~/projects/primum show f20debf:src/lifecycle/$f.rs > src/workflow/$f.rs; done
git -C ~/projects/primum show f20debf:src/lifecycle/mod.rs > src/workflow/mod.rs
git -C ~/projects/primum show f20debf:src/lifecycle/definitions.rs > src/workflow/actions.rs
git -C ~/projects/primum show f20debf:tests/manager.rs > tests/manager.rs
```

Merge `src/lifecycle/version.rs` into `src/workflow/mod.rs` (one file: `pin`, `split_pin`, version hashing). `src/lifecycle/actionables.rs` (primum's list of built-ins for validation) folds into `src/workflow/actions.rs`.

- [ ] **Step 2: Cargo.toml**

```toml
[package]
name = "itero"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "Runs workflows again and again: schedules, events, steps, workers"

[dependencies]
clap = { version = "~4.6.6", features = ["derive"] }
serde = { version = "~1.0.229", features = ["derive"] }
serde_json = "~1.0.151"
toml = "~1.1.6"
chrono = "~0.4.45"
anyhow = "~1.0.104"
cel-interpreter = { version = "~0.9.1", features = ["json"] }
sha2 = "~0.11.1"
humantime = "~2.4.0"
rusqlite = { version = "~0.40.2", features = ["bundled"] }
uuid = { version = "~1.28.0", features = ["v7"] }
# native-tls, not the default rustls: its aws-lc-rs is not MIT or Apache-2.0
reqwest = { version = "~0.13.5", default-features = false, features = ["blocking", "json", "native-tls"] }
wait-timeout = "~0.2.1"
keyring = "~4.2.0"
fs2 = "~0.4.3"

[target.'cfg(unix)'.dependencies]
libc = "~0.2.190"

[dev-dependencies]
tempfile = "~3.27.0"
```

`Makefile`: copy primum's `fmt`, `fmt-check`, `lint-check`, `test` targets (`git -C ~/projects/primum show f20debf:Makefile`). `.gitignore`: `/target`, `.itero/`.

- [ ] **Step 3: Cut the ledger out (edits, in this order)**

1. `src/workflow/*`: rename the module `lifecycle` to `workflow` everywhere (`use crate::lifecycle::` becomes `use crate::workflow::`). Remove everything taking a `store::Project` or reading items: `attach`, `detach`, the item `lifecycle:`/`args:` front matter. Keep `File`, `load_file`, validation, `pin`. A workflow's folder comes from the workspace (task 2), so functions take a `&Path` to the folder instead of a project.
2. `src/engine/mod.rs`: in `Input`, replace `lifecycle` with `workflow: crate::workflow::File`, `ticket` with `run: Map<String, Value>` (the run's `id`, `workflow`, `started`), `project` with `memory: Map<String, Value>`. In `Answer`, remove `status`, rename `changes` to `memory`. Keep `die_at`, `Job` (rename env var `PRIMUM_HEARTBEAT` to `ITERO_HEARTBEAT`, `PRIMUM_TEST_DIE_AT` to `ITERO_TEST_DIE_AT`, `PRIMUM_TEST_DIE_ONCE` to `ITERO_TEST_DIE_ONCE`).
3. `src/engine/manager.rs`: follow the compiler. `status` goes; the state is `runtime.state`. `changes` becomes `memory` (no note merging: `memory.set@1` replaces keys). `lifecycle.attached` becomes `run.started`. Person moves (`ticket.changed` setting a status) go.
4. `src/engine/eval.rs`: `Vars { run, memory, args, input, prev, steps, output }` (`steps` is added in task 3; for now `run` and `memory` replace `ticket` and `project`).
5. `src/engine/builtins.rs`: `ticket.update@1` becomes `memory.set@1` (same body: its `with` is the change).
6. `src/engine/journal.rs`: tables `events`, `runs (id TEXT PRIMARY KEY, workflow TEXT NOT NULL, pin TEXT NOT NULL, runtime TEXT NOT NULL, args TEXT NOT NULL, started TEXT NOT NULL, ended TEXT)`, `workflows (pin, definition)` (was `lifecycles`), `timers`, `jobs`, `meta`. Drop `ledger`, `seen`, `tickets`. Start the schema at version 1 with no migrations (new product, no old journals). Subjects: `itero.run.<id>` and `itero.worker`; `run_of(subject)` replaces `ticket_of`. The journal lives at `<workspace>/.itero/journal.sqlite`.
7. `src/engine/daemon.rs`: delete `feed`, `inbox`, `free_name`, `write_ledger`, `write_one`, `revisions`, `snapshot`, `sha`, the runner/lease code, and the revision retry loop in `deliver` (nothing outside changes a run while its manager runs). `deliver` builds `Input` from the run row, the pinned workflow and the workflow's memory (task 5 adds memory; pass an empty map now). `run_once(ws: &Workspace, now, exe: &Path)` replaces `run_once(p: &Project, ...)`. `MAX_EVENTS`, `MAX_TRIES` (manager retries), `MAX_RUNS`, `MAX_WORKERS`, `batch`, `spawn_worker`, `stop`, `quiet`, `save_result` stay.
8. `src/engine/worker.rs`: unchanged apart from the env var rename.
9. `src/engine/secrets.rs`: service name `"itero"`, env var `ITERO_NO_SECRET_STORE`.
10. `src/cli.rs`: only `manager` and `worker` for now (copy their arms from primum's cli.rs, with `die_at` calls); `src/main.rs` restores SIGPIPE and calls `itero::cli::run`.

- [ ] **Step 4: Adapt `tests/manager.rs`**

Keep the tests that exercise transitions, chains, `expect`, timeouts, failures and determinism; drop those about tickets, person moves and notes. Rewrite fixtures to a workflow with states in Turkish words, for example `"states": ["yeni", "hazır", "bitti"]`. Each kept test builds an `Input` with `run: {"id": "r1", "workflow": "akış"}` and `memory: {}`.

- [ ] **Step 5: Build and run**

Run: `cargo build && cargo test --test manager`
Expected: builds; the kept manager tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml Makefile .gitignore src tests docs
git commit -m "itero: primum's engine, cut free of the ledger"
```

---

### Task 2: The workspace and loading workflows and actions

**Files:**
- Create: `src/workspace.rs`, `tests/workspace.rs`
- Modify: `src/workflow/mod.rs`, `src/workflow/actions.rs`, `src/engine/daemon.rs` (`job()` finds actions through the workspace)

**Interfaces:**
- Produces: `workspace::Workspace { root: PathBuf, port: u16, workflows: Vec<PathBuf>, actions: Vec<PathBuf> }`; `workspace::find(cwd: &Path) -> Result<Workspace>` (walks up to the folder with `itero.toml`); `Workspace::workflows(&self) -> Result<Vec<(PathBuf, workflow::File)>>`; `Workspace::action(&self, name: &str) -> Result<actions::Definition>`; `Workspace::journal(&self) -> PathBuf` (`<root>/.itero/journal.sqlite`); `engine::journal::Journal::open(ws: &Workspace) -> Result<Journal>` (opens or makes that file; replaces primum's `Journal::open(&Project)`).

- [ ] **Step 1: Write the failing test**

```rust
// tests/workspace.rs
use itero::workspace;

#[test]
fn a_workspace_lists_its_folders_and_finds_workflows_in_each() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("itero.toml"),
        "port = 8790\nworkflows = [\"workflows\", \"başka/workflows\"]\nactions = [\"actions\"]\n",
    )
    .unwrap();
    for dir in ["workflows", "başka/workflows"] {
        std::fs::create_dir_all(d.path().join(dir)).unwrap();
    }
    let flow = |name: &str| format!(
        r#"{{ "name": "{name}", "states": ["başla", "bitti"], "start": "başla", "end": ["bitti"],
             "transitions": [ {{ "from": "başla", "to": "bitti" }} ] }}"#
    );
    std::fs::write(d.path().join("workflows/sayaç.json"), flow("sayaç")).unwrap();
    std::fs::write(d.path().join("başka/workflows/temizlik.json"), flow("temizlik")).unwrap();
    let sub = d.path().join("workflows");
    let ws = workspace::find(&sub).unwrap();
    assert_eq!(ws.port, 8790);
    let mut names: Vec<String> = ws.workflows().unwrap().into_iter().map(|(_, f)| f.name).collect();
    names.sort();
    assert_eq!(names, ["sayaç", "temizlik"]);
}

#[test]
fn no_itero_toml_is_an_error_saying_how_to_make_one() {
    let d = tempfile::tempdir().unwrap();
    let e = workspace::find(d.path()).unwrap_err().to_string();
    assert!(e.contains("itero.toml"), "{e}");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test workspace`
Expected: FAIL to compile: no `itero::workspace`.

- [ ] **Step 3: Implement `src/workspace.rs`**

```rust
//! The workspace: the folder with itero.toml, and the folders it lists for
//! workflows and actions. One daemon runs one workspace.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_workflows")]
    workflows: Vec<String>,
    #[serde(default = "default_actions")]
    actions: Vec<String>,
}

fn default_port() -> u16 { 8787 }
fn default_workflows() -> Vec<String> { vec!["workflows".into()] }
fn default_actions() -> Vec<String> { vec!["actions".into()] }

pub struct Workspace {
    pub root: PathBuf,
    pub port: u16,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
}

/// A listed folder: `~/` is the home folder, a relative path is under the workspace.
fn folder(root: &Path, s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        None => root.join(s),
    }
}

/// The workspace `cwd` is in: the nearest folder upwards with itero.toml.
pub fn find(cwd: &Path) -> Result<Workspace> {
    let Some(root) = cwd.ancestors().find(|d| d.join("itero.toml").is_file()) else {
        bail!("no itero.toml here or above: write one (port, workflows, actions) to make this folder a workspace");
    };
    let path = root.join("itero.toml");
    let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let s: Settings = toml::from_str(&text).with_context(|| format!("{}", path.display()))?;
    Ok(Workspace {
        root: root.to_path_buf(),
        port: s.port,
        workflows: s.workflows.iter().map(|w| folder(root, w)).collect(),
        actions: s.actions.iter().map(|a| folder(root, a)).collect(),
    })
}
```

`Workspace::workflows` reads every `*.json` in each listed folder with `workflow::load_file` (a missing folder is an error naming it); two workflows with one name is an error naming both files. `Workspace::action(name)` looks for `<folder>/<name>/action.json` in each actions folder, first found wins. `Workspace::journal` returns `root.join(".itero/journal.sqlite")`. In `daemon::job()`, replace primum's `definitions::load_all(p)` lookup with `ws.action(name)`; the action's folder is that `action.json`'s folder.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test workspace`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/workspace.rs src/lib.rs src/workflow src/engine/daemon.rs tests/workspace.rs
git commit -m "Workspace: itero.toml and its workflows and actions folders"
```

---

### Task 3: Step ids, `steps`, and `when` on an action

**Files:**
- Modify: `src/workflow/format.rs` (Action gains `id: Option<String>`, `when: Option<String>`), `src/workflow/validate.rs`, `src/engine/mod.rs` (`Chain` gains `steps: Map<String, Value>`), `src/engine/eval.rs` (`Vars.steps`), `src/engine/manager.rs`
- Test: `tests/manager.rs`

**Interfaces:**
- Consumes: `Input`, `Answer`, `Runtime` from task 1.
- Produces: in templates and conditions, `steps.<id>` is the output of the action with that `id`, for the rest of the chain; an action whose `when` is false is skipped and `prev` passes through. `Chain.steps` persists across jobs (it is in `Runtime`, saved in the journal).

- [ ] **Step 1: Write the failing tests** (in `tests/manager.rs`, using the file's existing helpers for building an `Input` and delivering `action.finished`)

```rust
#[test]
fn a_named_steps_output_is_seen_later_and_when_skips_an_action() {
    // start -> bitti: a worker job (id "kontrol"), then two pure actions:
    // the first runs only when kontrol found something, the second always
    let flow = r#"{ "name": "akış", "states": ["başla", "bitti"], "start": "başla", "end": ["bitti"],
      "transitions": [ { "from": "başla", "to": "bitti", "do": [
        { "id": "kontrol", "action": "command@1", "with": { "run": ["true"] } },
        { "action": "memory.set@1", "when": "steps.kontrol.json.yeni > 0",
          "with": { "uyarı": "${steps.kontrol.json.özet}" } },
        { "action": "memory.set@1", "with": { "son": "${steps.kontrol.json.son}" } }
      ] } ] }"#;
    // run.started: the job for kontrol is requested
    let a = decide(&input(flow, "run.started", json!({}))).unwrap();
    assert_eq!(a.events[0].kind, "action.requested");
    // its result: yeni = 0, so the first memory.set is skipped
    let out = json!({"exit": 0, "json": {"yeni": 0, "özet": "", "son": "m-7"}});
    let a = decide(&finished(&a, out)).unwrap();
    assert_eq!(a.runtime.state, "bitti");
    assert_eq!(a.memory, json!({"son": "m-7"}).as_object().unwrap().clone());
    // with yeni = 2, both run
    let a0 = decide(&input(flow, "run.started", json!({}))).unwrap();
    let out = json!({"exit": 0, "json": {"yeni": 2, "özet": "2 yeni ileti", "son": "m-9"}});
    let a = decide(&finished(&a0, out)).unwrap();
    assert_eq!(a.memory["uyarı"], "2 yeni ileti");
    assert_eq!(a.memory["son"], "m-9");
}

#[test]
fn an_id_used_twice_in_a_workflow_is_refused() {
    let flow = r#"{ "name": "akış", "states": ["başla", "bitti"], "start": "başla", "end": ["bitti"],
      "transitions": [ { "from": "başla", "to": "bitti", "do": [
        { "id": "a", "action": "json.pick@1", "with": { "path": "x" } },
        { "id": "a", "action": "json.pick@1", "with": { "path": "y" } } ] } ] }"#;
    let e = itero::workflow::parse(flow).unwrap_err().to_string();
    assert!(e.contains("id \"a\" is used twice"), "{e}");
}
```

`input(flow, kind, data)` and `finished(answer, output)` are helpers: `input` parses the workflow, sets `runtime.state` to its `start`, `run` to `{"id": "r1"}`, empty `args` and `memory`, `now` to `"2026-10-15T14:00:00Z"`; `finished` takes the previous answer's runtime and its `action.requested` event and returns an `Input` for `action.finished` with `{"output": output, "run": <run from the request>, "index": <index from the request>}`. Write them at the top of `tests/manager.rs` if the copied file has no equivalent.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test manager named_steps id_used_twice`
Expected: FAIL (unknown fields `id`/`when` on an action).

- [ ] **Step 3: Implement**

- `format.rs`: `Action { action, with, expect, description, id: Option<String>, when: Option<String> }` (both `#[serde(default, skip_serializing_if = "Option::is_none")]`).
- `validate.rs`: an `id` is lowercase letters, digits and `-` (reuse `is_word`); unique within the workflow (`id "a" is used twice`); a `when` must compile (`eval::check` as for `expect`).
- `mod.rs` (engine): `Chain` gets `#[serde(default, skip_serializing_if = "Map::is_empty")] pub steps: Map<String, Value>`.
- `eval.rs`: `Vars` gets `steps: &Map<String, Value>`; `value()` adds `c.add_variable_from_value("steps", object(v.steps))`. Every caller passes `&chain.steps` (or an empty map outside a chain).
- `manager.rs`: where the chain advances to its next action, first evaluate the action's `when` (with `prev` and `steps`); if false, skip it (index + 1, `prev` unchanged). When an action's output is known (a pure built-in's, or a worker's from `action.finished`), store it under `steps[id]` if it has an `id`.

- [ ] **Step 4: Run all manager tests**

Run: `cargo test --test manager`
Expected: PASS, the new tests and the kept ones.

- [ ] **Step 5: Commit**

```bash
git add src/workflow src/engine tests/manager.rs
git commit -m "Steps: an action's output by id, and when to skip an action"
```

---

### Task 4: Schedules start runs

**Files:**
- Create: `src/engine/schedule.rs`, `tests/schedule.rs`
- Modify: `src/workflow/format.rs` (`File.triggers: Vec<Trigger>`), `src/workflow/validate.rs`, `src/engine/journal.rs` (`schedules` table, `start_run`), `src/engine/daemon.rs` (fire due schedules at the start of a pass), `Cargo.toml` (a cron crate)

**Interfaces:**
- Produces: `Trigger { schedule: Option<String>, args: Map<String, Value> }` (`schedule` required in this iteration); `schedule::due(cron: &str, last: DateTime<Utc>, now: DateTime<Utc>) -> Result<bool, String>` (true when a slot of `cron`, in the machine's local time, falls in `(last, now]`); `Journal::start_run(workflow: &workflow::File, args: &Map<String, Value>, now: &str) -> Result<String>` (pins the workflow version, inserts the run, publishes `run.started` for `itero.run.<id>`, one transaction; returns the run id); `Journal::schedule_last(key: &str) -> Result<Option<String>>`, `Journal::set_schedule_last(key: &str, at: &str)`; the key is `<workflow>#<trigger index>`.

- [ ] **Step 1: Pick the cron crate**

Use `croner` (supports 5-field cron) if `cargo info croner` shows MIT or Apache-2.0; otherwise `cron` (check the same way). Add it to Cargo.toml as `~x.y.z`.

- [ ] **Step 2: Write the failing tests**

```rust
// tests/schedule.rs
use chrono::{DateTime, Utc};
use itero::engine::schedule::due;

fn at(s: &str) -> DateTime<Utc> { s.parse().unwrap() }

#[test]
fn every_five_minutes_is_due_once_per_slot() {
    std::env::set_var("TZ", "UTC");
    let c = "*/5 * * * *";
    assert!(due(c, at("2026-10-15T14:04:59Z"), at("2026-10-15T14:05:00Z")).unwrap());
    assert!(!due(c, at("2026-10-15T14:05:00Z"), at("2026-10-15T14:09:59Z")).unwrap());
    // a daemon asleep for an hour runs it once, not twelve times
    assert!(due(c, at("2026-10-15T13:00:00Z"), at("2026-10-15T14:00:30Z")).unwrap());
}

#[test]
fn a_bad_schedule_is_an_error_naming_it() {
    let e = due("her beş dakika", at("2026-10-15T14:00:00Z"), at("2026-10-15T14:05:00Z")).unwrap_err();
    assert!(e.contains("her beş dakika"), "{e}");
}
```

(`std::env::set_var` is unsafe in edition 2024: wrap it in `unsafe { }` with a `// SAFETY:` line, or compute with `chrono::Local` only where the test fixes `TZ` before any thread starts. Simpler: make `due` take the time zone as a parameter, `due_in(cron, last, now, tz: &impl chrono::TimeZone)`, test it with `Utc`, and have `due` call it with `Local`.)

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test --test schedule`
Expected: FAIL to compile: no `schedule` module.

- [ ] **Step 4: Implement `due`** with the chosen crate: find the first slot after `last` (in `tz`); due when it is at or before `now`.

- [ ] **Step 5: Wire it into the daemon, test first**

Add to `tests/daemon.rs` (create it, copying `tests/common/mod.rs` helpers as needed):

```rust
#[test]
fn a_scheduled_workflow_starts_a_run_once_per_slot() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::write(d.path().join("workflows/sayaç.json"), r#"{ "name": "sayaç",
      "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["başla", "bitti"], "start": "başla", "end": ["bitti"],
      "transitions": [ { "from": "başla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "son": "${run.id}" } } ] } ] }"#).unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let t = |s: &str| s.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    // first pass: the schedule is seen, nothing is due yet
    itero::engine::daemon::run_once(&ws, t("2026-10-15T14:01:00Z"), exe).unwrap();
    // the 14:05 slot: one run, which ends
    itero::engine::daemon::run_once(&ws, t("2026-10-15T14:05:10Z"), exe).unwrap();
    itero::engine::daemon::run_once(&ws, t("2026-10-15T14:06:00Z"), exe).unwrap();
    let j = itero::engine::journal::Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].state, "bitti");
}
```

Daemon: at the start of `run_once`, for each workflow and each of its `triggers` with a `schedule`: `last = journal.schedule_last(key)`; with none, set it to `now` and skip (a new schedule waits for its first slot); if `due(schedule, last, now)`, `journal.start_run(...)` and `set_schedule_last(key, now)`. `Journal::runs() -> Vec<RunRow { id, workflow, state, started, ended }>`. A run whose state is in `end` gets `ended` set when its answer is saved.

- [ ] **Step 6: Run the tests**

Run: `cargo test --test schedule --test daemon`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src tests/schedule.rs tests/daemon.rs
git commit -m "Schedules: a workflow's cron triggers start runs"
```

---

### Task 5: Memory

**Files:**
- Modify: `src/engine/journal.rs` (`memory` table; `Handled` saves memory changes), `src/engine/daemon.rs` (`deliver` reads memory into `Input`), `src/cli.rs` (`itero memory <workflow>` prints it as JSON)
- Test: `tests/daemon.rs`

**Interfaces:**
- Consumes: `Answer.memory` (task 1), `memory.set@1` (task 1).
- Produces: `Journal::memory(workflow: &str) -> Result<Map<String, Value>>`; `Handled.memory: Option<(&str, Map<String, Value>)>` saved in the answer's transaction (`INSERT ... ON CONFLICT (workflow, key) DO UPDATE`); table `memory (workflow TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workflow, key))`.

- [ ] **Step 1: Write the failing test** (in `tests/daemon.rs`)

```rust
#[test]
fn memory_set_by_one_run_is_read_by_the_next() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    // each run adds one to sayı, reading what the last run left
    std::fs::write(d.path().join("workflows/sayaç.json"), r#"{ "name": "sayaç",
      "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["başla", "bitti"], "start": "başla", "end": ["bitti"],
      "transitions": [ { "from": "başla", "to": "bitti",
        "do": [ { "action": "memory.set@1",
                  "with": { "sayı": "${has(memory.sayı) ? memory.sayı + 1 : 1}" } } ] } ] }"#).unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let t = |s: &str| s.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    for at in ["2026-10-15T14:01:00Z", "2026-10-15T14:05:10Z", "2026-10-15T14:10:10Z"] {
        itero::engine::daemon::run_once(&ws, t(at), exe).unwrap();
    }
    let j = itero::engine::journal::Journal::open(&ws).unwrap();
    assert_eq!(j.memory("sayaç").unwrap()["sayı"], 2);
}
```

(If a template that is exactly one `${...}` does not keep the expression's type, make it so: a template that is the whole string gives the expression's JSON value, not text. primum's `eval::fill` may already do this; check and keep it.)

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test daemon memory_set_by_one_run`
Expected: FAIL (memory is always empty).

- [ ] **Step 3: Implement** the table, `Journal::memory`, saving `Answer.memory` in `handle`'s transaction, and `deliver` passing `j.memory(&run.workflow)?` as `Input.memory`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test daemon`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src tests/daemon.rs
git commit -m "Memory: a workflow's own store, read by its runs, set by memory.set@1"
```

---

### Task 6: `sh@1` and `notify@1`

**Files:**
- Modify: `src/engine/worker.rs`, `src/engine/builtins.rs` (`default_expect`: `sh@1` expects `output.exit == 0`; `notify@1` none), `src/workflow/validate.rs` (no template in `sh@1`'s `run`; built-in names known), `src/engine/daemon.rs` (`job()` passes `sh@1` and `notify@1` straight to the worker like `command@1`)
- Test: `tests/worker.rs`

**Interfaces:**
- Produces: `sh@1` with `{"run": "<shell line>", "env": {"NAME": "value", ...}}`, run with `sh -c` in the workspace root; output `{"exit", "stdout", "stderr", "json"?, "lines"}` (`json` when stdout parses as JSON, `lines` always: stdout split into lines). `notify@1` with `{"title", "text"}`; runs `osascript -e 'on run argv' -e 'display notification (item 2 of argv) with title (item 1 of argv)' -e 'end run' <title> <text>` on macOS (title and text as arguments, never in the script text), `notify-send <title> <text>` on Linux; output `{"shown": true}`. `ITERO_NOTIFY_LOG=<file>`, when set, appends `<title>\t<text>\n` to that file instead of showing anything (for tests).

- [ ] **Step 1: Write the failing tests**

```rust
// tests/worker.rs (job() helper as in primum's tests/worker.rs, with heartbeat: None)
#[test]
fn sh_runs_a_pipeline_with_values_only_in_its_environment() {
    let d = tempfile::tempdir().unwrap();
    let with = json!({"run": "printf '%s\\n' \"$KONU\" | wc -l | tr -d ' '",
                      "env": {"KONU": "çay; touch kırık"}});
    let out = worker::run(&job("sh@1", with, d.path().to_path_buf())).unwrap();
    assert_eq!(out["lines"], json!(["1"]));
    assert_eq!(out["json"], json!(1));
    assert!(!d.path().join("kırık").exists(), "a value never runs as a command");
}

#[test]
fn notify_writes_to_its_log_when_one_is_set() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    // SAFETY: this test binary sets the variable before running any job
    unsafe { std::env::set_var("ITERO_NOTIFY_LOG", &log) };
    let out = worker::run(&job("notify@1", json!({"title": "Posta", "text": "2 yeni ileti"}), d.path().to_path_buf())).unwrap();
    assert_eq!(out["shown"], true);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "Posta\t2 yeni ileti\n");
}
```

and in `tests/workflow.rs` (or the validation test file you kept):

```rust
#[test]
fn a_template_in_sh_run_is_refused() {
    let flow = r#"{ "name": "akış", "states": ["a", "b"], "start": "a", "end": ["b"],
      "transitions": [ { "from": "a", "to": "b", "do": [
        { "action": "sh@1", "with": { "run": "echo ${args.ad}" } } ] } ] }"#;
    let e = itero::workflow::parse(flow).unwrap_err().to_string();
    assert!(e.contains("put values in env"), "{e}");
}
```

(Run the notify test in its own test binary, or have `notify` read the log path from `with` in tests, if setting an environment variable races with other tests in the same binary.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test worker sh_runs notify_writes`
Expected: FAIL (`sh@1 does not run in a worker`).

- [ ] **Step 3: Implement** `sh` and `notify` in `worker.rs` next to `command`, the `lines`/`json` output, the validation message `sh@1: a template in run; put values in env and use $NAME`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test worker`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src tests
git commit -m "sh@1 for pipelines, notify@1 for desktop notifications"
```

---

### Task 7: The mail check, end to end

**Files:**
- Create: `examples/mail-check/itero.toml`, `examples/mail-check/workflows/mail-check.json`, `examples/mail-check/actions/mail/action.json`, `examples/mail-check/actions/mail/check.sh`, `tests/mail.rs`
- Modify: `src/cli.rs` (`itero daemon [--once] [--now T] [--interval S]`, `itero check`, `itero runs`), `src/engine/daemon.rs` (`run_loop` from primum, without the HTTP server for now)

**Interfaces:**
- Consumes: everything above.
- Produces: the example, and `itero daemon` running it.

- [ ] **Step 1: The action**

`examples/mail-check/actions/mail/action.json`:

```json
{
  "name": "mail",
  "version": 1,
  "type": "executable",
  "description": "Reads a mailbox with the mail command: what is new since the last seen message.",
  "limits": { "timeout": "30s" },
  "targets": { "check": { "executable": "./check.sh" } }
}
```

`examples/mail-check/actions/mail/check.sh` (chmod 755):

```sh
#!/bin/sh
# Prints {"new": n, "seen": "<id of the last message>", "summary": "..."}.
# A message's id is its "From " line (sender and date). The mailbox and the
# last seen id come as with.mailbox and with.seen on stdin.
job=$(cat)
field() { printf '%s' "$job" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }
mailbox=$(field mailbox)
[ -n "$mailbox" ] || mailbox="/var/mail/$(id -un)"
seen=$(field seen)
if [ ! -s "$mailbox" ]; then
  printf '{"new": 0, "seen": "%s", "summary": ""}\n' "$seen"
  exit 0
fi
ids=$(grep '^From ' "$mailbox")
last=$(printf '%s\n' "$ids" | tail -n 1)
if [ -z "$seen" ]; then
  new=$(printf '%s\n' "$ids" | grep -c .)
else
  new=$(printf '%s\n' "$ids" | awk -v s="$seen" 'f { n++ } $0 == s { f = 1 } END { print n + 0 }')
fi
subject=$(grep '^Subject: ' "$mailbox" | tail -n 1 | sed 's/^Subject: //')
esc() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
printf '{"new": %s, "seen": "%s", "summary": "%s new: %s"}\n' "$new" "$(esc "$last")" "$new" "$(esc "$subject")"
```

(It uses `grep`, `awk` and `sed` on the mbox file; `mail -H -f` can replace the subject lookup if it reads better. Keep it POSIX sh.)

- [ ] **Step 2: The workflow and workspace**

`examples/mail-check/itero.toml`: `port = 8787` (defaults for folders).

`examples/mail-check/workflows/mail-check.json`: as in `docs/design.md` section 9, with `"triggers": [ { "schedule": "*/5 * * * *", "args": { "mailbox": "" } } ]`.

- [ ] **Step 3: Write the failing end-to-end test**

```rust
// tests/mail.rs
#![cfg(unix)]
use std::path::Path;

/// The example workspace, copied into `d`, with its mailbox at `mbox`.
fn example(d: &Path, mbox: &Path) -> std::path::PathBuf {
    let ws = d.join("ws");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mail-check");
    let status = std::process::Command::new("cp").arg("-R").arg(&src).arg(&ws).status().unwrap();
    assert!(status.success());
    let flow = ws.join("workflows/mail-check.json");
    let text = std::fs::read_to_string(&flow).unwrap()
        .replace("\"mailbox\": \"\"", &format!("\"mailbox\": \"{}\"", mbox.display()));
    std::fs::write(&flow, text).unwrap();
    ws
}

fn mail(mbox: &Path, from: &str, date: &str, subject: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(mbox).unwrap();
    write!(f, "From {from} {date}\nFrom: {from}\nSubject: {subject}\n\nmerhaba\n\n").unwrap();
}

fn pass(ws: &Path, now: &str, log: &Path) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["daemon", "--once", "--now", now])
        .current_dir(ws)
        .env("ITERO_NOTIFY_LOG", log)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
}

#[test]
fn new_mail_gives_one_notification_and_old_mail_none() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("posta kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
    mail(&mbox, "ayse@örnek.test", "Thu Oct 15 13:50:00 2026", "Toplantı notları");
    pass(&ws, "2026-10-15T14:01:00Z", &log); // the schedule is seen
    pass(&ws, "2026-10-15T14:05:10Z", &log); // first run: one mail, not seen before
    let shown = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(shown.lines().count(), 1, "{shown}");
    assert!(shown.contains("Toplantı notları"), "{shown}");
    pass(&ws, "2026-10-15T14:10:10Z", &log); // nothing new
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
    mail(&mbox, "can@örnek.test", "Thu Oct 15 14:12:00 2026", "Çay saati");
    pass(&ws, "2026-10-15T14:15:10Z", &log); // one new
    let shown = std::fs::read_to_string(&log).unwrap();
    assert_eq!(shown.lines().count(), 2, "{shown}");
    assert!(shown.lines().last().unwrap().contains("1 new: Çay saati"), "{shown}");
}
```

- [ ] **Step 4: Run it to see it fail**

Run: `cargo test --test mail`
Expected: FAIL (no `daemon` command, or no notification).

- [ ] **Step 5: Implement** the `daemon` command (`--once`, `--now`, `--interval`, default 1 s between passes; the loop from primum's `run_loop` without the server), `itero check` (validates every workflow and action, prints `<name>  <version>` each), `itero runs` (the journal's runs, newest first: id, workflow, state, started, ended).

- [ ] **Step 6: Run everything**

Run: `cargo test --no-fail-fast && make fmt-check && make lint-check`
Expected: all PASS, no warnings.

- [ ] **Step 7: Try it for real**

```bash
cd examples/mail-check && cargo run -q -- daemon
# in another terminal:
echo "merhaba" | mail -s "deneme" "$USER"
```

Expected: within 5 minutes one macOS notification "Mail: 1 new: deneme"; none on the next runs. Stop the daemon with Ctrl-C.

- [ ] **Step 8: Commit**

```bash
git add examples tests/mail.rs src
git commit -m "The mail check: every 5 minutes, a notification for mail not seen before"
```

---

### Task 8: README and Linux

**Files:**
- Create: `README.md`
- Test: the whole suite in a Linux container

- [ ] **Step 1: README.md**: what itero is (two sentences from `docs/design.md`), `cargo build`, the mail example (Task 7 step 7), where the design and this plan are. No em dash, no AI attribution.

- [ ] **Step 2: Linux**: with Docker Desktop (the `desktop-linux` context; do not start colima), run the suite in `rust:1.96`:

```bash
docker run --rm -v ~/projects/itero:/src -v itero-target:/target -v itero-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.96 bash -c "cargo test --no-fail-fast 2>&1 | grep -E '^test result|FAILED|panicked'"
```

Expected: every `test result: ok`.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "README: what itero is and the mail example"
```
