# Iteration 4: sources, file watching, concurrency and the breaker

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** v2 of the mail check: a notification within seconds of mail
arriving, because a source watches the mailbox and sends an event, with
guards so runs never pile up or fail for ever.

**Architecture:** A **source** is a long-lived program that prints one JSON
event per line on stdout; the daemon supervises each one (restart with a
growing pause, give up after too many restarts) and writes its lines to the
journal as events for the workspace, where `on` triggers start runs. The
built-in `watch@1` source is `itero source watch@1`, the same protocol,
built on the OS's own file events (kqueue on macOS, inotify on Linux)
behind one `Backend` interface, with debouncing in one place. Workflows
gain `concurrency` (queue or skip starts beyond a maximum) and
`pause_after` (the breaker: a workflow pauses after that many failed runs
in a row, publishing `workflow.paused`; `itero resume` or a new version
starts it again).

**Tech Stack:** Rust 1.96 (`rust-toolchain.toml`), `libc` for kqueue and
inotify (already a dependency), SQLite through `rusqlite` (the journal),
serde, the existing `itero` binary for every child process.

**Spec:** `docs/design.md`, sections 15 (guards), 18 (sources and
connectors, the watch decision) and 19 (v2).

## Global Constraints

- Crates are MIT or Apache-2.0 only; this iteration adds no crate (`libc` is there). Dependencies use `~x.y.z` requirements.
- Platforms: macOS (kqueue) and Linux (inotify). Code for one is `#[cfg(target_os = ...)]`; the other must still build.
- Public text (commits, docs, comments, errors): no em dash, no AI attribution.
- Test fixtures use non-ASCII words (`ı`, `ş`, `ğ`, `ü`, `é`) in titles, paths and data.
- Every file the user writes is checked strictly: unknown keys are errors naming the file.
- Secrets reach a source only through `${env.NAME}` in its `with`, filled when it starts, never written to the journal.
- Before each commit: `make fmt`, `make lint-check`, `make test` pass.
- Commits are on branch `iteration-4`, cut from `design-next`; stage files by name.

## Files

| File | Responsibility |
|---|---|
| `src/workflow/format.rs` | `concurrency`, `pause_after` in a workflow file |
| `src/workflow/validate.rs`, `src/workflow/mod.rs` | their checks; left out of the version |
| `src/engine/journal.rs` | migration 3: `queued`, `paused`, `cursors`; admission, the queue, pausing, cursors |
| `src/engine/daemon.rs` | starts go through admission; the queue is released each pass; the breaker in `deliver`; new versions resume; `Health.sources` |
| `src/engine/events.rs` | `engine_own` is public and covers `workflow.` and `source.` |
| `src/engine/http.rs` | `/health` shows sources; `/workflows` shows `paused` |
| `src/watch/mod.rs` | `Kind`, `Change`, `Backend`, `open`, `Debouncer`, `run` |
| `src/watch/kqueue.rs` | the macOS backend |
| `src/watch/inotify.rs` | the Linux backend |
| `src/sources/mod.rs` | a source's file (`sources/<name>.json`), its checks, the line protocol |
| `src/sources/supervise.rs` | running one source: spawn, settings, lines to the journal, restarts, the give-up guard |
| `src/workspace.rs` | the `sources` setting and `Workspace::sources()` |
| `src/cli.rs` | `itero resume`, hidden `itero source watch@1`, sources in `itero check`, supervisors started by `itero daemon` |
| `src/lib.rs` | `pub mod watch; pub mod sources;` |
| `examples/mail-check/` | v2: `sources/mailbox.json`, the `on` trigger, hourly safety net, `concurrency` |
| `tests/limits.rs`, `tests/breaker.rs`, `tests/watch.rs`, `tests/sources.rs`, `tests/mail.rs`, `tests/journal.rs`, `tests/http.rs` | tests |
| `README.md`, `docs/src/content/docs/...` | documentation |

---

### Task 0: The branch

- [ ] **Step 1: Cut it**

```bash
cd ~/projects/itero
git switch design-next
git switch -c iteration-4
make test
```

Expected: all tests pass before any change.

---

### Task 1: `concurrency` and `pause_after` in a workflow file

**Files:**
- Modify: `src/workflow/format.rs` (struct `File`, new `Concurrency`, `Then`)
- Modify: `src/workflow/validate.rs` (`validate`)
- Modify: `src/workflow/mod.rs` (`version`)
- Test: `tests/limits.rs` (new)

**Interfaces:**
- Produces: `itero::workflow::format::{Concurrency, Then}`; `File.concurrency: Option<Concurrency>`; `File.pause_after: Option<u32>`; `File::pause_after_or_default(&self) -> u32`; `pub const DEFAULT_PAUSE_AFTER: u32 = 5` in `format.rs`.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/limits.rs
use itero::workflow::{self, format::Then};

fn flow(extra: &str) -> String {
    format!(
        r#"{{ "name": "sayac-yok", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             {extra}
             "transitions": [ {{ "from": "basla", "to": "bitti" }} ] }}"#
    )
}

#[test]
fn concurrency_and_pause_after_read_and_default() {
    let f = workflow::parse(&flow(r#""concurrency": { "max": 1, "then": "skip" }, "pause_after": 3,"#)).unwrap();
    let c = f.concurrency.as_ref().unwrap();
    assert_eq!((c.max, c.then), (1, Then::Skip));
    assert_eq!(f.pause_after_or_default(), 3);
    let f = workflow::parse(&flow(r#""concurrency": { "max": 2 },"#)).unwrap();
    assert_eq!(f.concurrency.unwrap().then, Then::Queue, "queue unless said");
    assert_eq!(f.pause_after_or_default(), 5);
}

#[test]
fn a_concurrency_of_zero_or_an_unknown_then_is_an_error() {
    let e = workflow::parse(&flow(r#""concurrency": { "max": 0 },"#)).unwrap_err();
    assert!(e.to_string().contains("concurrency.max is at least 1"), "{e}");
    let e = workflow::parse(&flow(r#""concurrency": { "max": 1, "then": "bekleş" },"#)).unwrap_err();
    assert!(e.to_string().contains("bekleş"), "{e}");
}

#[test]
fn limits_are_not_part_of_the_version() {
    let a = workflow::parse(&flow("")).unwrap();
    let b = workflow::parse(&flow(r#""concurrency": { "max": 1 }, "pause_after": 0,"#)).unwrap();
    assert_eq!(workflow::version(&a), workflow::version(&b));
}
```

Workflow names and states are lowercase ASCII words (`is_word`), so the
Turkish letters live in values: the unknown `then` is `"bekleş"`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test limits`
Expected: compile error, no field `concurrency`.

- [ ] **Step 3: Add the fields**

In `src/workflow/format.rs`, after `transitions` in `File`:

```rust
    /// How many runs may be under way at once, and what happens to a start
    /// beyond that. Without it, any number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
    /// Failed runs in a row that pause the workflow: 5 unless given, 0 never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_after: Option<u32>,
}

/// Failed runs in a row that pause a workflow when it says nothing.
pub const DEFAULT_PAUSE_AFTER: u32 = 5;

impl File {
    pub fn pause_after_or_default(&self) -> u32 {
        self.pause_after.unwrap_or(DEFAULT_PAUSE_AFTER)
    }
}

/// At most `max` runs under way; a start beyond waits its turn (`queue`)
/// or is dropped (`skip`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    pub max: u32,
    #[serde(default)]
    pub then: Then,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Then {
    #[default]
    Queue,
    Skip,
}
```

(Remove the struct's old closing brace so `File` ends after `pause_after`.)

In `src/workflow/validate.rs`, in `validate` after the triggers loop:

```rust
    if let Some(c) = &f.concurrency
        && c.max == 0
    {
        return Err(whole("concurrency.max is at least 1".into()));
    }
```

In `src/workflow/mod.rs`, in `version`, after `f.triggers.clear();`:

```rust
    // how many run at once, and when to pause, say when it runs, not what
    // a run does
    f.concurrency = None;
    f.pause_after = None;
```

Update `version`'s doc comment: "Formatting, descriptions, triggers,
concurrency and pause_after never count".

- [ ] **Step 4: Run the tests**

Run: `cargo test --test limits && make test`
Expected: PASS, and every other test still passes (versions of existing
workflows do not change: the new fields serialize to nothing when unset).

- [ ] **Step 5: Commit**

```bash
git add src/workflow/format.rs src/workflow/validate.rs src/workflow/mod.rs tests/limits.rs
git commit -m "Workflows say how many runs at once, and when to pause"
```

---

### Task 2: The journal: queue, pauses and cursors

**Files:**
- Modify: `src/engine/journal.rs`
- Test: `tests/journal.rs`

**Interfaces:**
- Consumes: `File.concurrency`, `Then` (Task 1).
- Produces:
  - `pub enum Started { Run(String), Queued, Skipped, Paused }` (derive `Debug, Clone, PartialEq`).
  - `Journal::start_run(&mut self, f: &File, args: &Map<String, Value>, now: &str, schedule: Option<&str>) -> Result<Started>` (was `Result<String>`).
  - `Journal::start_runs_for_event(&mut self, seq: i64, starts: &[(&File, &Map<String, Value>)], input: &Value, at: &str) -> Result<Vec<Started>>` (was `Vec<String>`).
  - `Journal::release_queued(&mut self, flows: &[File], now: &str) -> Result<Vec<String>>`: run ids started from the queue.
  - `pub struct Paused { pub workflow: String, pub pin: String, pub at: String, pub why: String }` (derive `Debug, Clone, PartialEq, Serialize`).
  - `Journal::paused(&self) -> Result<Vec<Paused>>`, `Journal::resume(&self, workflow: &str) -> Result<bool>`.
  - `pub struct Pause<'a> { pub workflow: &'a str, pub pin: &'a str, pub why: String }` and a new field `Handled.pause: Option<Pause<'a>>`.
  - `Journal::failed_in_a_row(&self, workflow: &str) -> Result<u32>`.
  - `Journal::cursor(&self, source: &str) -> Result<Option<Value>>`, `Journal::set_cursor(&self, source: &str, value: &Value) -> Result<()>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/journal.rs` (it already has a helper making a workspace;
reuse it, or add this one):

```rust
use itero::engine::journal::{Journal, Pause, Started};
use serde_json::{Map, Value, json};

fn ws_with(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

fn tek(then: &str) -> itero::workflow::File {
    itero::workflow::parse(&format!(
        r#"{{ "name": "tek", "states": ["bekle", "bitti"], "start": "bekle", "end": ["bitti"],
             "concurrency": {{ "max": 1, "then": "{then}" }},
             "transitions": [ {{ "from": "bekle", "to": "bitti", "on": "tamam" }} ] }}"#
    ))
    .unwrap()
}

#[test]
fn a_start_beyond_the_maximum_is_queued_and_released_when_a_slot_frees() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("queue");
    let args = Map::new();
    let first = j.start_run(&f, &args, "2026-10-16T10:00:00Z", None).unwrap();
    let Started::Run(id) = first else { panic!("{first:?}") };
    assert_eq!(j.start_run(&f, &args, "2026-10-16T10:00:01Z", None).unwrap(), Started::Queued);
    assert!(j.release_queued(&[f.clone()], "2026-10-16T10:00:02Z").unwrap().is_empty(), "still full");
    // the first run ends
    let run = j.run(&id).unwrap().unwrap();
    let mut rt = run.runtime.clone();
    rt.state = "bitti".into();
    j.handle(itero::engine::journal::Handled {
        seq: 0, at: "2026-10-16T10:00:03Z", error: None,
        run: Some((&id, &rt, true)), timers: None, events: vec![], memory: None, pause: None,
    }).unwrap();
    let released = j.release_queued(&[f], "2026-10-16T10:00:04Z").unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(j.runs().unwrap().len(), 2);
}

#[test]
fn a_start_beyond_the_maximum_is_skipped_when_the_workflow_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("skip");
    j.start_run(&f, &Map::new(), "2026-10-16T10:00:00Z", None).unwrap();
    assert_eq!(j.start_run(&f, &Map::new(), "2026-10-16T10:00:01Z", None).unwrap(), Started::Skipped);
    assert_eq!(j.runs().unwrap().len(), 1);
}

#[test]
fn a_paused_workflow_starts_nothing_until_resumed() {
    let d = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&ws_with(d.path())).unwrap();
    let f = tek("queue");
    j.handle(itero::engine::journal::Handled {
        seq: 0, at: "2026-10-16T10:00:00Z", error: None, run: None, timers: None,
        events: vec![], memory: None,
        pause: Some(Pause { workflow: "tek", pin: "tek@abc", why: "5 başarısız".into() }),
    }).unwrap();
    assert_eq!(j.paused().unwrap()[0].why, "5 başarısız");
    assert_eq!(j.start_run(&f, &Map::new(), "2026-10-16T10:00:01Z", None).unwrap(), Started::Paused);
    assert!(j.resume("tek").unwrap());
    assert!(!j.resume("tek").unwrap(), "not paused any more");
    assert!(matches!(j.start_run(&f, &Map::new(), "2026-10-16T10:00:02Z", None).unwrap(), Started::Run(_)));
}

#[test]
fn a_cursor_is_kept_per_source() {
    let d = tempfile::tempdir().unwrap();
    let j = Journal::open(&ws_with(d.path())).unwrap();
    assert_eq!(j.cursor("posta").unwrap(), None);
    j.set_cursor("posta", &json!({"uid": 42, "kutu": "Gelen"})).unwrap();
    j.set_cursor("posta", &json!({"uid": 43, "kutu": "Gelen"})).unwrap();
    assert_eq!(j.cursor("posta").unwrap(), Some(json!({"uid": 43, "kutu": "Gelen"})));
    let _: Option<Value> = j.cursor("başka").unwrap();
}
```

`handle` with `seq: 0` updates no event row (there is none with seq 0),
which is what these tests want: they test the run, pause and memory parts.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test journal`
Expected: compile errors: no `Started`, no `pause` field, no `cursor`.

- [ ] **Step 3: Implement**

In `src/engine/journal.rs`:

1. Migration 3. Change `MIGRATIONS` to two steps:

```rust
const MIGRATIONS: [&str; 2] = [
    // 2: what handling an event did, when no run shows it
    "ALTER TABLE events ADD COLUMN note TEXT;",
    // 3: starts waiting for a free slot, paused workflows, sources' cursors
    "CREATE TABLE queued (seq INTEGER PRIMARY KEY AUTOINCREMENT, workflow TEXT NOT NULL, args TEXT NOT NULL, input TEXT NOT NULL, at TEXT NOT NULL);
     CREATE TABLE paused (workflow TEXT PRIMARY KEY, pin TEXT NOT NULL, at TEXT NOT NULL, why TEXT NOT NULL);
     CREATE TABLE cursors (source TEXT PRIMARY KEY, value TEXT NOT NULL);",
];
```

2. Types, near `Run`:

```rust
/// What asking for a run did.
#[derive(Debug, Clone, PartialEq)]
pub enum Started {
    /// A run started: its id.
    Run(String),
    /// The workflow is at its maximum: the start waits its turn.
    Queued,
    /// The workflow is at its maximum and skips starts beyond it.
    Skipped,
    /// The workflow is paused: nothing starts.
    Paused,
}

/// A paused workflow: the version that failed, when, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Paused {
    pub workflow: String,
    pub pin: String,
    pub at: String,
    pub why: String,
}

/// A workflow to pause, as part of an answer.
pub struct Pause<'a> {
    pub workflow: &'a str,
    pub pin: &'a str,
    pub why: String,
}
```

Add to `Handled`, after `memory`:

```rust
    /// Pauses a workflow (the breaker), in the same transaction.
    pub pause: Option<Pause<'a>>,
```

3. Admission, as a free function beside `insert_run`:

```rust
/// Starts a run of `f` unless it is paused or at its maximum; then queues
/// or skips the start as the workflow says. A start that finds others
/// queued queues behind them, so starts keep their order.
fn admit(
    db: &Connection,
    f: &File,
    args: &Map<String, Value>,
    input: &Value,
    now: &str,
) -> Result<Started> {
    let paused: bool = db
        .query_row("SELECT 1 FROM paused WHERE workflow = ?1", params![f.name], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    if paused {
        return Ok(Started::Paused);
    }
    if let Some(c) = &f.concurrency {
        let active: i64 = db.query_row(
            "SELECT COUNT(*) FROM runs WHERE workflow = ?1 AND ended IS NULL",
            params![f.name],
            |r| r.get(0),
        )?;
        let waiting: i64 = db.query_row(
            "SELECT COUNT(*) FROM queued WHERE workflow = ?1",
            params![f.name],
            |r| r.get(0),
        )?;
        if active >= i64::from(c.max) || waiting > 0 {
            return match c.then {
                Then::Skip => Ok(Started::Skipped),
                Then::Queue => {
                    db.execute(
                        "INSERT INTO queued (workflow, args, input, at) VALUES (?1, ?2, ?3, ?4)",
                        params![f.name, json(args)?, json(input)?, now],
                    )?;
                    Ok(Started::Queued)
                }
            };
        }
    }
    Ok(Started::Run(insert_run(db, f, args, input, now)?))
}
```

Import `crate::workflow::format::Then`.

4. `start_run` and `start_runs_for_event` call `admit` instead of
`insert_run`, and return `Started` / `Vec<Started>`. The note of
`start_runs_for_event`: "no workflow starts on it" when `starts` is empty;
otherwise, when no start became a run, the outcomes joined, such as
`"mail-check: queued"` or `"mail-check: paused"`:

```rust
        let mut outcomes = vec![];
        for (f, args) in starts {
            outcomes.push(admit(&tx, f, args, input, at)?);
        }
        let note = if starts.is_empty() {
            Some("no workflow starts on it".to_string())
        } else if outcomes.iter().any(|o| matches!(o, Started::Run(_))) {
            None
        } else {
            Some(
                starts
                    .iter()
                    .zip(&outcomes)
                    .map(|((f, _), o)| format!("{}: {}", f.name, outcome_word(o)))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
```

with

```rust
fn outcome_word(o: &Started) -> &'static str {
    match o {
        Started::Run(_) => "started",
        Started::Queued => "queued",
        Started::Skipped => "skipped",
        Started::Paused => "paused",
    }
}
```

5. The queue, pauses, the breaker's count and cursors, in `impl Journal`:

```rust
    /// Starts queued runs, oldest first, while their workflow has a free
    /// slot and is not paused; returns the runs started. A queued start of
    /// a workflow no longer in `flows` waits.
    pub fn release_queued(&mut self, flows: &[File], now: &str) -> Result<Vec<String>> {
        let mut started = vec![];
        for f in flows.iter().filter(|f| f.concurrency.is_some()) {
            loop {
                let tx = self.db.transaction()?;
                let next: Option<(i64, String, String)> = tx
                    .query_row(
                        "SELECT seq, args, input FROM queued WHERE workflow = ?1 ORDER BY seq LIMIT 1",
                        params![f.name],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                let Some((seq, args, input)) = next else { break };
                tx.execute("DELETE FROM queued WHERE seq = ?1", params![seq])?;
                let args: Map<String, Value> = serde_json::from_str(&args)?;
                let input: Value = serde_json::from_str(&input)?;
                // the queued start's own place: admit again with the queue
                // empty of it; queued again means still full
                match admit(&tx, f, &args, &input, now)? {
                    Started::Run(id) => {
                        tx.commit()?;
                        started.push(id);
                    }
                    _ => break, // dropped: the transaction rolls back
                }
            }
        }
        Ok(started)
    }
```

`admit` sees `waiting > 0` only for other queued starts; when this start
is still blocked it queues itself again at the back, so the `break`
without commit keeps it at the front instead (the rollback undoes both the
delete and the new insert).

```rust
    /// The paused workflows.
    pub fn paused(&self) -> Result<Vec<Paused>> {
        let mut s = self.db.prepare("SELECT workflow, pin, at, why FROM paused ORDER BY workflow")?;
        let out = s
            .query_map([], |r| {
                Ok(Paused { workflow: r.get(0)?, pin: r.get(1)?, at: r.get(2)?, why: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Lets a paused workflow start runs again; false when it was not paused.
    pub fn resume(&self, workflow: &str) -> Result<bool> {
        Ok(self.db.execute("DELETE FROM paused WHERE workflow = ?1", params![workflow])? > 0)
    }

    /// How many of a workflow's newest ended runs failed, in a row.
    pub fn failed_in_a_row(&self, workflow: &str) -> Result<u32> {
        let mut s = self.db.prepare(
            "SELECT json_extract(runtime, '$.state') FROM runs WHERE workflow = ?1 AND ended IS NOT NULL ORDER BY ended DESC, id DESC",
        )?;
        let states = s.query_map(params![workflow], |r| r.get::<_, String>(0))?;
        let mut n = 0;
        for st in states {
            if st? != crate::workflow::FAILED {
                break;
            }
            n += 1;
        }
        Ok(n)
    }

    /// Where a source stopped, as it said last.
    pub fn cursor(&self, source: &str) -> Result<Option<Value>> {
        let t: Option<String> = self
            .db
            .query_row("SELECT value FROM cursors WHERE source = ?1", params![source], |r| r.get(0))
            .optional()?;
        t.map(|t| Ok(serde_json::from_str(&t)?)).transpose()
    }

    pub fn set_cursor(&self, source: &str, value: &Value) -> Result<()> {
        self.db.execute(
            "INSERT INTO cursors (source, value) VALUES (?1, ?2) ON CONFLICT (source) DO UPDATE SET value = ?2",
            params![source, json(value)?],
        )?;
        Ok(())
    }
```

6. In `handle`, before `tx.commit()`:

```rust
        if let Some(p) = &h.pause {
            tx.execute(
                "INSERT INTO paused (workflow, pin, at, why) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (workflow) DO UPDATE SET pin = ?2, at = ?3, why = ?4",
                params![p.workflow, p.pin, now, p.why],
            )?;
        }
```

7. Every `Handled { ... }` literal in `src/engine/daemon.rs` (three of
them: `give_up`, `deliver`, `save_result`) gets `pause: None`; Task 4
fills the one in `deliver`. Callers of `start_run` and
`start_runs_for_event` in `daemon.rs` compile again in Task 3; for now
make them ignore the new return values (`let _ = ...` is wrong for a
`Result`: keep the `?`, drop the value).

- [ ] **Step 4: Run the tests**

Run: `cargo test --test journal && make test`
Expected: PASS. A journal made by the previous itero opens and migrates
(the existing journal test that opens an old schema covers it; if there is
none, Step 1 of Task 3 adds one).

- [ ] **Step 5: Commit**

```bash
git add src/engine/journal.rs src/engine/daemon.rs tests/journal.rs
git commit -m "The journal queues starts, pauses workflows and keeps sources' cursors"
```

---

### Task 3: The daemon admits starts, releases the queue, and resumes new versions

**Files:**
- Modify: `src/engine/daemon.rs` (`run_once`, `fire_schedules`, `start_triggered`)
- Test: `tests/daemon.rs`

**Interfaces:**
- Consumes: `Started`, `release_queued`, `paused`, `resume` (Task 2).
- Produces: nothing new for later tasks; behaviour only.

- [ ] **Step 1: Write the failing tests**

Append to `tests/daemon.rs`:

```rust
/// A workflow that runs a slow shell line, at most one at a time.
fn yavas(then: &str) -> String {
    format!(
        r#"{{ "name": "yavas", "triggers": [ {{ "on": "dosya.geldi" }} ],
             "concurrency": {{ "max": 1, "then": "{then}" }},
             "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ {{ "from": "basla", "to": "bitti",
               "do": [ {{ "action": "sh@1", "with": {{ "run": "true" }} }} ] }} ] }}"#
    )
}

fn emit(ws: &itero::workspace::Workspace, kind: &str) {
    itero::engine::events::emit(ws, kind, Some(serde_json::json!({"ad": "ağaç.txt"})), None, "2026-10-16T10:00:00Z").unwrap();
}

#[test]
fn starts_beyond_the_maximum_wait_and_run_in_turn() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    emit(&ws, "dosya.geldi");
    emit(&ws, "dosya.geldi");
    // one pass: the first run starts and finishes; the second was queued
    // while the first was under way, and starts on the next pass
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(runs.iter().all(|r| r.runtime.state == "bitti"), "{runs:?}");
}

#[test]
fn starts_beyond_the_maximum_are_dropped_when_the_workflow_skips() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("skip"));
    emit(&ws, "dosya.geldi");
    emit(&ws, "dosya.geldi");
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.runs().unwrap().len(), 1);
    let notes: Vec<_> = j.events(Some("itero.app")).unwrap().into_iter().filter_map(|e| e.note).collect();
    assert_eq!(notes, ["yavas: skipped"]);
}

#[test]
fn a_new_version_of_a_paused_workflow_resumes_it() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    let mut j = Journal::open(&ws).unwrap();
    j.handle(itero::engine::journal::Handled {
        seq: 0, at: "2026-10-16T10:00:00Z", error: None, run: None, timers: None,
        events: vec![], memory: None,
        pause: Some(itero::engine::journal::Pause { workflow: "yavas", pin: "yavas@eski", why: "deneme".into() }),
    }).unwrap();
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    assert!(j.paused().unwrap().is_empty(), "the current version is not the one that failed");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test daemon starts_beyond a_new_version`
Expected: FAIL: two runs where one was wanted, or the pause is kept.

- [ ] **Step 3: Implement**

In `run_once`, after `fire_schedules(ws, &mut j, now)?;`:

```rust
    let flows: Vec<crate::workflow::File> = ws.workflows()?.into_iter().map(|(_, f)| f).collect();
    resume_new_versions(&j, &flows)?;
    for id in j.release_queued(&flows, &stamp(now))? {
        eprintln!("{id}: started from the queue");
    }
```

and the helper:

```rust
/// A paused workflow whose file is now another version starts again: the
/// fix is the new version.
fn resume_new_versions(j: &Journal, flows: &[crate::workflow::File]) -> Result<()> {
    for p in j.paused()? {
        let now_pin = flows.iter().find(|f| f.name == p.workflow).map(crate::workflow::pin);
        if now_pin.is_some_and(|pin| pin != p.pin) {
            j.resume(&p.workflow)?;
            eprintln!("{}: a new version; resumed", p.workflow);
        }
    }
    Ok(())
}
```

In `fire_schedules`, the `start_run` result names what happened:

```rust
            if super::schedule::due(schedule, last, now).map_err(|e| anyhow!(e))? {
                match j.start_run(&f, &t.args, &stamp(now), Some(&key))? {
                    journal::Started::Run(_) => {}
                    other => eprintln!("{}: schedule {schedule}: {other:?}", f.name),
                }
            }
```

In `start_triggered`, the result is `Vec<Started>`:

```rust
    let outcomes = j.start_runs_for_event(r.seq, &starts, &r.event.data, &stamp(now))?;
    if outcomes.is_empty() {
        eprintln!("{}: no workflow starts on it", r.event.kind);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test daemon && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/daemon.rs tests/daemon.rs
git commit -m "The daemon queues or skips starts beyond a workflow's maximum"
```

---

### Task 4: The breaker and `itero resume`

**Files:**
- Modify: `src/engine/daemon.rs` (`deliver`)
- Modify: `src/engine/events.rs` (`engine_own`)
- Modify: `src/engine/http.rs` (`workflows`)
- Modify: `src/cli.rs` (`Resume`)
- Test: `tests/breaker.rs` (new), `tests/http.rs`, `tests/events.rs`

**Interfaces:**
- Consumes: `Pause`, `failed_in_a_row`, `paused`, `resume` (Task 2); `File::pause_after_or_default` (Task 1).
- Produces: `pub fn engine_own(kind: &str) -> bool` in `itero::engine::events` (Task 9 uses it); the event type `workflow.paused` with data `{"workflow", "version", "failed", "why"}`.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/breaker.rs
use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use std::path::Path;

fn t(s: &str) -> DateTime<Utc> { s.parse().unwrap() }
fn exe() -> &'static Path { Path::new(env!("CARGO_BIN_EXE_itero")) }

fn workspace(d: &Path, flow: &str) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    std::fs::write(d.join("workflows/bozuk.json"), flow).unwrap();
    itero::workspace::find(d).unwrap()
}

const BOZUK: &str = r#"{ "name": "bozuk", "triggers": [ { "on": "dene.simdi" } ], "pause_after": 2,
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "action": "sh@1", "with": { "run": "echo çöktü >&2; exit 3" } } ] } ] }"#;

fn kick(ws: &itero::workspace::Workspace) {
    itero::engine::events::emit(ws, "dene.simdi", None, None, "2026-10-16T10:00:00Z").unwrap();
}
```

`dene.simdi` is not an event type (types are lowercase ASCII words); use
`"dene.simdi"` in both places, and keep `çöktü` in the shell line.

```rust
#[test]
fn a_workflow_pauses_after_its_failed_runs_in_a_row_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), BOZUK);
    for second in 1..=2 {
        kick(&ws);
        run_once(&ws, t(&format!("2026-10-16T10:00:0{second}Z")), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let p = j.paused().unwrap();
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].workflow, "bozuk");
    assert!(p[0].why.contains("2 failed runs in a row"), "{}", p[0].why);
    let said: Vec<_> = j.events(Some("itero.app")).unwrap().into_iter()
        .filter(|e| e.event.kind == "workflow.paused").collect();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].event.data["workflow"], "bozuk");
    assert_eq!(said[0].event.data["failed"], 2);
    // paused: a third event starts nothing
    kick(&ws);
    run_once(&ws, t("2026-10-16T10:00:03Z"), exe()).unwrap();
    assert_eq!(j.runs().unwrap().len(), 2);
}

#[test]
fn itero_resume_lets_it_start_again() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), BOZUK);
    for second in 1..=2 {
        kick(&ws);
        run_once(&ws, t(&format!("2026-10-16T10:00:0{second}Z")), exe()).unwrap();
    }
    let out = std::process::Command::new(exe()).args(["resume", "bozuk"]).current_dir(d.path()).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "bozuk: resumed");
    let again = std::process::Command::new(exe()).args(["resume", "bozuk"]).current_dir(d.path()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&again.stdout).trim(), "bozuk: not paused");
    assert!(Journal::open(&ws).unwrap().paused().unwrap().is_empty());
}

#[test]
fn a_success_between_failures_starts_the_count_again() {
    let d = tempfile::tempdir().unwrap();
    let flaky = BOZUK.replace("exit 3", "test -e işaret || { touch işaret; exit 3; }; rm işaret");
    let ws = workspace(d.path(), &flaky);
    for second in 1..=4 {
        kick(&ws);
        run_once(&ws, t(&format!("2026-10-16T10:00:0{second}Z")), exe()).unwrap();
    }
    // fail, succeed, fail, succeed: never 2 in a row
    assert!(Journal::open(&ws).unwrap().paused().unwrap().is_empty());
}
```

`sh@1` runs in the workspace folder, so `işaret` is a file there.

In `tests/events.rs`, add:

```rust
#[test]
fn the_engines_workflow_and_source_types_cannot_be_sent_from_outside() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.toml"), "").unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    for kind in ["workflow.paused", "source.paused"] {
        let e = itero::engine::events::emit(&ws, kind, None, None, "2026-10-16T10:00:00Z").unwrap_err();
        assert!(e.to_string().contains("the engine's own"), "{kind}: {e}");
    }
}
```

In `tests/http.rs`, add a test that `GET /workflows` has `"paused": null`
for a workflow that is not paused, and the pause (`{"at", "why", "pin"}`)
for one that is (pause it through `Journal::handle` with a `Pause`, as in
Task 2's test).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test breaker --test events --test http`
Expected: FAIL: nothing paused; `workflow.paused` is accepted from outside;
no `paused` key; no `resume` command.

- [ ] **Step 3: Implement**

`src/engine/events.rs`:

```rust
/// Types the engine sends itself: from outside they would forge a worker's
/// result, a timer, a run's start, a workflow's pause or a source's state.
pub fn engine_own(kind: &str) -> bool {
    ["action.", "timer.", "run.", "workflow.", "source."]
        .iter()
        .any(|p| kind.starts_with(p))
}
```

`src/engine/daemon.rs`, in `deliver`, before `j.handle(Handled { ... })`:

```rust
    let failed_now = ended && state == FAILED;
    let limit = workflow.pause_after_or_default();
    let in_a_row = if failed_now { j.failed_in_a_row(&run.workflow)? + 1 } else { 0 };
    let why = format!("{in_a_row} failed runs in a row");
    let mut events = events;
    let pause = (failed_now && limit > 0 && in_a_row >= limit).then(|| {
        eprintln!("{}: paused: {why}", run.workflow);
        events.push(Envelope::new(
            "daemon",
            "workflow.paused",
            &journal::subject("app"),
            &stamp(now),
            Some(r.event.id.clone()),
            json!({
                "workflow": run.workflow,
                "version": crate::workflow::short(crate::workflow::split_pin(&run.pin).1),
                "failed": in_a_row,
                "why": answer.runtime.failed.as_ref().map(|f| f.chain.clone()),
            }),
        ));
        journal::Pause { workflow: &run.workflow, pin: &run.pin, why: why.clone() }
    });
```

Move the existing `let state = ...; let ended = ...;` above this block (they
are computed from `answer` already), and pass `events` and `pause` into
`Handled`. `"why"` carries what the failed chain was; if `Failed` has an
error text field, use it instead (read `src/engine/mod.rs` `Failed`).

`src/engine/http.rs`, in `workflows`, open the journal and add `paused`:

```rust
        let j = Journal::open(ws).map_err(internal)?;
        let paused = j.paused().map_err(internal)?;
        ...
                    json!({
                        "name": f.name,
                        "version": crate::workflow::version(f),
                        "triggers": f.triggers,
                        "paused": paused.iter().find(|p| p.workflow == f.name),
                    })
```

`src/cli.rs`: a command

```rust
    /// Let a paused workflow start runs again
    Resume { workflow: String },
```

and in `dispatch`:

```rust
        Cmd::Resume { workflow } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let word = if j.resume(&workflow)? { "resumed" } else { "not paused" };
            println!("{workflow}: {word}");
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test breaker --test events --test http && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/daemon.rs src/engine/events.rs src/engine/http.rs src/cli.rs tests/breaker.rs tests/events.rs tests/http.rs
git commit -m "A workflow pauses after failed runs in a row; itero resume"
```

---

### Task 5: The watch interface and the debouncer

**Files:**
- Create: `src/watch/mod.rs`
- Modify: `src/lib.rs` (`pub mod watch;`)
- Test: in `src/watch/mod.rs` (`#[cfg(test)] mod tests`, the debouncer is pure)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "lowercase")] pub enum Kind { Created, Changed, Removed, Renamed, Rescan }`
  - `#[derive(Debug, Clone, PartialEq, Eq, Serialize)] pub struct Change { pub path: PathBuf, pub kind: Kind }`
  - `pub trait Backend: Send { fn wait(&mut self, timeout: Duration) -> std::io::Result<Vec<Change>>; }`
  - `pub fn open(path: &Path) -> std::io::Result<Box<dyn Backend>>`: the platform's backend for one file or folder (Tasks 6 and 7 fill it).
  - `pub struct Debouncer` with `new(quiet: Duration, max_wait: Duration)`, `add(&mut self, changes: Vec<Change>, now: Instant)`, `due(&self) -> Option<Instant>`, `take(&mut self, now: Instant) -> Vec<Change>`.
  - `pub fn run(backend: &mut dyn Backend, d: &mut Debouncer, emit: impl FnMut(Change) -> std::io::Result<()>) -> std::io::Result<()>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn c(p: &str, kind: Kind) -> Change {
        Change { path: PathBuf::from(p), kind }
    }

    #[test]
    fn changes_wait_for_quiet_then_come_once_per_path() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut d = Debouncer::new(ms(500), ms(5000));
        d.add(vec![c("/posta/kutu", Kind::Changed)], t0);
        d.add(vec![c("/posta/kutu", Kind::Changed)], t0 + ms(200));
        assert_eq!(d.due(), Some(t0 + ms(700)));
        assert!(d.take(t0 + ms(600)).is_empty(), "not quiet yet");
        assert_eq!(d.take(t0 + ms(700)), vec![c("/posta/kutu", Kind::Changed)]);
        assert_eq!(d.due(), None);
    }

    #[test]
    fn a_steady_stream_is_cut_at_max_wait() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut d = Debouncer::new(ms(500), ms(2000));
        for i in 0..10 {
            d.add(vec![c("/günlük.log", Kind::Changed)], t0 + ms(i * 300));
        }
        assert_eq!(d.due(), Some(t0 + ms(2000)));
        assert_eq!(d.take(t0 + ms(2000)).len(), 1);
    }

    #[test]
    fn kinds_merge_into_what_a_reader_needs_to_know() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
        let merged = |a: Kind, b: Kind| {
            let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
            d.add(vec![c("/a", a), c("/a", b)], t0);
            d.take(t0)[0].kind
        };
        assert_eq!(merged(Kind::Removed, Kind::Created), Kind::Changed, "replaced");
        assert_eq!(merged(Kind::Created, Kind::Changed), Kind::Created);
        assert_eq!(merged(Kind::Created, Kind::Removed), Kind::Removed);
        assert_eq!(merged(Kind::Changed, Kind::Rescan), Kind::Rescan);
        assert_eq!(merged(Kind::Rescan, Kind::Changed), Kind::Rescan);
        d.add(vec![c("/b", Kind::Changed), c("/a", Kind::Removed)], t0);
        let out = d.take(t0);
        assert_eq!(out.len(), 2, "one per path, in path order");
        assert_eq!(out[0].path, PathBuf::from("/a"));
    }

    struct Fake(std::collections::VecDeque<Vec<Change>>);
    impl Backend for Fake {
        fn wait(&mut self, _: Duration) -> std::io::Result<Vec<Change>> {
            self.0.pop_front().ok_or_else(|| std::io::Error::other("bitti"))
        }
    }

    #[test]
    fn run_emits_debounced_changes_and_stops_when_emit_fails() {
        let mut b = Fake(vec![vec![c("/ç", Kind::Changed)], vec![], vec![c("/ç", Kind::Changed)]].into());
        let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
        let mut seen = vec![];
        let end = run(&mut b, &mut d, |ch| { seen.push(ch); Ok(()) });
        assert!(end.is_err(), "the fake backend ends with an error");
        assert_eq!(seen.len(), 2);
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib watch`
Expected: compile error, module `watch` has nothing.

- [ ] **Step 3: Implement**

```rust
//! Watching files and folders with the OS's own events. A `Backend` turns
//! one system's raw events into `Change`s; kqueue (macOS) and inotify
//! (Linux) come first, and FSEvents, fanotify or a polling backend plug in
//! behind the same trait. A backend that may have missed something reports
//! `Rescan`, so the reader looks again instead of trusting silence.
//! Debouncing happens once, here, not in each backend.
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod kqueue;
#[cfg(target_os = "linux")]
mod inotify;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Created,
    Changed,
    Removed,
    Renamed,
    /// Something may have been missed: read the state again.
    Rescan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: PathBuf,
    pub kind: Kind,
}

/// One system's file events.
pub trait Backend: Send {
    /// Waits up to `timeout` for changes; none when it ran out.
    fn wait(&mut self, timeout: Duration) -> std::io::Result<Vec<Change>>;
}

/// This system's backend, watching one file or folder. A file that is not
/// there yet is watched for: its creation is a change.
pub fn open(path: &Path) -> std::io::Result<Box<dyn Backend>> {
    #[cfg(target_os = "macos")]
    return Ok(Box::new(kqueue::Kqueue::new(path)?));
    #[cfg(target_os = "linux")]
    return Ok(Box::new(inotify::Inotify::new(path)?));
    #[allow(unreachable_code)]
    Err(std::io::Error::other(format!(
        "watching {} needs macOS or Linux",
        path.display()
    )))
}

/// Two changes to one path, as one.
fn merge(old: Kind, new: Kind) -> Kind {
    use Kind::*;
    match (old, new) {
        (Rescan, _) | (_, Rescan) => Rescan,
        (Removed | Renamed, Created) => Changed,
        (Created, Changed) => Created,
        (_, k) => k,
    }
}

/// Holds changes until the path has been quiet for `quiet`, or the first of
/// them is `max_wait` old, then gives one change per path.
pub struct Debouncer {
    quiet: Duration,
    max_wait: Duration,
    pending: BTreeMap<PathBuf, Kind>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debouncer {
    pub fn new(quiet: Duration, max_wait: Duration) -> Debouncer {
        Debouncer { quiet, max_wait, pending: BTreeMap::new(), first: None, last: None }
    }

    pub fn add(&mut self, changes: Vec<Change>, now: Instant) {
        if changes.is_empty() {
            return;
        }
        for c in changes {
            let k = match self.pending.get(&c.path) {
                Some(old) => merge(*old, c.kind),
                None => c.kind,
            };
            self.pending.insert(c.path, k);
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// When the held changes are due, if any are held.
    pub fn due(&self) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet).min(first + self.max_wait))
    }

    /// The held changes, when due at `now`; else none.
    pub fn take(&mut self, now: Instant) -> Vec<Change> {
        match self.due() {
            Some(at) if now >= at => {
                self.first = None;
                self.last = None;
                std::mem::take(&mut self.pending)
                    .into_iter()
                    .map(|(path, kind)| Change { path, kind })
                    .collect()
            }
            _ => vec![],
        }
    }
}

/// Watches until the backend fails or `emit` does (its reader went away).
pub fn run(
    backend: &mut dyn Backend,
    d: &mut Debouncer,
    mut emit: impl FnMut(Change) -> std::io::Result<()>,
) -> std::io::Result<()> {
    loop {
        let now = Instant::now();
        let timeout = d.due().map_or(Duration::from_secs(3600), |at| at.saturating_duration_since(now));
        let changes = backend.wait(timeout)?;
        let now = Instant::now();
        d.add(changes, now);
        for c in d.take(now) {
            emit(c)?;
        }
    }
}
```

Until Tasks 6 and 7, give `kqueue.rs` and `inotify.rs` a stub so the crate
builds: `pub struct Kqueue; impl Kqueue { pub fn new(_: &Path) -> std::io::Result<Kqueue> { Err(std::io::Error::other("not yet")) } } impl super::Backend for Kqueue { fn wait(&mut self, _: Duration) -> std::io::Result<Vec<super::Change>> { Ok(vec![]) } }` (and the same for `Inotify`).

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib watch && make lint-check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/watch/mod.rs src/watch/kqueue.rs src/watch/inotify.rs src/lib.rs
git commit -m "Watching files: one interface for the OS's events, and a debouncer"
```

---

### Task 6: The kqueue backend (macOS)

**Files:**
- Modify: `src/watch/kqueue.rs`
- Test: `tests/watch.rs` (new; the same tests run on Linux in Task 7)

**Interfaces:**
- Consumes: `Backend`, `Change`, `Kind` (Task 5).
- Produces: `watch::open` works on macOS.

How it watches: the target and its parent folder. The target's own events
say a file changed, was removed or renamed; the parent's say an entry came
or went, which is how a file that is not there yet, or was replaced by an
atomic save (write a temporary file, rename it over), is found again. A
watch that had to be set again reports `Rescan` as well, since writes
between losing and finding it are unseen.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/watch.rs
#![cfg(any(target_os = "macos", target_os = "linux"))]
use itero::watch::{self, Kind};
use std::io::Write;
use std::time::{Duration, Instant};

/// Changes seen within `secs`, or until one of `kind` comes.
fn until(b: &mut Box<dyn watch::Backend>, kind: Kind, secs: u64) -> Vec<Kind> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut seen = vec![];
    while Instant::now() < end {
        for c in b.wait(Duration::from_millis(200)).unwrap() {
            seen.push(c.kind);
        }
        if seen.contains(&kind) {
            break;
        }
    }
    seen
}

fn append(p: &std::path::Path, text: &str) {
    std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap().write_all(text.as_bytes()).unwrap();
}

#[test]
fn an_append_is_a_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("posta kutusu");
    append(&f, "From ayşe\n");
    let mut b = watch::open(&f).unwrap();
    append(&f, "From can\n");
    assert!(until(&mut b, Kind::Changed, 3).contains(&Kind::Changed));
}

#[test]
fn a_file_that_comes_later_is_seen() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("henüz yok");
    let mut b = watch::open(&f).unwrap();
    append(&f, "merhaba\n");
    assert!(until(&mut b, Kind::Created, 3).contains(&Kind::Created));
    // and it is watched from then on
    append(&f, "tekrar\n");
    assert!(until(&mut b, Kind::Changed, 3).contains(&Kind::Changed));
}

#[test]
fn an_atomic_save_keeps_the_watch() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("ayarlar.json");
    append(&f, "{}");
    let mut b = watch::open(&f).unwrap();
    let tmp = d.path().join(".ayarlar.json.yeni");
    std::fs::write(&tmp, "{\"dil\": \"tr\"}").unwrap();
    std::fs::rename(&tmp, &f).unwrap();
    let seen = until(&mut b, Kind::Rescan, 3);
    assert!(!seen.is_empty(), "the replace is seen");
    // the new file is watched
    append(&f, " ");
    assert!(until(&mut b, Kind::Changed, 3).contains(&Kind::Changed));
}

#[test]
fn a_removal_is_seen() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("silinecek.txt");
    append(&f, "é");
    let mut b = watch::open(&f).unwrap();
    std::fs::remove_file(&f).unwrap();
    assert!(until(&mut b, Kind::Removed, 3).contains(&Kind::Removed));
}

#[test]
fn a_folder_reports_entries_coming_and_going() {
    let d = tempfile::tempdir().unwrap();
    let mut b = watch::open(d.path()).unwrap();
    append(&d.path().join("yeni dosya"), "ü");
    assert!(until(&mut b, Kind::Changed, 3).contains(&Kind::Changed));
}

#[test]
fn nothing_happening_is_no_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("sessiz");
    append(&f, "ı");
    let mut b = watch::open(&f).unwrap();
    assert!(b.wait(Duration::from_millis(300)).unwrap().is_empty());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test watch`
Expected: FAIL: `open` returns "not yet".

- [ ] **Step 3: Implement**

```rust
//! macOS: kqueue's vnode events on the target and on its parent folder.
use super::{Backend, Change, Kind};
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE_EVENTS: u32 = libc::NOTE_WRITE
    | libc::NOTE_EXTEND
    | libc::NOTE_ATTRIB
    | libc::NOTE_DELETE
    | libc::NOTE_RENAME
    | libc::NOTE_REVOKE;

pub struct Kqueue {
    kq: OwnedFd,
    path: PathBuf,
    /// The target's own watch: none while it is not there.
    target: Option<(OwnedFd, u64)>,
    /// The parent folder's watch.
    parent: OwnedFd,
}

fn open_evtonly(p: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(p.as_os_str().as_bytes())?;
    // SAFETY: a valid C string; O_EVTONLY opens for events only
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor this function owns
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn inode(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.ino())
}

fn register(kq: RawFd, fd: RawFd, fflags: u32) -> io::Result<()> {
    let ev = libc::kevent {
        ident: fd as libc::uintptr_t,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one valid kevent in, none out
    let n = unsafe { libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if n < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

impl Kqueue {
    pub fn new(path: &Path) -> io::Result<Kqueue> {
        // SAFETY: kqueue takes no arguments
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor this function owns
        let kq = unsafe { OwnedFd::from_raw_fd(kq) };
        let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let parent = open_evtonly(folder)?;
        register(kq.as_raw_fd(), parent.as_raw_fd(), libc::NOTE_WRITE)?;
        let mut k = Kqueue { kq, path: path.to_path_buf(), target: None, parent };
        k.watch_target()?;
        Ok(k)
    }

    /// Watches the target if it is there; true when a new watch was set.
    fn watch_target(&mut self) -> io::Result<bool> {
        let Some(ino) = inode(&self.path) else {
            self.target = None;
            return Ok(false);
        };
        if self.target.as_ref().is_some_and(|(_, i)| *i == ino) {
            return Ok(false);
        }
        let fd = match open_evtonly(&self.path) {
            Ok(fd) => fd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        register(self.kq.as_raw_fd(), fd.as_raw_fd(), FILE_EVENTS)?;
        self.target = Some((fd, ino));
        Ok(true)
    }
}

impl Backend for Kqueue {
    fn wait(&mut self, timeout: Duration) -> io::Result<Vec<Change>> {
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        // SAFETY: an all-zero kevent array is valid to be written into
        let mut evs: [libc::kevent; 16] = unsafe { std::mem::zeroed() };
        // SAFETY: room for 16 events, a valid timeout
        let n = unsafe {
            libc::kevent(self.kq.as_raw_fd(), std::ptr::null(), 0, evs.as_mut_ptr(), evs.len() as i32, &ts)
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(vec![]) } else { Err(e) };
        }
        let mut out = vec![];
        let change = |kind| Change { path: self.path.clone(), kind };
        let target_fd = self.target.as_ref().map(|(fd, _)| fd.as_raw_fd());
        let mut look_again = false;
        for ev in &evs[..n as usize] {
            let fd = ev.ident as RawFd;
            if Some(fd) == target_fd {
                let f = ev.fflags;
                if f & libc::NOTE_DELETE != 0 {
                    out.push(change(Kind::Removed));
                    look_again = true;
                } else if f & libc::NOTE_RENAME != 0 {
                    out.push(change(Kind::Renamed));
                    look_again = true;
                } else if f & libc::NOTE_REVOKE != 0 {
                    out.push(change(Kind::Rescan));
                    look_again = true;
                } else {
                    out.push(change(Kind::Changed));
                }
            } else {
                // an entry in the parent came or went
                look_again = true;
            }
        }
        if look_again {
            let had = self.target.is_some();
            let gone = inode(&self.path).is_none();
            if gone {
                if had {
                    out.push(change(Kind::Removed));
                }
                self.target = None;
            } else if self.watch_target()? {
                // a new watch: anything written before it was set is unseen
                out.push(change(if had { Kind::Rescan } else { Kind::Created }));
            } else if self.path.is_dir() {
                out.push(change(Kind::Changed));
            }
        }
        Ok(out)
    }
}
```

A watched folder: its own `NOTE_WRITE` (an entry came or went) arrives on
the target descriptor and is a `Changed`; the parent's events about other
entries cause a look that finds the same inode and adds nothing, except the
`is_dir` line which reports the folder as changed (harmless, debounced).
If `a_folder_reports_entries_coming_and_going` passes without that line,
remove it.

Note `self.target = None` when the target was removed: dropping the
`OwnedFd` closes it, and kqueue drops its registration with it.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test watch && make lint-check`
Expected: PASS on macOS.

- [ ] **Step 5: Commit**

```bash
git add src/watch/kqueue.rs tests/watch.rs
git commit -m "Watching on macOS with kqueue: the file and its folder"
```

---

### Task 7: The inotify backend (Linux)

**Files:**
- Modify: `src/watch/inotify.rs`
- Test: `tests/watch.rs` (unchanged; run in a Linux container)

**Interfaces:**
- Consumes: `Backend`, `Change`, `Kind` (Task 5).
- Produces: `watch::open` works on Linux.

- [ ] **Step 1: Run the tests on Linux to see them fail**

Docker Desktop's `desktop-linux` context (never colima):

```bash
docker --context desktop-linux run --rm -v "$PWD":/src -w /src \
  -e CARGO_TARGET_DIR=/src/target/linux rust:1.96.0 \
  cargo test --test watch
```

Expected: FAIL: `open` returns "not yet".

- [ ] **Step 2: Implement**

```rust
//! Linux: inotify on the target and on its parent folder.
use super::{Backend, Change, Kind};
use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TARGET_FILE: u32 = libc::IN_MODIFY | libc::IN_ATTRIB | libc::IN_CLOSE_WRITE | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
const TARGET_DIR: u32 = libc::IN_CREATE | libc::IN_DELETE | libc::IN_MODIFY | libc::IN_MOVED_FROM | libc::IN_MOVED_TO | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
const PARENT: u32 = libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO;

pub struct Inotify {
    fd: OwnedFd,
    path: PathBuf,
    name: Vec<u8>,
    target: Option<i32>,
    parent: i32,
}

fn add(fd: i32, p: &Path, mask: u32) -> io::Result<i32> {
    let c = CString::new(p.as_os_str().as_bytes())?;
    // SAFETY: a valid descriptor and C string
    let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask) };
    if wd < 0 { Err(io::Error::last_os_error()) } else { Ok(wd) }
}

impl Inotify {
    pub fn new(path: &Path) -> io::Result<Inotify> {
        // SAFETY: no pointers
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor this function owns
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let parent = add(fd.as_raw_fd(), folder, PARENT)?;
        let name = path.file_name().map(|n| n.as_bytes().to_vec()).unwrap_or_default();
        let mut i = Inotify { fd, path: path.to_path_buf(), name, target: None, parent };
        i.watch_target();
        Ok(i)
    }

    /// Watches the target if it is there; true when a watch was set.
    fn watch_target(&mut self) -> bool {
        let mask = if self.path.is_dir() { TARGET_DIR } else { TARGET_FILE };
        match add(self.fd.as_raw_fd(), &self.path, mask) {
            Ok(wd) => {
                let new = self.target != Some(wd);
                self.target = Some(wd);
                new
            }
            Err(_) => {
                self.target = None;
                false
            }
        }
    }
}

impl Backend for Inotify {
    fn wait(&mut self, timeout: Duration) -> io::Result<Vec<Change>> {
        let mut pfd = libc::pollfd { fd: self.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: one valid pollfd
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(vec![]) } else { Err(e) };
        }
        if n == 0 {
            return Ok(vec![]);
        }
        let mut buf = vec![0u8; 64 * 1024];
        // SAFETY: room for buf.len() bytes
        let len = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock { Ok(vec![]) } else { Err(e) };
        }
        let mut out = vec![];
        let change = |kind| Change { path: self.path.clone(), kind };
        let head = std::mem::size_of::<libc::inotify_event>();
        let mut at = 0usize;
        let mut look_again = false;
        while at + head <= len as usize {
            // SAFETY: the kernel wrote a whole inotify_event here
            let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf[at..].as_ptr().cast()) };
            let name_bytes = &buf[at + head..at + head + ev.len as usize];
            let name = name_bytes.split(|b| *b == 0).next().unwrap_or(&[]);
            at += head + ev.len as usize;
            if ev.mask & libc::IN_Q_OVERFLOW != 0 {
                out.push(change(Kind::Rescan));
                continue;
            }
            if Some(ev.wd) == self.target {
                if ev.mask & libc::IN_DELETE_SELF != 0 {
                    out.push(change(Kind::Removed));
                    self.target = None;
                } else if ev.mask & libc::IN_MOVE_SELF != 0 {
                    out.push(change(Kind::Renamed));
                    self.target = None;
                    look_again = true;
                } else if ev.mask & libc::IN_IGNORED != 0 {
                    self.target = None;
                    look_again = true;
                } else {
                    out.push(change(Kind::Changed));
                }
            } else if ev.wd == self.parent && OsStr::from_bytes(name) == OsStr::from_bytes(&self.name) {
                if ev.mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0 {
                    look_again = true;
                } else if ev.mask & (libc::IN_DELETE | libc::IN_MOVED_FROM) != 0 && self.target.is_some() {
                    out.push(change(Kind::Removed));
                }
            }
        }
        if look_again && self.path.exists() {
            let had = self.target.is_some();
            if self.watch_target() {
                out.push(change(if had || out.iter().any(|c| matches!(c.kind, Kind::Removed | Kind::Renamed)) { Kind::Rescan } else { Kind::Created }));
            }
        }
        Ok(out)
    }
}
```

`inotify_add_watch` on a path already watched returns the same watch
descriptor; a replaced file is a new inode and gets a new one.

- [ ] **Step 3: Run the tests on Linux and on macOS**

```bash
docker --context desktop-linux run --rm -v "$PWD":/src -w /src \
  -e CARGO_TARGET_DIR=/src/target/linux rust:1.96.0 \
  sh -c 'cargo test --test watch && cargo clippy --all-targets -- -D warnings'
cargo test --test watch
```

Expected: PASS in both.

- [ ] **Step 4: Commit**

```bash
git add src/watch/inotify.rs
git commit -m "Watching on Linux with inotify: the file and its folder"
```

---

### Task 8: `itero source watch@1`

**Files:**
- Modify: `src/cli.rs` (hidden `Source` command)
- Create: `src/sources/mod.rs` (the settings line and the event line, used here and by the supervisor)
- Modify: `src/lib.rs` (`pub mod sources;`)
- Test: `tests/sources.rs` (new)

**Interfaces:**
- Consumes: `watch::{open, Debouncer, run, Change}` (Tasks 5 to 7).
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct Settings { pub name: String, #[serde(default)] pub with: Map<String, Value>, #[serde(default)] pub emit: Option<String>, #[serde(default)] pub cursor: Option<Value> }` in `itero::sources`: the first line a source reads.
  - `pub fn watch_source(settings: &Settings, out: &mut dyn std::io::Write) -> anyhow::Result<()>` in `itero::sources`.

The protocol, written once in `src/sources/mod.rs`'s doc comment:

- A source reads one line of JSON on stdin when it starts: `{"name", "with", "emit", "cursor"}`.
- It prints one JSON object per line on stdout: an event, `{"type": "...", "data": {...}}` (`type` may be left out when its file sets `emit`), or a cursor, `{"cursor": <any JSON>}`.
- It exits when stdin closes: the daemon has gone.
- stderr is the daemon's log.

- [ ] **Step 1: Write the failing test**

```rust
// tests/sources.rs
#![cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn the_watch_source_prints_a_debounced_event_and_ends_with_its_stdin() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "From ayşe\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["source", "watch@1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let settings = serde_json::json!({
        "name": "kutu", "emit": "posta.degisti",
        "with": { "path": f, "quiet": "100ms" }
    });
    writeln!(stdin, "{settings}").unwrap();
    std::thread::sleep(Duration::from_millis(300)); // the watch is set
    for _ in 0..5 {
        std::fs::OpenOptions::new().append(true).open(&f).unwrap().write_all(b"From can\n").unwrap();
    }
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let line = lines.next().unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "posta.degisti");
    assert_eq!(v["data"]["kind"], "changed");
    assert_eq!(v["data"]["path"], f.to_str().unwrap());
    // five appends, one event
    drop(stdin);
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(Instant::now() < end, "still running after its stdin closed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(lines.next().is_none(), "one event for five appends");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test sources`
Expected: FAIL: no `source` command.

- [ ] **Step 3: Implement**

`src/sources/mod.rs` (Task 9 adds the file format to it):

```rust
//! Sources: long-lived programs that turn what happens outside into
//! events. The protocol (any language):
//!
//! - a source reads one JSON line on stdin when it starts: its `name`, its
//!   `with` (secrets filled in), its `emit` and its last `cursor`;
//! - it prints one JSON object per line on stdout: an event,
//!   `{"type": "...", "data": {...}}`, `type` left out when its file sets
//!   `emit`; or a cursor, `{"cursor": ...}`, given back on its next start;
//! - it exits when stdin closes, which means the daemon has gone;
//! - its stderr goes to the daemon's log.
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub mod supervise;

/// What a source reads first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub name: String,
    #[serde(default)]
    pub with: Map<String, Value>,
    #[serde(default)]
    pub emit: Option<String>,
    #[serde(default)]
    pub cursor: Option<Value>,
}

fn duration(with: &Map<String, Value>, key: &str, default: Duration) -> Result<Duration> {
    match with.get(key) {
        None => Ok(default),
        Some(Value::String(s)) => crate::workflow::expr::duration(s).map_err(|e| anyhow!("with.{key}: {e}")),
        Some(other) => Err(anyhow!("with.{key} is a duration like 500ms, not {other}")),
    }
}

/// `watch@1`: a file or folder's changes, debounced, as events of type
/// `emit` with data `{path, kind}`.
pub fn watch_source(s: &Settings, out: &mut dyn std::io::Write) -> Result<()> {
    let path = s.with.get("path").and_then(Value::as_str).ok_or_else(|| anyhow!("watch@1 needs with.path"))?;
    let emit = s.emit.as_deref().ok_or_else(|| anyhow!("watch@1 needs emit"))?;
    let quiet = duration(&s.with, "quiet", Duration::from_millis(500))?;
    let max_wait = duration(&s.with, "max_wait", Duration::from_secs(5))?;
    let mut backend = crate::watch::open(std::path::Path::new(path)).with_context(|| format!("cannot watch {path}"))?;
    let mut d = crate::watch::Debouncer::new(quiet, max_wait);
    crate::watch::run(backend.as_mut(), &mut d, |c| {
        writeln!(out, "{}", json!({ "type": emit, "data": c }))?;
        out.flush()
    })?;
    Ok(())
}
```

`humantime` reads `500ms`; check `expr::duration` accepts it.

`src/cli.rs`, a hidden command:

```rust
    /// A built-in source: its settings on stdin, its events on stdout
    #[command(hide = true)]
    Source { kind: String },
```

and in `dispatch`:

```rust
        Cmd::Source { kind } => {
            let mut first = String::new();
            std::io::stdin().read_line(&mut first)?;
            let s: crate::sources::Settings =
                serde_json::from_str(&first).map_err(|e| anyhow!("the source's settings: {e}"))?;
            // the daemon going away closes stdin: then this source stops
            std::thread::spawn(|| {
                let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
                std::process::exit(0);
            });
            let mut out = std::io::stdout().lock();
            match kind.as_str() {
                "watch@1" => crate::sources::watch_source(&s, &mut out)?,
                other => bail!("no built-in source {other}"),
            }
        }
```

Until Task 10, create `src/sources/supervise.rs` with a doc comment line
only, so `pub mod supervise;` builds.

- [ ] **Step 4: Run the test**

Run: `cargo test --test sources && make lint-check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sources/mod.rs src/sources/supervise.rs src/lib.rs src/cli.rs tests/sources.rs
git commit -m "itero source watch@1: a file's changes as events, one per line"
```

---

### Task 9: Sources in a workspace

**Files:**
- Modify: `src/sources/mod.rs` (the file format and its checks)
- Modify: `src/workspace.rs` (`sources` setting, `Workspace::sources()`)
- Modify: `src/cli.rs` (`itero check` lists sources)
- Test: `tests/workspace.rs`

**Interfaces:**
- Consumes: `engine_own` (Task 4).
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(deny_unknown_fields)] pub struct Definition { #[serde(default)] pub source: Option<String>, #[serde(default)] pub program: Vec<String>, #[serde(default)] pub with: Map<String, Value>, #[serde(default)] pub emit: Option<String> }`
  - `pub const BUILT_IN: [&str; 1] = ["watch@1"];`
  - `pub fn load_file(path: &Path) -> Result<Definition, crate::workflow::WorkflowError>`
  - `Workspace.sources: Vec<PathBuf>` and `Workspace::sources(&self) -> anyhow::Result<Vec<(String, PathBuf, Definition)>>`: name (the file's stem), the folder it is in, the definition.

- [ ] **Step 1: Write the failing tests**

Append to `tests/workspace.rs`:

```rust
#[test]
fn a_workspace_reads_its_sources() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(d.path(), "sources/posta.json",
        r#"{ "source": "watch@1", "with": { "path": "/var/mail/ayşe" }, "emit": "posta.degisti" }"#);
    write(d.path(), "sources/takvim.json",
        r#"{ "program": ["./takvim.sh", "--gün", "bugün"], "with": { "hesap": "iş" } }"#);
    let ws = workspace::find(d.path()).unwrap();
    let all = ws.sources().unwrap();
    let names: Vec<&str> = all.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, ["posta", "takvim"]);
    assert_eq!(all[1].2.program, ["./takvim.sh", "--gün", "bugün"]);
    assert_eq!(all[0].1, d.path().join("sources"));
}

#[test]
fn a_bad_source_file_is_an_error_naming_it() {
    for (text, says) in [
        (r#"{ "source": "watch@1", "emit": "a.b" }"#, "watch@1 needs with.path"),
        (r#"{ "source": "watch@1", "with": { "path": "/x" } }"#, "watch@1 needs emit"),
        (r#"{ "source": "izle@1", "with": {}, "emit": "a.b" }"#, "no built-in source izle@1"),
        (r#"{ "program": [], "emit": "a.b" }"#, "a source is a built-in (source) or a program"),
        (r#"{ "source": "watch@1", "program": ["x"], "with": { "path": "/x" }, "emit": "a.b" }"#, "a source is a built-in (source) or a program"),
        (r#"{ "program": ["x"], "emit": "workflow.paused" }"#, "the engine's own"),
        (r#"{ "program": ["x"], "emit": "Büyük.Harf" }"#, "an event type"),
        (r#"{ "program": ["x"], "renk": "mavi" }"#, "unknown field"),
    ] {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "itero.toml", "");
        write(d.path(), "sources/kotu.json", text);
        let e = workspace::find(d.path()).unwrap().sources().unwrap_err();
        let e = format!("{e:#}");
        assert!(e.contains("kotu.json") && e.contains(says), "{text}: {e}");
    }
}
```

A source's name is its file's stem and must be a lowercase ASCII word, so
the file is `kotu.json`; the Turkish letters are in the values.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test workspace`
Expected: compile error: no `sources`.

- [ ] **Step 3: Implement**

In `src/sources/mod.rs`:

```rust
/// The built-in sources, at their versions.
pub const BUILT_IN: [&str; 1] = ["watch@1"];

/// A source's file, `sources/<name>.json`: a built-in (`source`) or a
/// program and its arguments (`program`), its settings (`with`, where
/// `${env.NAME}` is a secret) and the event type it sends (`emit`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub program: Vec<String>,
    #[serde(default)]
    pub with: Map<String, Value>,
    #[serde(default)]
    pub emit: Option<String>,
}

fn check(d: &Definition) -> Result<(), String> {
    use crate::workflow::validate::is_word;
    match (&d.source, d.program.is_empty()) {
        (Some(_), true) | (None, false) => {}
        _ => return Err("a source is a built-in (source) or a program, one of them".into()),
    }
    if let Some(s) = &d.source {
        if !BUILT_IN.contains(&s.as_str()) {
            return Err(format!("no built-in source {s}"));
        }
        if !d.with.get("path").is_some_and(Value::is_string) {
            return Err("watch@1 needs with.path".into());
        }
        if d.emit.is_none() {
            return Err("watch@1 needs emit".into());
        }
    }
    if let Some(e) = &d.emit {
        if !e.split('.').all(is_word) {
            return Err(format!("emit \"{e}\" is an event type, like mail.changed"));
        }
        if crate::engine::events::engine_own(e) {
            return Err(format!("emit \"{e}\" is the engine's own"));
        }
    }
    for v in d.with.values().filter_map(Value::as_str) {
        crate::workflow::validate::check_templates(v)?;
    }
    Ok(())
}

/// Reads and checks a source's file; its name is the file's stem.
pub fn load_file(path: &std::path::Path) -> Result<Definition, crate::workflow::WorkflowError> {
    let err = |message: String| crate::workflow::WorkflowError { file: path.display().to_string(), at: None, message };
    let text = std::fs::read_to_string(path).map_err(|e| err(format!("cannot read it: {e}")))?;
    let d: Definition = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
    check(&d).map_err(err)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !crate::workflow::validate::is_word(stem) {
        return Err(err(format!("a source's name \"{stem}\" is lowercase letters, digits and -")));
    }
    Ok(d)
}
```

`src/workspace.rs`: a setting `sources: Vec<String>` with default
`vec!["sources".into()]`, a field `pub sources: Vec<PathBuf>` filled with
`folder(root, s)`, and

```rust
    /// Every source in the sources folders, checked: its name, its folder,
    /// its definition. A name found twice is an error naming both files.
    pub fn sources(&self) -> Result<Vec<(String, PathBuf, crate::sources::Definition)>> {
        let mut out: Vec<(String, PathBuf, crate::sources::Definition)> = vec![];
        for dir in &self.sources {
            if !dir.exists() {
                continue;
            }
            let files = listed(dir, |p| (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf()))?;
            for f in files {
                let d = crate::sources::load_file(&f)?;
                let name = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                if out.iter().any(|(n, _, _)| *n == name) {
                    bail!("two sources named {name}: the second is {}", f.display());
                }
                out.push((name, dir.clone(), d));
            }
        }
        Ok(out)
    }
```

`src/cli.rs`, in `Cmd::Check`, after the actions loop:

```rust
            for (name, _, d) in ws.sources()? {
                let what = d.source.clone().unwrap_or_else(|| d.program.join(" "));
                println!("source {name}  {what}");
            }
```

and the missing-folder note covers `ws.sources` too.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test workspace && make test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sources/mod.rs src/workspace.rs src/cli.rs tests/workspace.rs
git commit -m "A workspace's sources: sources/<name>.json, checked"
```

---

### Task 10: The supervisor

**Files:**
- Modify: `src/sources/supervise.rs`
- Modify: `src/engine/daemon.rs` (`Health.sources`, `SourceHealth`; `fill_secrets` made `pub(crate)`)
- Modify: `src/engine/http.rs` (`/health` shows sources)
- Test: `tests/sources.rs`, `tests/http.rs`

**Interfaces:**
- Consumes: `Settings`, `Definition` (Tasks 8, 9); `Journal::{publish, cursor, set_cursor}` (Task 2); `engine_own` (Task 4).
- Produces:
  - `pub struct Policy { pub first_pause: Duration, pub max_pause: Duration, pub calm_after: Duration, pub give_up_after: usize, pub window: Duration }` with `Default` (1 s, 60 s, 300 s, 10 restarts, 10 minutes).
  - `pub enum Line { Event { kind: String, data: Value }, Cursor(Value) }` and `pub fn parse_line(line: &str, emit: Option<&str>) -> Result<Line, String>`.
  - `pub fn run(ws: &Workspace, exe: &Path, name: &str, folder: &Path, def: &Definition, policy: &Policy, wake: &Sender<()>, health: &Mutex<Health>)`: runs the source until it gives up.
  - `#[derive(Debug, Clone, PartialEq, Serialize)] pub struct SourceHealth { pub state: String, pub restarts: usize, pub last_exit: Option<String> }` and `Health.sources: BTreeMap<String, SourceHealth>` in `itero::engine::daemon`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/sources.rs`:

```rust
use itero::engine::daemon::Health;
use itero::engine::journal::Journal;
use itero::sources::supervise::{self, Line, Policy};
use std::sync::Mutex;

fn quick() -> Policy {
    Policy {
        first_pause: Duration::from_millis(10),
        max_pause: Duration::from_millis(40),
        calm_after: Duration::from_secs(60),
        give_up_after: 3,
        window: Duration::from_secs(60),
    }
}

fn ws_with_source(d: &std::path::Path, script: &str, def: &str) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("sources")).unwrap();
    let s = d.join("sources/kaynak.sh");
    std::fs::write(&s, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(d.join("sources/deneme.json"), def).unwrap();
    itero::workspace::find(d).unwrap()
}

fn run_it(ws: &itero::workspace::Workspace) -> Health {
    let (name, folder, def) = ws.sources().unwrap().remove(0);
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-16T10:00:00Z"));
    supervise::run(ws, std::path::Path::new(env!("CARGO_BIN_EXE_itero")), &name, &folder, &def, &quick(), &wake, &health);
    health.into_inner().unwrap()
}

#[test]
fn lines_become_events_for_the_workspace_and_cursors_come_back() {
    let d = tempfile::tempdir().unwrap();
    // prints its settings back as an event, a cursor, then exits; the
    // supervisor restarts it until it gives up (3 restarts)
    let ws = ws_with_source(d.path(),
        "#!/bin/sh\nread -r settings\nprintf '{\"type\":\"deneme.basladi\",\"data\":{\"ayarlar\":%s}}\\n' \"$settings\"\nprintf '{\"cursor\":{\"sıra\":7}}\\n'\nprintf 'bozuk satır\\n'\nexit 1\n",
        r#"{ "program": ["./kaynak.sh"], "with": { "hesap": "iş" } }"#);
    let h = run_it(&ws);
    assert_eq!(h.sources["deneme"].state, "gave up");
    assert_eq!(h.sources["deneme"].restarts, 4);
    let j = Journal::open(&ws).unwrap();
    let evs: Vec<_> = j.events(Some("itero.app")).unwrap();
    assert_eq!(evs.len(), 4, "one event per start");
    assert_eq!(evs[0].event.source, "source/deneme");
    assert_eq!(evs[0].event.data["ayarlar"]["with"]["hesap"], "iş");
    assert!(evs[0].event.data["ayarlar"]["cursor"].is_null(), "no cursor at first");
    assert_eq!(evs[1].event.data["ayarlar"]["cursor"]["sıra"], 7, "the cursor given back");
    assert_eq!(j.cursor("deneme").unwrap().unwrap()["sıra"], 7);
}

#[test]
fn a_line_is_an_event_or_a_cursor_and_nothing_else() {
    assert_eq!(supervise::parse_line(r#"{"type":"a.b","data":{"x":1}}"#, None).unwrap(),
        Line::Event { kind: "a.b".into(), data: serde_json::json!({"x": 1}) });
    assert_eq!(supervise::parse_line(r#"{"data":{}}"#, Some("posta.geldi")).unwrap(),
        Line::Event { kind: "posta.geldi".into(), data: serde_json::json!({}) });
    assert_eq!(supervise::parse_line(r#"{"cursor":"ç"}"#, None).unwrap(), Line::Cursor(serde_json::json!("ç")));
    for (bad, says) in [
        ("düz yazı", "not JSON"),
        (r#"{"data":{}}"#, "no type"),
        (r#"{"type":"run.started"}"#, "the engine's own"),
        (r#"{"type":"a.b","data":[1]}"#, "data is an object"),
    ] {
        let e = supervise::parse_line(bad, None).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn a_secret_in_with_is_filled_in_for_the_source_only() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_source(d.path(),
        "#!/bin/sh\nread -r settings\nprintf '{\"type\":\"deneme.basladi\",\"data\":{\"ayarlar\":%s}}\\n' \"$settings\"\nexit 1\n",
        r#"{ "program": ["./kaynak.sh"], "with": { "parola": "${env.ITERO_TEST_PAROLA}" } }"#);
    // SAFETY: tests in this file do not read this variable elsewhere
    unsafe { std::env::set_var("ITERO_TEST_PAROLA", "gizli-şifre") };
    unsafe { std::env::set_var("ITERO_NO_SECRET_STORE", "1") };
    run_it(&ws);
    let j = Journal::open(&ws).unwrap();
    let ev = &j.events(Some("itero.app")).unwrap()[0];
    // the source got the secret; the event shows what the source printed,
    // which here echoes it: a real source never prints its settings back
    assert_eq!(ev.event.data["ayarlar"]["with"]["parola"], "gizli-şifre");
}
```

The third test shows the secret reached the source. Keep it, but name it
for what it proves, and make sure nothing else writes `with` to the
journal: the supervisor never logs or stores settings.

In `tests/http.rs`, a test that `/health` has `"sources": {}`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test sources --test http`
Expected: compile errors: no `supervise::run`, no `Health.sources`.

- [ ] **Step 3: Implement**

`src/engine/daemon.rs`:

```rust
/// How one source is doing.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SourceHealth {
    /// `running`, `waiting` (before a restart) or `gave up`.
    pub state: String,
    pub restarts: usize,
    /// How it ended last.
    pub last_exit: Option<String>,
}
```

`Health` gets `pub sources: std::collections::BTreeMap<String, SourceHealth>`,
empty in `Health::starting`. `/health` adds `"sources": h.sources`.
`fill_secrets` becomes `pub(crate)`.

`src/sources/supervise.rs`:

```rust
//! Runs one source for the daemon: starts it with its settings, turns its
//! lines into events for the workspace, keeps its cursor, and starts it
//! again when it exits, pausing longer each time; one that keeps exiting is
//! given up on, and the log and `/health` say so.
use super::{Definition, Settings};
use crate::engine::daemon::{Health, SourceHealth, stamp};
use crate::engine::journal::{self, Envelope, Journal};
use crate::workspace::Workspace;
use serde_json::{Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

pub struct Policy {
    pub first_pause: Duration,
    pub max_pause: Duration,
    /// A run this long resets the pause to `first_pause`.
    pub calm_after: Duration,
    /// More restarts than this within `window` and it is given up on.
    pub give_up_after: usize,
    pub window: Duration,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            first_pause: Duration::from_secs(1),
            max_pause: Duration::from_secs(60),
            calm_after: Duration::from_secs(300),
            give_up_after: 10,
            window: Duration::from_secs(600),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Event { kind: String, data: Value },
    Cursor(Value),
}

/// One line a source printed.
pub fn parse_line(line: &str, emit: Option<&str>) -> Result<Line, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not JSON ({e}): {line}"))?;
    let Value::Object(mut o) = v else {
        return Err(format!("not a JSON object: {line}"));
    };
    if !o.contains_key("type") && let Some(c) = o.remove("cursor") {
        return Ok(Line::Cursor(c));
    }
    let kind = match o.remove("type") {
        Some(Value::String(t)) => t,
        Some(other) => return Err(format!("type is text, not {other}")),
        None => emit.map(str::to_string).ok_or("no type, and its file sets no emit")?,
    };
    if !kind.split('.').all(crate::workflow::validate::is_word) {
        return Err(format!("\"{kind}\" is not an event type, like mail.changed"));
    }
    if crate::engine::events::engine_own(&kind) {
        return Err(format!("\"{kind}\" is the engine's own"));
    }
    let data = match o.remove("data") {
        None => Value::Object(Map::new()),
        Some(d @ Value::Object(_)) => d,
        Some(_) => return Err("data is an object".into()),
    };
    Ok(Line::Event { kind, data })
}

fn set(health: &Mutex<Health>, name: &str, f: impl FnOnce(&mut SourceHealth)) {
    let mut h = health.lock().unwrap_or_else(PoisonError::into_inner);
    let s = h.sources.entry(name.to_string()).or_insert(SourceHealth {
        state: "running".into(),
        restarts: 0,
        last_exit: None,
    });
    f(s);
}

/// The command that runs a source: `itero source <built-in>`, or its
/// program, a path with `/` being in its folder.
fn command(exe: &Path, folder: &Path, def: &Definition) -> Command {
    let mut c = match &def.source {
        Some(built_in) => {
            let mut c = Command::new(exe);
            c.args(["source", built_in]);
            c
        }
        None => {
            let p = &def.program[0];
            let mut c = Command::new(if p.contains('/') { folder.join(p) } else { p.into() });
            c.args(&def.program[1..]);
            c
        }
    };
    c.current_dir(folder).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    c
}

/// One start of the source, until it exits: how it ended.
fn once(ws: &Workspace, exe: &Path, name: &str, folder: &Path, def: &Definition, wake: &Sender<()>) -> anyhow::Result<String> {
    let j = Journal::open(ws)?;
    let with = crate::engine::daemon::fill_secrets(&Value::Object(def.with.clone()))
        .map_err(|e| anyhow::anyhow!(e))?;
    let settings = Settings {
        name: name.to_string(),
        with: with.as_object().cloned().unwrap_or_default(),
        emit: def.emit.clone(),
        cursor: j.cursor(name)?,
    };
    let mut child = command(exe, folder, def).spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    writeln!(stdin, "{}", serde_json::to_string(&settings)?)?;
    // stdin stays open while this runs: its closing tells the source the
    // daemon has gone
    let stdout = child.stdout.take().expect("piped");
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(&line, def.emit.as_deref()) {
            Ok(Line::Event { kind, data }) => {
                let e = Envelope::new(&format!("source/{name}"), &kind, &journal::subject("app"), &stamp(chrono::Utc::now()), None, data);
                j.publish(&e)?;
                let _ = wake.send(());
            }
            Ok(Line::Cursor(c)) => j.set_cursor(name, &c)?,
            Err(why) => eprintln!("source {name}: {why}"),
        }
    }
    drop(stdin);
    let status = child.wait()?;
    Ok(status.to_string())
}

/// Runs a source until it is given up on.
pub fn run(
    ws: &Workspace,
    exe: &Path,
    name: &str,
    folder: &Path,
    def: &Definition,
    policy: &Policy,
    wake: &Sender<()>,
    health: &Mutex<Health>,
) {
    let mut starts: Vec<Instant> = vec![];
    let mut pause = Duration::ZERO;
    loop {
        let began = Instant::now();
        set(health, name, |s| s.state = "running".into());
        let ended = match once(ws, exe, name, folder, def, wake) {
            Ok(status) => status,
            Err(e) => format!("error: {e:#}"),
        };
        let ran = began.elapsed();
        eprintln!("source {name}: ended ({ended}) after {}s", ran.as_secs());
        starts.retain(|t| t.elapsed() < policy.window);
        starts.push(Instant::now());
        let restarts = starts.len();
        if restarts > policy.give_up_after {
            eprintln!("source {name}: gave up after {restarts} restarts in {}s", policy.window.as_secs());
            set(health, name, |s| {
                s.state = "gave up".into();
                s.restarts = restarts;
                s.last_exit = Some(ended);
            });
            return;
        }
        pause = if ran >= policy.calm_after {
            policy.first_pause
        } else {
            (pause * 2).clamp(policy.first_pause, policy.max_pause)
        };
        set(health, name, |s| {
            s.state = "waiting".into();
            s.restarts = restarts;
            s.last_exit = Some(ended);
        });
        std::thread::sleep(pause);
    }
}
```

The `restarts` the test expects is 4: three restarts allowed, the fourth
end gives up. Match the numbers to the policy (`give_up_after: 3` means
the source is started four times); adjust the test or the comparison so
"more restarts than `give_up_after`" holds, and say so in `Policy`'s doc.

`#[allow(clippy::too_many_arguments)]` on `run` if clippy asks; better, pass
a small `Source<'a> { name, folder, def }` struct, and change the test's
call to match.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test sources --test http && make test && make lint-check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sources/supervise.rs src/engine/daemon.rs src/engine/http.rs tests/sources.rs tests/http.rs
git commit -m "The daemon's sources: supervised, their lines become events, cursors kept"
```

---

### Task 10b: IMAP IDLE, a push source in a container

Added on 2026-10-16 with the owner's mail-servers example (done before
Task 2: `examples/mail-servers`, container actions, `make test-docker`).

**Files:**
- Modify: `src/sources/mod.rs` (`Definition.image`: a program source may run in a container)
- Modify: `src/sources/supervise.rs` (`command`: `docker run --rm -i` when `image` is set, as the worker does for container actions; reuse `engine::worker::container_name` and a shared argument builder, moved to `engine::worker` as `pub fn container_args(name: &str, folder: &Path, image: &str, env: &[String], program: &[String]) -> Vec<String>`)
- Create: `examples/mail-servers/sources/imap-idle.json`, `examples/mail-servers/sources/idle.py`
- Modify: `examples/mail-servers/workflows/imap-check.json` (trigger `on: mail.changed` beside the 5-minute schedule, `concurrency` 1)
- Test: `tests/mail_servers.rs` (a daemon with the source: mail sent, notified within seconds)

**Interfaces:**
- Consumes: the supervisor (Task 10), `container_args` (refactored from Task "container actions").
- Produces: `Definition.image: Option<String>`; a source with `image` must have `program`.

- [ ] **Step 1: The source program.** `idle.py` (standard library only):
  read the settings line; connect as `check.py` does (same login rules);
  `SELECT` the mailbox; loop: send `IDLE`, read untagged lines until
  `* <n> EXISTS` or 25 minutes (RFC 2177 asks to renew within 29), send
  `DONE`, read the tagged `OK`; on `EXISTS` print
  `{"type": "mail.changed", "data": {"mailbox": ..., "exists": n}}` and
  flush. A server without the `IDLE` capability: print nothing and poll
  `NOOP` every 60 s for `EXISTS` instead. Exit when stdin closes (a
  thread reading stdin to its end calls `os._exit(0)`). Errors to stderr,
  exit 1: the supervisor restarts it.
- [ ] **Step 2: The source file.**

```json
{ "program": ["python3", "idle.py"], "image": "python:3.13-alpine",
  "with": { "host": "imap.example.com", "port": 993, "tls": true,
            "user": "me@example.com", "password": "${env.IMAP_PASSWORD}",
            "mailbox": "INBOX" },
  "emit": "mail.changed" }
```

- [ ] **Step 3: The test.** With GreenMail (it supports IDLE): start
  `itero daemon` on a free port in a copy of the example (host
  `host.docker.internal`), wait until `/health` shows the source
  `running`, send a mail over SMTP, and wait up to 10 s for
  `Mail\t1 new: ...` in the notification log; the 5-minute schedule never
  fires in that time, so the source did it. Then kill the daemon and
  assert no container labelled `dev.aegea.itero.source` is left after 5 s.
- [ ] **Step 4: Run** `make fmt lint-check test test-docker`.
- [ ] **Step 5: Commit** "IMAP IDLE: mail is notified as it arrives, from a source in a container".

---

### Task 11: `itero daemon` runs the workspace's sources

**Files:**
- Modify: `src/cli.rs` (`Cmd::Daemon`)
- Test: `tests/mail.rs` (Task 12's end-to-end test covers it)

**Interfaces:**
- Consumes: `Workspace::sources`, `supervise::run`, `Policy::default` (Tasks 9, 10).

- [ ] **Step 1: Implement**

In `Cmd::Daemon`, after `engine` is made and before the loop thread:

```rust
            if !once {
                for (name, folder, def) in ws.sources()? {
                    let (ws, exe, engine) = (ws.clone(), exe.clone(), engine.clone());
                    let wake = engine.wake.clone();
                    eprintln!("itero: source {name}");
                    std::thread::spawn(move || {
                        crate::sources::supervise::run(&ws, &exe, &name, &folder, &def,
                            &crate::sources::supervise::Policy::default(), &wake, &engine.health);
                    });
                }
            }
```

(`once` returns earlier, so the `if` is only a reminder; drop it if the
code above already returned.) A bad source file stops the daemon at start
with the file named, as a bad `itero.toml` does.

- [ ] **Step 2: Build and run the existing tests**

Run: `make test`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/cli.rs
git commit -m "itero daemon starts the workspace's sources"
```

---

### Task 12: v2 of the mail check, end to end

**Files:**
- Create: `examples/mail-check/sources/mailbox.json`
- Modify: `examples/mail-check/workflows/mail-check.json`, `examples/mail-check/itero.toml`
- Modify: `tests/mail.rs`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: The example**

`examples/mail-check/sources/mailbox.json`:

```json
{ "source": "watch@1", "with": { "path": "/var/mail/${env.USER}" }, "emit": "mail.changed" }
```

`examples/mail-check/workflows/mail-check.json`:

```json
{
  "name": "mail-check",
  "description": "Notifies about mail not seen before, as it arrives.",
  "triggers": [
    { "on": "mail.changed", "args": { "mailbox": "" } },
    { "schedule": "0 * * * *", "args": { "mailbox": "" } }
  ],
  "concurrency": { "max": 1, "then": "queue" },
  "states": ["start", "done"],
  "start": "start",
  "end": ["done"],
  "transitions": [
    { "from": "start", "to": "done",
      "do": [
        { "id": "check", "action": "mail.check@1",
          "with": { "mailbox": "${args.mailbox}",
                    "seen": "${has(memory.seen) ? memory.seen : ''}" } },
        { "action": "notify@1", "when": "steps.check.json.new > 0",
          "with": { "title": "Mail", "text": "${steps.check.json.summary}" } },
        { "action": "memory.set@1", "with": { "seen": "${steps.check.json.seen}" } }
      ] }
  ]
}
```

`examples/mail-check/itero.toml`'s comment: "The mail check: a notification
for mail not seen before, as soon as it arrives (the mailbox is watched),
and an hourly check as a safety net. Run `itero daemon` in this folder."

- [ ] **Step 2: Adapt the v1 tests to the hourly schedule**

In `tests/mail.rs`, `example()` also rewrites the source's path:

```rust
    let src = ws.join("sources/mailbox.json");
    let text = std::fs::read_to_string(&src).unwrap().replace("/var/mail/${env.USER}", mbox.to_str().unwrap());
    std::fs::write(&src, text).unwrap();
```

The schedule tests move to hourly slots: the passes at `14:01` (seen),
`15:00:10` (first run), `16:00:10` (nothing new), then mail, `17:00:10`
(one new).

- [ ] **Step 3: Write the end-to-end test**

```rust
/// A free port on 127.0.0.1.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait_for(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

#[test]
fn mail_is_notified_as_it_arrives_and_never_twice() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
    std::fs::write(ws.join("itero.toml"), format!("port = {}\n", free_port())).unwrap();
    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("daemon")
        .current_dir(&ws)
        .env("ITERO_NOTIFY_LOG", &log)
        .env("ITERO_NO_SECRET_STORE", "1")
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1)); // the watch is set
    mail(&mbox, "ayse@örnek.test", "Fri Oct 16 10:00:00 2026", "Toplantı");
    assert!(wait_for(5, || shown(&log).contains("1 new: Toplantı")), "{}", shown(&log));
    mail(&mbox, "can@örnek.test", "Fri Oct 16 10:00:01 2026", "Çay");
    mail(&mbox, "elif@örnek.test", "Fri Oct 16 10:00:02 2026", "Kahve");
    let memory = || {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero")).args(["memory", "mail-check"]).current_dir(&ws).output().unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    assert!(wait_for(5, || memory().contains("elif@örnek.test")), "{}", memory());
    std::thread::sleep(std::time::Duration::from_secs(1));
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let all = shown(&log);
    // three mails, each counted once: "1 new" then "2 new" (or "1 new" twice)
    let counted: u32 = all.lines()
        .map(|l| l.split('\t').nth(1).unwrap().split(' ').next().unwrap().parse::<u32>().unwrap())
        .sum();
    assert_eq!(counted, 3, "{all}");
}
```

- [ ] **Step 4: Run it**

Run: `cargo test --test mail && make test`
Expected: PASS, the end-to-end test within a few seconds.

- [ ] **Step 5: Try it by hand (with the owner's yes, since it changes their machine's running service)**

```bash
make install
cp -R examples/mail-check/sources ~/itero/
cp examples/mail-check/workflows/mail-check.json ~/itero/workflows/
cd ~/itero && itero check && itero service install
echo "v2" | mail -s "itero v2" "$USER"
```

Expected: a notification within about a second.

- [ ] **Step 6: Commit**

```bash
git add examples/mail-check tests/mail.rs
git commit -m "The mail check, v2: notified as mail arrives"
```

---

### Task 13: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/design.md` (section 19: "Done in iteration 4", with what differs from the design)
- If the documentation site is on this branch (aegea-dev/itero#1 merged into `main`, then `main` merged here): create `docs/src/content/docs/guides/sources.md`; modify `guides/triggers.md`, `reference/workflow-file.md` (`concurrency`, `pause_after`), `reference/itero-toml.md` (`sources`), `reference/commands.md` (`itero resume`, sources in `itero check`), `reference/http.md` (`sources` in `/health`, `paused` in `/workflows`), `reference/glossary.md` (source, cursor, concurrency, breaker), `start/quickstart.md` (v2), and `astro.config.mjs` (the new guide).

- [ ] **Step 1: Write them**

The sources guide covers: what a source is, the three kinds (push, watch,
poll), `sources/<name>.json` with every key, `watch@1` and its `with`
(`path`, `quiet`, `max_wait`) and its events (`{path, kind}`, the kinds,
`rescan` meaning "read again"), program sources and the line protocol,
cursors, supervision (restarts, giving up, `/health`), secrets in `with`.

- [ ] **Step 2: Check them**

```bash
cd docs && yarn build && cd ..
grep -rn "$(printf '\342\200\224')" README.md docs/*.md docs/src/content/docs || echo "no em dash"
```

Expected: the build passes and no em dash is found.

- [ ] **Step 3: Commit**

```bash
git add README.md docs
git commit -m "Docs: sources, watching, concurrency and the breaker"
```

---

## Self-review notes

- Spec coverage: section 15's `concurrency` (Tasks 1 to 3), breaker and
  `itero resume` (Task 4), `workflow.paused` published (Task 4); section
  18's source protocol, supervision, cursors, `watch@1`, backends behind
  one interface, debouncing in one place, `rescan` (Tasks 5 to 10),
  `sources/` in a workspace (Task 9); section 19's v2 and its done-when
  list (Task 12; "a watcher that keeps dying is paused and logged" is Task
  10's give-up). Not in this iteration, by the design's order:
  `max_attempts`, `limits.run`, `itero errors`, `poll@1`, `hook@1`,
  connectors as packages.
- Types used across tasks: `Started`, `Pause`, `Paused` (Task 2),
  `engine_own` (Task 4), `Kind`, `Change`, `Backend`, `Debouncer` (Task 5),
  `Settings` (Task 8), `Definition` (Task 9), `Policy`, `Line`,
  `SourceHealth` (Task 10).
