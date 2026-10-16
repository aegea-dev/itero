# Iteration 5: publish and subscribe

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Workflows talk to each other and to long-lived runs through
events for the workspace: a run publishes, waiting runs that match
receive, a failure is published for maintenance workflows, and `itero
errors` shows what failed and why.

**Architecture:** The journal stays the bus. An event for the workspace
(`itero.app`) still starts runs through triggers, and is now also copied
to every waiting run whose current state (or `*`) has a transition `on`
its type with a `match`, in the same transaction that handles it; each
copy then goes through the existing per-run delivery, where the pure
manager decides `match` and `when`. The daemon routes by type only and
decides nothing. Each run keeps the input it started with (`run.input`)
so `match` can correlate; a failed run keeps why it failed.

**Tech Stack:** Rust 1.96, SQLite (`rusqlite`), CEL (`cel-interpreter`),
serde. No new crate.

**Spec:** `docs/design.md`, section 14 (publish and subscribe, long-lived
runs, explicit keys), with section 12 (`"from": "*"`), section 15
(`run.failed`, `itero errors`, `GET /runs?state=failed`) and section 11
(names for people).

## Global Constraints

- Crates are MIT or Apache-2.0 only; this iteration adds none. `~x.y.z` requirements.
- Platforms: macOS and Linux; CI runs `make fmt-check lint-check test` on both, and `make test-docker`.
- Public text (commits, docs, comments, errors): no em dash, no AI attribution.
- Test fixtures use non-ASCII words (`ı`, `ş`, `ğ`, `ü`, `é`) in values; names, states and event types stay lowercase ASCII words.
- Every file is checked strictly: unknown keys are errors naming the file; explicit keys, never meaning by omission (design section 14).
- The manager stays pure: JSON in, JSON out. The daemon routes and saves; it evaluates no expression.
- A journal made by an earlier itero is migrated (`MIGRATIONS` only grows).
- Before each commit: `make fmt`, `make lint-check`, `make test` pass.
- Work on branch `iteration-5` from `origin/main`; stage files by name.

## Design decisions made here

- **Copies, not a deliveries table.** Section 14 proposed a `deliveries`
  table. Copying the event to each subscribed run's subject, in the
  transaction that handles it, gives the same guarantee (each subscriber
  gets it once, in its own transaction, never twice) with the delivery
  code that exists. The copy's `cause` is the original event's id.
- **A copy is not a promise.** The daemon copies to runs whose transitions
  could take the type; the run's manager evaluates `match` and `when`, and
  a run that does not take it logs "no transition" and stays where it is.
  A run busy in a chain ignores it (logged), as for any event today.
- **`run.input`** is the data of the event that started the run (`{}` for
  a schedule). It is stored with the run (journal schema 4) and is in
  `run` for every expression.
- **No feedback, no self-repair.** An event is never copied back to the run
  that published it. `run.failed` and `workflow.paused` never start or
  reach runs of the workflow they are about.

## Files

| File | Responsibility |
|---|---|
| `src/engine/mod.rs` | `Failed.why` |
| `src/engine/manager.rs` | `why`; `"from": "*"`; `match`; the `event` variable |
| `src/engine/eval.rs` | `Vars.event` |
| `src/engine/builtins.rs` | `event.publish@1` |
| `src/workflow/format.rs` | `Transition.match`; the `publish` step |
| `src/workflow/validate.rs` | checks for `*`, `match`, `publish` |
| `src/workflow/actions.rs` | `event.publish@1` among the built-ins |
| `src/engine/journal.rs` | schema 4 (`runs.input`); `Run.input`; `listeners`; copies in `start_runs_for_event`; `runs_page` by state |
| `src/engine/daemon.rs` | `run.input` in `run`; `run.failed`; subscribers in `start_triggered` |
| `src/engine/http.rs` | `GET /runs?state=` |
| `src/cli.rs` | `itero errors` |
| `tests/manager.rs`, `tests/pubsub.rs` (new), `tests/breaker.rs`, `tests/http.rs`, `tests/journal.rs` | tests |
| `README.md`, `docs/design.md`, `docs/src/content/docs/...` | docs |

---

### Task 0: The branch

- [ ] **Step 1**

```bash
cd ~/projects/itero
git fetch -q && git switch -c iteration-5 origin/main
make test
```

Expected: all tests pass.

---

### Task 1: A failed run says why, and `run.failed` is published

**Files:**
- Modify: `src/engine/mod.rs` (`Failed`), `src/engine/manager.rs` (`fail_at`), `src/engine/daemon.rs` (`deliver`, `start_triggered`)
- Test: `tests/manager.rs`, `tests/breaker.rs`

**Interfaces:**
- Produces: `Failed.why: String` (`#[serde(default)]`, so runtimes saved before read as `""`); the event `run.failed` for the workspace, data `{"workflow", "version", "run", "from", "why"}`; the rule that `run.failed` and `workflow.paused` never start a run of the workflow named in their data.

- [ ] **Step 1: Write the failing tests**

In `tests/manager.rs`, after the existing failure tests:

```rust
#[test]
fn a_failed_run_keeps_why() {
    let a = requested();
    let data = json!({"chain": "e1.2", "index": 0, "error": "bağlantı koptu"});
    let b = decide(&input(a.runtime, "action.failed", "e2", data)).unwrap();
    assert_eq!(b.runtime.state, "failed");
    let f = b.runtime.failed.unwrap();
    assert_eq!(f.why, "http.request@1: bağlantı koptu");
}
```

In `tests/breaker.rs`:

```rust
/// Starts when any workflow fails, and remembers which.
const BAKIM: &str = r#"{ "name": "bakim", "description": "bakım", "triggers": [ { "on": "run.failed" } ],
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "action": "memory.set@1",
              "with": { "son": "${input.workflow}", "neden": "${input.why}" } } ] } ] }"#;

#[test]
fn a_failed_run_is_published_and_a_maintenance_workflow_hears_it() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), BOZUK);
    std::fs::write(d.path().join("workflows/bakim.json"), BAKIM).unwrap();
    runs(&ws, 1);
    run_once(&ws, t("2026-10-16T10:00:30Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let said: Vec<_> = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter(|e| e.event.kind == "run.failed")
        .collect();
    assert_eq!(said.len(), 1);
    let data = &said[0].event.data;
    assert_eq!(data["workflow"], "bozuk");
    assert_eq!(data["from"], "basla");
    assert!(data["why"].as_str().unwrap().contains("sh@1"), "{data}");
    let m = j.memory("bakim").unwrap();
    assert_eq!(m["son"], "bozuk");
    assert_eq!(m["neden"], data["why"]);
}

#[test]
fn a_maintenance_workflow_never_starts_on_its_own_failure() {
    let d = tempfile::tempdir().unwrap();
    // fails, and listens to failures: it must not start itself
    let kendi = BOZUK.replace(
        r#""triggers": [ { "on": "dene.simdi" } ]"#,
        r#""triggers": [ { "on": "dene.simdi" }, { "on": "run.failed" } ]"#,
    );
    let ws = workspace(d.path(), &kendi);
    runs(&ws, 1);
    for s in 10..13 {
        run_once(&ws, t(&format!("2026-10-16T10:00:{s}Z")), exe()).unwrap();
    }
    assert_eq!(Journal::open(&ws).unwrap().runs().unwrap().len(), 1);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test manager a_failed_run_keeps_why; cargo test --test breaker`
Expected: compile error (no `why`), then no `run.failed` event.

- [ ] **Step 3: Implement**

`src/engine/mod.rs`, in `Failed`:

```rust
    /// Why it failed, as the manager logged it.
    #[serde(default)]
    pub why: String,
```

`src/engine/manager.rs`, in `fail_at`, the `Failed` literal gets
`why: why.clone()` (move the log line's `why` use after, or format first).

`src/engine/daemon.rs`, in `deliver`, beside the breaker (the events
vector is already `mut`), when `ended && state == FAILED`:

```rust
    if ended && state == FAILED {
        let f = answer.runtime.failed.as_ref();
        events.push(Envelope::new(
            "daemon",
            "run.failed",
            &journal::subject("app"),
            &stamp(now),
            Some(r.event.id.clone()),
            json!({
                "workflow": run.workflow,
                "version": crate::workflow::short(crate::workflow::split_pin(&run.pin).1),
                "run": id,
                "from": f.map(|f| f.from.as_str()),
                "why": f.map(|f| f.why.as_str()),
            }),
        ));
    }
```

`run.failed` goes before `workflow.paused` when both happen.

In `start_triggered`, leave out the workflow an engine event is about:

```rust
/// The workflow an engine event (`run.failed`, `workflow.paused`) is
/// about: its own runs never hear it, so a broken workflow never starts
/// itself to repair itself.
fn about(e: &Envelope) -> Option<&str> {
    matches!(e.kind.as_str(), "run.failed" | "workflow.paused")
        .then(|| e.data.get("workflow").and_then(Value::as_str))
        .flatten()
}
```

and filter `.filter(|(_, f)| Some(f.name.as_str()) != about(&r.event))`
before the triggers are matched.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test manager --test breaker && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/mod.rs src/engine/manager.rs src/engine/daemon.rs tests/manager.rs tests/breaker.rs
git commit -m "A failed run says why, and run.failed is published"
```

---

### Task 2: `itero errors` and `GET /runs?state=`

**Files:**
- Modify: `src/engine/journal.rs` (`runs_page`), `src/cli.rs`, `src/engine/http.rs`
- Test: `tests/breaker.rs`, `tests/http.rs`

**Interfaces:**
- Consumes: `Failed.why` (Task 1).
- Produces: `Journal::runs_page(&self, workflow: Option<&str>, state: Option<&str>, limit: usize) -> Result<Vec<Run>>` (one more argument; update its callers: `cli.rs` runs, `http.rs` runs, `tests/breaker.rs`).

- [ ] **Step 1: Write the failing tests**

`tests/breaker.rs`:

```rust
#[test]
fn itero_errors_shows_failed_runs_newest_first_with_why() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &BOZUK.replace("\"pause_after\": 2", "\"pause_after\": 0"));
    runs(&ws, 2);
    let o = std::process::Command::new(exe())
        .args(["errors", "--limit", "5"])
        .current_dir(d.path())
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8_lossy(&o.stdout);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with("2026-10-16T10:00:02Z"), "newest first: {out}");
    assert!(lines.iter().all(|l| l.contains("bozuk") && l.contains("basla -> bitti") && l.contains("sh@1")), "{out}");
}
```

`tests/http.rs`, in `runs_workflows_and_memory_can_be_read`:

```rust
    let (_, none) = send(&app, get("/runs?state=failed")).await;
    assert_eq!(none, json!([]));
    let (_, done) = send(&app, get("/runs?state=bitti")).await;
    assert_eq!(done.as_array().unwrap().len(), 1);
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test breaker itero_errors --test http`
Expected: FAIL: no `errors` command; `?state=` ignored.

- [ ] **Step 3: Implement**

`runs_page`:

```rust
    /// The newest `limit` runs, of one workflow or all, in one state or
    /// any, newest first.
    pub fn runs_page(&self, workflow: Option<&str>, state: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut s = self.db.prepare(
            "SELECT id FROM runs WHERE (?1 IS NULL OR workflow = ?1)
               AND (?2 IS NULL OR json_extract(runtime, '$.state') = ?2)
             ORDER BY started DESC, id DESC LIMIT ?3",
        )?;
        let ids: Vec<String> = s
            .query_map(params![workflow, state, limit], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = vec![];
        for id in ids {
            out.extend(self.run(&id)?);
        }
        Ok(out)
    }
```

`http.rs`: `RunsQuery` gets `state: Option<String>`, passed through.

`cli.rs`:

```rust
    /// Failed runs, newest first: when, the workflow, the run, where and why
    Errors {
        /// How many
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
```

```rust
        Cmd::Errors { limit } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            for r in j.runs_page(None, Some(crate::workflow::FAILED), limit)? {
                let (from, to, why) = match &r.runtime.failed {
                    Some(f) => {
                        let to = j
                            .workflow(&r.pin)?
                            .and_then(|w| w.transitions.get(f.transition).map(|t| t.to.clone()))
                            .unwrap_or_default();
                        (f.from.clone(), to, f.why.clone())
                    }
                    None => (String::new(), String::new(), String::new()),
                };
                println!(
                    "{}  {:<20} {}  {from} -> {to}: {why}",
                    r.ended.as_deref().unwrap_or(&r.started),
                    r.workflow,
                    r.id
                );
            }
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test breaker --test http && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/journal.rs src/cli.rs src/engine/http.rs tests/breaker.rs tests/http.rs
git commit -m "itero errors: failed runs, where and why; GET /runs?state="
```

---

### Task 3: `"from": "*"`

**Files:**
- Modify: `src/workflow/validate.rs` (`check_transition`), `src/engine/manager.rs` (`trigger`)
- Test: `tests/manager.rs`, `tests/limits.rs`

**Interfaces:**
- Produces: `pub const ANY: &str = "*";` in `src/workflow/mod.rs`, the state a transition from anywhere names.

- [ ] **Step 1: Write the failing tests**

`tests/manager.rs`:

```rust
/// A run that can be cancelled from wherever it is, except that `odeme`
/// has its own answer to the same event.
const IPTAL: &str = r#"{ "name": "siparis", "states": ["yeni", "odeme", "iptal", "geri-odeme", "bitti"],
  "start": "yeni", "end": ["iptal", "geri-odeme", "bitti"],
  "transitions": [
    { "from": "yeni", "to": "odeme", "on": "odendi" },
    { "from": "odeme", "to": "bitti", "on": "gonderildi" },
    { "from": "odeme", "to": "geri-odeme", "on": "iptal.istendi" },
    { "from": "*", "to": "iptal", "on": "iptal.istendi" } ] }"#;

fn on(state: &str, kind: &str) -> Answer {
    let mut i = input(at(state), kind, "e9", json!({"neden": "vazgeçti"}));
    i.workflow = itero::workflow::parse(IPTAL).unwrap();
    decide(&i).unwrap()
}

#[test]
fn a_transition_from_anywhere_is_taken_where_nothing_closer_is() {
    assert_eq!(on("yeni", "iptal.istendi").runtime.state, "iptal");
    assert_eq!(on("odeme", "iptal.istendi").runtime.state, "geri-odeme", "the state's own wins");
    assert_eq!(on("yeni", "gonderildi").runtime.state, "yeni", "no transition");
}
```

`tests/limits.rs`:

```rust
#[test]
fn a_transition_from_anywhere_needs_on_and_takes_no_timeout() {
    let t = |tr: &str| {
        workflow::parse(&format!(
            r#"{{ "name": "a", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
                 "transitions": [ {tr} ] }}"#
        ))
    };
    assert!(t(r#"{ "from": "*", "to": "bitti", "on": "dur" }"#).is_ok());
    let e = t(r#"{ "from": "*", "to": "bitti" }"#).unwrap_err();
    assert!(e.to_string().contains("from \"*\" needs on"), "{e}");
    let e = t(r#"{ "from": "*", "to": "bitti", "on": "dur", "timeout": "5m", "on_timeout": "bitti" }"#).unwrap_err();
    assert!(e.to_string().contains("from \"*\" takes no timeout"), "{e}");
    let e = t(r#"{ "from": "basla", "to": "*", "on": "dur" }"#).unwrap_err();
    assert!(e.to_string().contains("to \"*\" is not a state"), "{e}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test manager a_transition_from --test limits`
Expected: FAIL: `from "*" is not a state`.

- [ ] **Step 3: Implement**

`src/workflow/mod.rs`: `pub const ANY: &str = "*";` with the doc
comment "The `from` of a transition from any state."

`validate.rs`, in `check_transition`, skip the state check for
`from == ANY` and add:

```rust
    if t.from == super::ANY {
        if t.on.is_none() {
            return Err("from \"*\" needs on: a transition from anywhere waits for an event".into());
        }
        if t.timeout.is_some() {
            return Err("from \"*\" takes no timeout".into());
        }
    }
```

`manager.rs`, in `trigger`: look for a transition from the run's own
state first, then from `*`:

```rust
        for from in [self.rt.state.as_str(), crate::workflow::ANY] {
            for (i, t) in self.i.workflow.transitions.iter().enumerate() {
                if t.from != from || t.on.as_deref() != Some(e.kind.as_str()) {
                    continue;
                }
                // `when` (and, from Task 4, `match`) as now
                ...
            }
            if found.is_some() {
                break;
            }
        }
```

(`self.rt.state` is borrowed while `found` is set; copy the state into a
local `String` first.) `enter` needs nothing: `*` transitions have `on`,
so they are never taken on entering, and no timeout, so they set no timer.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test manager --test limits && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workflow/mod.rs src/workflow/validate.rs src/engine/manager.rs tests/manager.rs tests/limits.rs
git commit -m "A transition from anywhere: \"from\": \"*\""
```

---

### Task 4: `match`, and each run's `run.input`

**Files:**
- Modify: `src/workflow/format.rs` (`Transition`), `src/workflow/validate.rs`, `src/engine/eval.rs` (`Vars`), `src/engine/manager.rs` (`vars`, `trigger`), `src/engine/journal.rs` (schema 4, `Run.input`, `insert_run`), `src/engine/daemon.rs` (`run_map`)
- Test: `tests/manager.rs`, `tests/journal.rs`

**Interfaces:**
- Produces: `Transition.r#match: Option<String>` (serde name `match`); `Vars.event: Option<&Value>` (the event as `{id, type, data}`, in `match` only); `Run.input: Value`; `run.input` in every expression.

- [ ] **Step 1: Write the failing tests**

`tests/manager.rs`:

```rust
/// Waits for a reply on its own thread; replies on other threads are not
/// its business.
const SOHBET: &str = r#"{ "name": "sohbet", "states": ["bekliyor", "cevaplandi"],
  "start": "bekliyor", "end": ["cevaplandi"],
  "transitions": [
    { "from": "bekliyor", "to": "cevaplandi", "on": "mesaj.geldi",
      "match": "event.data.konu == run.input.konu" } ] }"#;

fn sohbet(konu: &str) -> Answer {
    let mut i = input(at("bekliyor"), "mesaj.geldi", "e5", json!({"konu": konu, "metin": "merhaba dünya"}));
    i.workflow = itero::workflow::parse(SOHBET).unwrap();
    i.run.insert("input".into(), json!({"konu": "çay"}));
    decide(&i).unwrap()
}

#[test]
fn match_takes_only_the_events_that_are_this_runs() {
    assert_eq!(sohbet("çay").runtime.state, "cevaplandi");
    let other = sohbet("kahve");
    assert_eq!(other.runtime.state, "bekliyor");
    assert!(other.log.iter().any(|l| l.contains("no transition")), "{:?}", other.log);
}
```

`tests/limits.rs`:

```rust
#[test]
fn match_needs_on_and_compiles() {
    let t = |tr: &str| {
        workflow::parse(&format!(
            r#"{{ "name": "a", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
                 "transitions": [ {tr} ] }}"#
        ))
    };
    let e = t(r#"{ "from": "basla", "to": "bitti", "match": "true" }"#).unwrap_err();
    assert!(e.to_string().contains("match needs on"), "{e}");
    let e = t(r#"{ "from": "basla", "to": "bitti", "on": "x", "match": "event.data. ==" }"#).unwrap_err();
    assert!(e.to_string().contains("bad expression"), "{e}");
}
```

`tests/daemon.rs`: a run started by an event keeps its input (the
`yavas` workflow and `arrives` helper are already there, from iteration 4):

```rust
#[test]
fn a_run_keeps_the_input_it_started_with() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    arrives(&ws);
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs[0].input, serde_json::json!({"ad": "ağaç.txt"}));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test manager match_ --test limits match_ --test journal`
Expected: compile errors and unknown field `match`.

- [ ] **Step 3: Implement**

`format.rs`, in `Transition` after `when`:

```rust
    /// For an event the run did not ask for: whether it is this run's
    /// (`event.data.thread == run.input.thread`).
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub r#match: Option<String>,
```

`validate.rs`: `match` needs `on`; compile it like `when`.

`eval.rs`: `Vars` gets `pub event: Option<&'a Value>`; `value` adds
`c.add_variable_from_value("event", cel(e))` when it is `Some`.
`manager.rs`'s `vars` passes `event: None`; `trigger` builds
`let ev = json!({"id": e.id, "type": e.kind, "data": e.data});` and
evaluates `match` (when present) with `Vars { event: Some(&ev), ..}`
before `when`; both must hold.

`journal.rs`:

```rust
    // 4: the input each run started with, for correlation (run.input)
    "ALTER TABLE runs ADD COLUMN input TEXT;",
```

`Run` gets `pub input: Value` (read as `Value::Null` when the column is
null, for runs from before); `insert_run` writes `json(input)?`; the
`SELECT` in `run` reads it.

`daemon.rs`, where `run_map` is built: `run_map.insert("input".into(),
run.input.clone());`

- [ ] **Step 4: Run the tests**

Run: `make test`
Expected: PASS, including the journal migration test (a version-1
journal migrates through 2, 3 and 4).

- [ ] **Step 5: Commit**

```bash
git add src/workflow/format.rs src/workflow/validate.rs src/engine/eval.rs src/engine/manager.rs src/engine/journal.rs src/engine/daemon.rs tests/manager.rs tests/limits.rs tests/journal.rs
git commit -m "match: a transition takes only the events that are its run's"
```

---

### Task 5: Events for the workspace reach the runs waiting for them

**Files:**
- Modify: `src/engine/journal.rs` (`listeners`, `start_runs_for_event`), `src/engine/daemon.rs` (`start_triggered`)
- Test: `tests/pubsub.rs` (new)

**Interfaces:**
- Consumes: `match`, `run.input`, `"*"` (Tasks 3, 4); `about` (Task 1).
- Produces: `Journal::listeners(&self, kind: &str) -> Result<Vec<(String, String)>>`: the id and workflow of each run under way whose current state, or `*`, has a transition `on` `kind` with a `match`; `Journal::start_runs_for_event(&mut self, r: &Recorded, starts: &[(&File, &Map<String, Value>)], forward: &[String], at: &str) -> Result<Vec<Started>>` (was `seq` and `input`: both are in `r`): also publishes a copy of the event to each run in `forward`. Update its callers (`daemon.rs` and `tests/journal.rs`).

- [ ] **Step 1: Write the failing tests**

```rust
// tests/pubsub.rs
use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use serde_json::json;
use std::path::Path;

fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

fn workspace(d: &Path, flows: &[(&str, &str)]) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    for (name, text) in flows {
        std::fs::write(d.join(format!("workflows/{name}.json")), text).unwrap();
    }
    itero::workspace::find(d).unwrap()
}

fn emit(ws: &itero::workspace::Workspace, kind: &str, data: serde_json::Value) {
    itero::engine::events::emit(ws, kind, Some(data), None, "2026-10-16T10:00:00Z").unwrap();
}

/// A conversation per subject: started by `sohbet.basladi`, it waits for a
/// message on its own subject.
const SOHBET: &str = r#"{ "name": "sohbet", "triggers": [ { "on": "sohbet.basladi" } ],
  "states": ["bekliyor", "cevaplandi"], "start": "bekliyor", "end": ["cevaplandi"],
  "transitions": [
    { "from": "bekliyor", "to": "cevaplandi", "on": "mesaj.geldi",
      "match": "event.data.konu == run.input.konu",
      "do": [ { "action": "memory.set@1", "with": { "son": "${input.metin}" } } ] } ] }"#;

#[test]
fn an_event_reaches_only_the_run_it_matches() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("sohbet", SOHBET)]);
    emit(&ws, "sohbet.basladi", json!({"konu": "çay"}));
    emit(&ws, "sohbet.basladi", json!({"konu": "kahve"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    emit(&ws, "mesaj.geldi", json!({"konu": "kahve", "metin": "şekersiz lütfen"}));
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let mut states: Vec<(String, String)> = j
        .runs()
        .unwrap()
        .into_iter()
        .map(|r| (r.input["konu"].as_str().unwrap().to_string(), r.runtime.state))
        .collect();
    states.sort();
    assert_eq!(
        states,
        [("kahve".into(), "cevaplandi".into()), ("çay".into(), "bekliyor".into())]
    );
    assert_eq!(j.memory("sohbet").unwrap()["son"], "şekersiz lütfen");
}

/// Lives for good: every reading for its room is noted.
const DINLEYICI: &str = r#"{ "name": "dinleyici", "triggers": [ { "on": "oda.acildi" } ],
  "states": ["dinliyor"], "start": "dinliyor",
  "transitions": [
    { "from": "dinliyor", "to": "dinliyor", "on": "olcum.geldi",
      "match": "event.data.oda == run.input.oda",
      "do": [ { "action": "memory.set@1", "with": { "son": "${input.derece}" } } ] } ] }"#;

#[test]
fn a_long_lived_run_takes_every_event_and_stays() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("dinleyici", DINLEYICI)]);
    emit(&ws, "oda.acildi", json!({"oda": "mutfak"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    for (s, derece) in [(2, 19), (3, 21), (4, 23)] {
        emit(&ws, "olcum.geldi", json!({"oda": "mutfak", "derece": derece}));
        emit(&ws, "olcum.geldi", json!({"oda": "salon", "derece": 30}));
        run_once(&ws, t(&format!("2026-10-16T10:00:0{s}Z")), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].runtime.state, "dinliyor");
    assert!(runs[0].ended.is_none(), "it lives on");
    assert_eq!(j.memory("dinleyici").unwrap()["son"], 23);
}

#[test]
fn an_event_nothing_waits_for_is_noted() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("sohbet", SOHBET)]);
    emit(&ws, "mesaj.geldi", json!({"konu": "çay"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let notes: Vec<String> = j.events(Some("itero.app")).unwrap().into_iter().filter_map(|e| e.note).collect();
    assert_eq!(notes, ["no workflow starts on it"]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test pubsub`
Expected: FAIL: the message reaches no run.

- [ ] **Step 3: Implement**

`journal.rs`:

```rust
    /// The runs under way that may want an event of type `kind`: their
    /// current state, or `*`, has a transition `on` it with a `match`.
    /// Their managers decide; this only routes. Id and workflow of each.
    pub fn listeners(&self, kind: &str) -> Result<Vec<(String, String)>> {
        let mut s = self
            .db
            .prepare("SELECT id, workflow, pin, json_extract(runtime, '$.state') FROM runs WHERE ended IS NULL ORDER BY started, id")?;
        let rows: Vec<(String, String, String, String)> = s
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut defs: std::collections::BTreeMap<String, Option<File>> = Default::default();
        let mut out = vec![];
        for (id, wf, pin, state) in rows {
            if !defs.contains_key(&pin) {
                defs.insert(pin.clone(), self.workflow(&pin)?);
            }
            let Some(Some(f)) = defs.get(&pin) else { continue };
            let wants = f.transitions.iter().any(|t| {
                (t.from == state || t.from == workflow::ANY)
                    && t.on.as_deref() == Some(kind)
                    && t.r#match.is_some()
            });
            if wants {
                out.push((id, wf));
            }
        }
        Ok(out)
    }
```

`start_runs_for_event` takes the recorded event `r` (its `seq`, and its
`event.data` as the input) and `forward: &[String]`; in its transaction,
for each run id, it publishes a copy:

```rust
        let e = &r.event;
        for id in forward {
            publish(&tx, &Envelope::new(&e.source, &e.kind, &subject(id), at, Some(e.id.clone()), e.data.clone()))?;
        }
```

The note is "no workflow starts on it" only when there are no starts and
no forwards.

`daemon.rs`, in `start_triggered`:

```rust
    let about = about(&r.event);
    // never back to the run that published it
    let publisher = r.event.source.strip_prefix("manager/");
    let forward: Vec<String> = j
        .listeners(&r.event.kind)?
        .into_iter()
        .filter(|(id, wf)| Some(id.as_str()) != publisher && Some(wf.as_str()) != about)
        .map(|(id, _)| id)
        .collect();
```

and pass `&forward`. The log line "no workflow starts on it" is printed
only when there are no starts and no forwards.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test pubsub && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/journal.rs src/engine/daemon.rs tests/pubsub.rs tests/journal.rs
git commit -m "Events for the workspace reach the runs waiting for them"
```

---

### Task 6: The `publish` step

**Files:**
- Modify: `src/workflow/format.rs` (`Call`, `Action` deserialize), `src/workflow/actions.rs` (`BUILT_IN`, `check`), `src/engine/builtins.rs` (`PURE`, `run`)
- Test: `tests/manager.rs`, `tests/limits.rs`, `tests/pubsub.rs`

**Interfaces:**
- Produces: the step `{ "publish": "<type>", "data": {...} }` (also `id`, `when`, `description`), read as the built-in `event.publish@1` with `with: {"type", "data"}`; its event goes to the workspace when the chain succeeds.

- [ ] **Step 1: Write the failing tests**

`tests/limits.rs`:

```rust
#[test]
fn a_publish_step_is_read_and_checked() {
    let t = |step: &str| {
        workflow::parse(&format!(
            r#"{{ "name": "a", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
                 "transitions": [ {{ "from": "basla", "to": "bitti", "do": [ {step} ] }} ] }}"#
        ))
    };
    let f = t(r#"{ "publish": "rapor.hazir", "data": { "ad": "çeyrek" } }"#).unwrap();
    let a = &f.transitions[0].actions[0];
    assert_eq!(a.action, "event.publish@1");
    assert_eq!(a.with.as_ref().unwrap()["type"], "rapor.hazir");
    for (step, says) in [
        (r#"{ "publish": "run.failed" }"#, "the engine's own"),
        (r#"{ "publish": "Rapor" }"#, "an event type"),
        (r#"{ "publish": "a.b", "action": "notify@1" }"#, "publish or action"),
        (r#"{ "publish": "a.b", "with": {} }"#, "data, not with"),
        (r#"{ "action": "notify@1", "data": {} }"#, "data is for publish"),
    ] {
        let e = t(step).unwrap_err();
        assert!(e.to_string().contains(says), "{step}: {e}");
    }
}
```

`tests/pubsub.rs`:

```rust
const RAPOR: &str = r#"{ "name": "rapor", "triggers": [ { "on": "ay.bitti" } ],
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "publish": "rapor.hazir", "data": { "ay": "${input.ay}" } } ] } ] }"#;

const DUYURU: &str = r#"{ "name": "duyuru", "triggers": [ { "on": "rapor.hazir" } ],
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "action": "memory.set@1", "with": { "ay": "${input.ay}" } } ] } ] }"#;

#[test]
fn a_published_event_starts_the_workflows_waiting_for_it() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("rapor", RAPOR), ("duyuru", DUYURU)]);
    emit(&ws, "ay.bitti", json!({"ay": "Ekim"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.memory("duyuru").unwrap()["ay"], "Ekim");
    let published = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .find(|e| e.event.kind == "rapor.hazir")
        .unwrap();
    assert!(published.event.source.starts_with("manager/"));
}

/// Listens to `yanki` and publishes `yanki` again: it must not hear itself.
const YANKI: &str = r#"{ "name": "yanki", "triggers": [ { "on": "yanki.basla" } ],
  "states": ["dinliyor"], "start": "dinliyor",
  "transitions": [ { "from": "dinliyor", "to": "dinliyor", "on": "yanki", "match": "true",
    "do": [ { "publish": "yanki", "data": { "kez": "tekrar" } } ] } ] }"#;

#[test]
fn a_run_never_hears_what_it_published() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("yanki", YANKI)]);
    emit(&ws, "yanki.basla", json!({}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    emit(&ws, "yanki", json!({"kez": "ilk"}));
    for s in 2..6 {
        run_once(&ws, t(&format!("2026-10-16T10:00:0{s}Z")), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let echoes = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter(|e| e.event.kind == "yanki")
        .count();
    assert_eq!(echoes, 2, "the first, and one answer to it; no loop");
}
```

In `tests/manager.rs`, a publish step in a chain gives an `OutEvent`
with `target: "app"` only when the chain completes (follow the existing
`event.emit@1` test's shape there).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test limits a_publish --test pubsub`
Expected: FAIL: unknown field `publish`.

- [ ] **Step 3: Implement**

`format.rs`: `Call` gets `#[serde(default)] action: Option<String>`,
`#[serde(default)] publish: Option<String>`, `#[serde(default)] data:
Option<Value>`. In `Action`'s deserialize:

```rust
                let (action, with) = match (c.action, c.publish) {
                    (Some(a), None) => {
                        if c.data.is_some() {
                            return Err(serde::de::Error::custom("data is for publish; an action takes with"));
                        }
                        (a, c.with)
                    }
                    (None, Some(kind)) => {
                        if c.with.is_some() {
                            return Err(serde::de::Error::custom("publish takes data, not with"));
                        }
                        let mut w = Map::new();
                        w.insert("type".into(), Value::String(kind));
                        w.insert("data".into(), c.data.unwrap_or_else(|| Value::Object(Map::new())));
                        ("event.publish@1".to_string(), Some(w))
                    }
                    _ => return Err(serde::de::Error::custom("a step is publish or action, one of them")),
                };
```

`actions.rs`: `"event.publish@1"` joins `BUILT_IN`, and `check`:

```rust
        "event.publish@1" => {
            only(&["type", "data"])?;
            let kind = with.get("type").and_then(Value::as_str).unwrap_or("");
            if !kind.contains("${") {
                if !kind.split('.').all(super::validate::is_word) {
                    return Err(format!("publish \"{kind}\" is an event type, like report.ready"));
                }
                if crate::engine::events::engine_own(kind) {
                    return Err(format!("publish \"{kind}\" is the engine's own"));
                }
            }
            if !with.get("data").is_none_or(Value::is_object) {
                return Err("publish's data is an object".into());
            }
        }
```

`builtins.rs`: `"event.publish@1"` joins `PURE`; in `run`:

```rust
        "event.publish@1" => {
            let kind = text("type")?;
            if !kind.split('.').all(crate::workflow::validate::is_word) || crate::engine::events::engine_own(&kind) {
                return Err(format!("event.publish@1: \"{kind}\" cannot be published"));
            }
            let data = with.get("data").cloned().unwrap_or(Value::Object(Map::new()));
            Ok(Done { output: data.clone(), memory: Map::new(), emit: Some((kind, "app".into(), data)) })
        }
```

The daemon already turns an `OutEvent` with target `app` into an event on
`itero.app`, with source `manager/<run id>` (which Task 5's "never back to
the publisher" reads).

- [ ] **Step 4: Run the tests**

Run: `cargo test --test limits --test pubsub --test manager && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workflow/format.rs src/workflow/actions.rs src/engine/builtins.rs tests/limits.rs tests/pubsub.rs tests/manager.rs
git commit -m "The publish step: an event for the workspace, when the chain succeeds"
```

---

### Task 7: Documentation, and finishing

**Files:**
- Modify: `README.md`; `docs/design.md` (section 14: "Done in iteration 5", the copies instead of a deliveries table, `run.input`); `docs/src/content/docs/guides/apps.md` (publish and subscribe: `publish`, `match`, long-lived runs, `run.input`); `guides/workflows.md` (`"from": "*"`, `match`, the `publish` step, `run.input`); `reference/workflow-file.md` (`match`, `"*"`, the publish step); `reference/commands.md` (`itero errors`); `reference/http.md` (`?state=`); `reference/glossary.md` (publish, subscribe, match, listener, `run.failed`); `guides/actions.md` (`event.publish@1` among the built-ins).

- [ ] **Step 1: Write them**, each change in the page's own words and form.
- [ ] **Step 2: Check them**

```bash
cd docs && yarn build && cd ..
grep -rn "$(printf '\342\200\224')" README.md docs/*.md docs/src/content/docs || echo "no em dash"
make fmt-check lint-check test
make test-docker
```

Expected: all pass, no em dash.

- [ ] **Step 3: Commit**

```bash
git add README.md docs
git commit -m "Docs: publish and subscribe, match, from anywhere, itero errors"
```

## Self-review notes

- Spec coverage (section 14): subscribers are triggers and waiting runs
  (Task 5); correlation by `match` (Task 4); publishing (Task 6);
  explicit keys (Tasks 3, 4, 6); long-lived runs (Task 5's test); a
  journal-only bus, NATS-shaped subjects unchanged. Section 12: `"from":
  "*"` with the own state winning (Task 3). Section 15: `run.failed` and
  `workflow.paused` for subscribers (Task 1, and Task 4 of iteration 4),
  `itero errors` and `?state=failed` (Task 2). Not here, by the design's
  order: `at` timers, journal compaction, groups.
- Type names used across tasks: `Failed.why` (1), `runs_page(workflow,
  state, limit)` (2), `workflow::ANY` (3), `Transition.r#match`,
  `Vars.event`, `Run.input` (4), `listeners`, `start_runs_for_event(r,
  starts, forward, at)` (5), `event.publish@1` (6).
