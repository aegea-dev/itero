//! The engine's journal: one SQLite file per workspace,
//! `.itero/journal.sqlite`, local and never synced. It is the event queue
//! and log, and keeps each run's place, the workflow versions runs are
//! pinned to, timers, schedules, and each workflow's memory.
use super::{Runtime, Timer};
use crate::workflow::format::Then;
use crate::workflow::{self, File};
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The journal's schema version, kept in its `meta` table.
pub const SCHEMA_VERSION: i64 = 1 + MIGRATIONS.len() as i64;

/// The steps from each version to the next: `MIGRATIONS[0]` takes version 1
/// to 2, and so on. A step is only ever added, never changed: a journal on
/// someone's machine was made by the steps as they were.
const MIGRATIONS: [&str; 2] = [
    // 2: what handling an event did, when no run shows it
    "ALTER TABLE events ADD COLUMN note TEXT;",
    // 3: starts waiting for a free slot, paused workflows, sources' cursors
    "CREATE TABLE queued (seq INTEGER PRIMARY KEY AUTOINCREMENT, workflow TEXT NOT NULL, args TEXT NOT NULL, input TEXT NOT NULL, at TEXT NOT NULL);
     CREATE TABLE paused (workflow TEXT PRIMARY KEY, pin TEXT NOT NULL, at TEXT NOT NULL, why TEXT NOT NULL);
     CREATE TABLE cursors (source TEXT PRIMARY KEY, value TEXT NOT NULL);",
];

/// Why a job's run was lost when nothing else says: the daemon stopped
/// while it ran.
const STOPPED: &str = "the daemon stopped while it ran";

/// Version 1's tables; `MIGRATIONS` take them to this version.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  time TEXT NOT NULL,
  type TEXT NOT NULL,
  source TEXT NOT NULL,
  subject TEXT NOT NULL,
  cause TEXT,
  data TEXT NOT NULL,
  handled TEXT,
  error TEXT
);
CREATE INDEX IF NOT EXISTS events_open ON events (handled, seq);
CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  workflow TEXT NOT NULL,
  pin TEXT NOT NULL,
  runtime TEXT NOT NULL,
  args TEXT NOT NULL,
  started TEXT NOT NULL,
  ended TEXT
);
CREATE TABLE IF NOT EXISTS workflows (pin TEXT PRIMARY KEY, definition TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS timers (id INTEGER PRIMARY KEY AUTOINCREMENT, run TEXT NOT NULL, at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs (seq INTEGER PRIMARY KEY, runs INTEGER NOT NULL, lost TEXT);
CREATE TABLE IF NOT EXISTS schedules (key TEXT PRIMARY KEY, last TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS memory (workflow TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workflow, key));
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

/// An event: the CloudEvents fields, and the event that led to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub subject: String,
    pub time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(default)]
    pub data: Value,
}

impl Envelope {
    /// A new event with a fresh UUID v7 id.
    pub fn new(
        source: &str,
        kind: &str,
        subject: &str,
        time: &str,
        cause: Option<String>,
        data: Value,
    ) -> Envelope {
        Envelope {
            id: uuid::Uuid::now_v7().to_string(),
            source: source.into(),
            kind: kind.into(),
            subject: subject.into(),
            time: time.into(),
            cause,
            data,
        }
    }
}

/// An event as the journal holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recorded {
    pub seq: i64,
    pub event: Envelope,
    pub handled: Option<String>,
    pub error: Option<String>,
    /// What handling it did, when it did nothing a run shows.
    pub note: Option<String>,
}

/// A run as the journal holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    pub id: String,
    pub workflow: String,
    pub pin: String,
    pub runtime: Runtime,
    pub args: Map<String, Value>,
    pub started: String,
    pub ended: Option<String>,
}

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

impl Started {
    fn word(&self) -> &'static str {
        match self {
            Started::Run(_) => "started",
            Started::Queued => "queued",
            Started::Skipped => "skipped",
            Started::Paused => "paused",
        }
    }
}

/// A paused workflow: the version that failed, when, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Paused {
    pub workflow: String,
    pub pin: String,
    pub at: String,
    pub why: String,
}

/// A workflow to pause, as part of an answer (the breaker).
pub struct Pause<'a> {
    pub workflow: &'a str,
    pub pin: &'a str,
    pub why: String,
}

/// A subject: `itero.run.<id>`, `itero.worker` for jobs, or `itero.app`
/// for an app's events (which may start runs).
pub fn subject(target: &str) -> String {
    match target {
        "worker" => "itero.worker".into(),
        "app" => "itero.app".into(),
        id => format!("itero.run.{id}"),
    }
}

/// The run a subject names.
pub fn run_of(subject: &str) -> Option<&str> {
    subject.strip_prefix("itero.run.")
}

/// What one answer changes, saved in one transaction.
pub struct Handled<'a> {
    /// The event answered.
    pub seq: i64,
    /// When: the daemon's pass time (RFC 3339), never the clock's.
    pub at: &'a str,
    pub error: Option<String>,
    /// The run's runtime state after it, and whether the run has ended.
    pub run: Option<(&'a str, &'a Runtime, bool)>,
    /// The run's timers from now on; `None` leaves them.
    pub timers: Option<&'a [Timer]>,
    pub events: Vec<Envelope>,
    /// Keys of a workflow's memory to set.
    pub memory: Option<(&'a str, &'a Map<String, Value>)>,
    /// A workflow to pause (the breaker), in the same transaction.
    pub pause: Option<Pause<'a>>,
}

pub struct Journal {
    db: Connection,
}

fn json<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Recorded> {
    let data: String = r.get(7)?;
    Ok(Recorded {
        seq: r.get(0)?,
        event: Envelope {
            id: r.get(1)?,
            time: r.get(2)?,
            kind: r.get(3)?,
            source: r.get(4)?,
            subject: r.get(5)?,
            cause: r.get(6)?,
            data: serde_json::from_str(&data).unwrap_or(Value::Null),
        },
        handled: r.get(8)?,
        error: r.get(9)?,
        note: r.get(10)?,
    })
}

const COLUMNS: &str = "seq, id, time, type, source, subject, cause, data, handled, error, note";

impl Journal {
    /// Opens (and creates) a workspace's journal; one from a newer itero is
    /// refused.
    pub fn open(ws: &Workspace) -> Result<Journal> {
        let path = ws.journal();
        std::fs::create_dir_all(path.parent().expect("a folder"))?;
        let mut db =
            Connection::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        // immediate: two processes opening at once make it one after the other
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        let version: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
                r.get(0)
            })
            .optional()?;
        // a journal being made has no version yet: it starts at 1
        let v: i64 = match version {
            None => 1,
            Some(v) => v
                .parse()
                .with_context(|| format!("the journal's schema version is not a number: {v}"))?,
        };
        if v > SCHEMA_VERSION {
            bail!(
                "{} was made by a newer itero (journal schema {v}; this itero knows up to {SCHEMA_VERSION}): update itero",
                path.display()
            );
        }
        if v < 1 {
            bail!("{}: journal schema {v} is not a version", path.display());
        }
        for step in &MIGRATIONS[(v - 1) as usize..] {
            tx.execute_batch(step)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        tx.commit()?;
        Ok(Journal { db })
    }

    /// Saves an event; an id already saved is kept as it was.
    pub fn publish(&self, e: &Envelope) -> Result<()> {
        publish(&self.db, e)
    }

    /// The oldest event not yet handled, apart from those for workers.
    pub fn next(&self) -> Result<Option<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND subject != 'itero.worker' ORDER BY seq LIMIT 1"
        );
        Ok(self.db.query_row(&sql, [], row).optional()?)
    }

    /// Jobs (events for a worker) not yet run, oldest first.
    pub fn jobs(&self) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND subject = 'itero.worker' ORDER BY seq"
        );
        let mut s = self.db.prepare(&sql)?;
        let out = s.query_map([], row)?.collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// A job's runs so far, and why the last was lost. A run the daemon
    /// stopped during counts as lost.
    pub fn job_runs(&self, seq: i64) -> Result<(i64, Option<String>)> {
        Ok(self
            .db
            .query_row(
                "SELECT runs, lost FROM jobs WHERE seq = ?1",
                params![seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, None)))
    }

    /// Counts a job's run as it starts: lost, until its result is saved.
    pub fn start_job(&self, seq: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO jobs (seq, runs, lost) VALUES (?1, 1, ?2)
             ON CONFLICT (seq) DO UPDATE SET runs = runs + 1, lost = ?2",
            params![seq, STOPPED],
        )?;
        Ok(())
    }

    /// Why a job's run was lost; the job stays waiting, to run again.
    pub fn lose_job(&self, seq: i64, why: &str) -> Result<()> {
        self.db.execute(
            "UPDATE jobs SET lost = ?2 WHERE seq = ?1",
            params![seq, why],
        )?;
        Ok(())
    }

    /// Every event, or those about one subject, in order.
    pub fn events(&self, subject: Option<&str>) -> Result<Vec<Recorded>> {
        let sql =
            format!("SELECT {COLUMNS} FROM events WHERE ?1 IS NULL OR subject = ?1 ORDER BY seq");
        let mut s = self.db.prepare(&sql)?;
        let out = s
            .query_map(params![subject], row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// A run by id.
    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        let r: Option<(String, String, String, String, String, Option<String>)> = self
            .db
            .query_row(
                "SELECT workflow, pin, runtime, args, started, ended FROM runs WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        r.map(|(workflow, pin, rt, args, started, ended)| {
            Ok(Run {
                id: id.to_string(),
                workflow,
                pin,
                runtime: serde_json::from_str(&rt)?,
                args: serde_json::from_str(&args)?,
                started,
                ended,
            })
        })
        .transpose()
    }

    /// Starts a run of a workflow, pinned at its version, unless it is
    /// paused or at its maximum (`admit`): the run, and its `run.started`
    /// event, in one transaction. With a schedule's key, that schedule's
    /// last slot becomes `now` in the same transaction, so a slot never
    /// starts two runs, whatever became of the start.
    pub fn start_run(
        &mut self,
        f: &File,
        args: &Map<String, Value>,
        now: &str,
        schedule: Option<&str>,
    ) -> Result<Started> {
        let tx = self.db.transaction()?;
        let started = admit(&tx, f, args, &Value::Object(Map::new()), now)?;
        if let Some(key) = schedule {
            set_schedule_last(&tx, key, now)?;
        }
        tx.commit()?;
        Ok(started)
    }

    /// Starts a run for each of `starts` on app event `seq`, the event's
    /// data their input, and marks the event handled: one transaction, so
    /// an event delivered again starts nothing twice. No starts, or none
    /// that became a run, is noted, not an error.
    pub fn start_runs_for_event(
        &mut self,
        seq: i64,
        starts: &[(&File, &Map<String, Value>)],
        input: &Value,
        at: &str,
    ) -> Result<Vec<Started>> {
        let tx = self.db.transaction()?;
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
                    .map(|((f, _), o)| format!("{}: {}", f.name, o.word()))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
        tx.execute(
            "UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL",
            params![seq, at, note],
        )?;
        tx.commit()?;
        Ok(outcomes)
    }

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
                let Some((seq, args, input)) = next else {
                    break;
                };
                tx.execute("DELETE FROM queued WHERE seq = ?1", params![seq])?;
                let args: Map<String, Value> = serde_json::from_str(&args)?;
                let input: Value = serde_json::from_str(&input)?;
                // still full or paused: the transaction is dropped, so the
                // start keeps its place at the front
                let Started::Run(id) = admit(&tx, f, &args, &input, now)? else {
                    break;
                };
                tx.commit()?;
                started.push(id);
            }
        }
        Ok(started)
    }

    /// The paused workflows.
    pub fn paused(&self) -> Result<Vec<Paused>> {
        let mut s = self
            .db
            .prepare("SELECT workflow, pin, at, why FROM paused ORDER BY workflow")?;
        let out = s
            .query_map([], |r| {
                Ok(Paused {
                    workflow: r.get(0)?,
                    pin: r.get(1)?,
                    at: r.get(2)?,
                    why: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Lets a paused workflow start runs again; false when it was not paused.
    pub fn resume(&self, workflow: &str) -> Result<bool> {
        Ok(self
            .db
            .execute("DELETE FROM paused WHERE workflow = ?1", params![workflow])?
            > 0)
    }

    /// How many of a workflow's newest ended runs failed, in a row.
    pub fn failed_in_a_row(&self, workflow: &str) -> Result<u32> {
        let mut s = self.db.prepare(
            "SELECT json_extract(runtime, '$.state') FROM runs WHERE workflow = ?1 AND ended IS NOT NULL ORDER BY ended DESC, id DESC",
        )?;
        let states = s.query_map(params![workflow], |r| r.get::<_, String>(0))?;
        let mut n = 0;
        for st in states {
            if st? != workflow::FAILED {
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
            .query_row(
                "SELECT value FROM cursors WHERE source = ?1",
                params![source],
                |r| r.get(0),
            )
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

    /// The newest `limit` runs, of one workflow or all, in one state or any,
    /// newest first.
    pub fn runs_page(
        &self,
        workflow: Option<&str>,
        state: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Run>> {
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

    /// Every run, oldest first.
    pub fn runs(&self) -> Result<Vec<Run>> {
        let mut s = self
            .db
            .prepare("SELECT id FROM runs ORDER BY started, id")?;
        let ids: Vec<String> = s
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = vec![];
        for id in ids {
            out.extend(self.run(&id)?);
        }
        Ok(out)
    }

    /// When a schedule (`<workflow>#<trigger>`) last started a run, or was
    /// first seen.
    pub fn schedule_last(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT last FROM schedules WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_schedule_last(&self, key: &str, at: &str) -> Result<()> {
        set_schedule_last(&self.db, key, at)
    }

    /// A workflow's memory: every key its runs have set.
    pub fn memory(&self, workflow: &str) -> Result<Map<String, Value>> {
        let mut s = self
            .db
            .prepare("SELECT key, value FROM memory WHERE workflow = ?1 ORDER BY key")?;
        let rows = s.query_map(params![workflow], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Map::new();
        for r in rows {
            let (k, v) = r?;
            out.insert(k, serde_json::from_str(&v)?);
        }
        Ok(out)
    }

    /// The definition of a pinned workflow version.
    pub fn workflow(&self, pin: &str) -> Result<Option<File>> {
        let t: Option<String> = self
            .db
            .query_row(
                "SELECT definition FROM workflows WHERE pin = ?1",
                params![pin],
                |r| r.get(0),
            )
            .optional()?;
        t.map(|t| Ok(serde_json::from_str(&t)?)).transpose()
    }

    /// Timers due at `now` (RFC 3339, UTC): id, run, data.
    pub fn due_timers(&self, now: &str) -> Result<Vec<(i64, String, Value)>> {
        let mut s = self
            .db
            .prepare("SELECT id, run, data FROM timers WHERE at <= ?1 ORDER BY at, id")?;
        let out = s
            .query_map(params![now], |r| {
                let d: String = r.get(2)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    serde_json::from_str(&d).unwrap_or(Value::Null),
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Turns a due timer into its event, in one transaction.
    pub fn fire_timer(&mut self, id: i64, e: &Envelope) -> Result<()> {
        let tx = self.db.transaction()?;
        publish(&tx, e)?;
        tx.execute("DELETE FROM timers WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Saves an answer: all of it, or (on any error) none of it.
    pub fn handle(&mut self, h: Handled) -> Result<()> {
        let now = h.at;
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE events SET handled = ?2, error = ?3 WHERE seq = ?1",
            params![h.seq, now, h.error],
        )?;
        if let Some((id, rt, ended)) = h.run {
            tx.execute(
                "UPDATE runs SET runtime = ?2, ended = CASE WHEN ?3 THEN COALESCE(ended, ?4) ELSE NULL END WHERE id = ?1",
                params![id, json(rt)?, ended, now],
            )?;
            if let Some(timers) = h.timers {
                tx.execute("DELETE FROM timers WHERE run = ?1", params![id])?;
                for t in timers {
                    tx.execute(
                        "INSERT INTO timers (run, at, data) VALUES (?1, ?2, ?3)",
                        params![id, t.at, json(&t.data)?],
                    )?;
                }
            }
        }
        for e in &h.events {
            publish(&tx, e)?;
        }
        if let Some((workflow, m)) = h.memory {
            for (k, v) in m {
                tx.execute(
                    "INSERT INTO memory (workflow, key, value) VALUES (?1, ?2, ?3)
                     ON CONFLICT (workflow, key) DO UPDATE SET value = ?3",
                    params![workflow, k, json(v)?],
                )?;
            }
        }
        if let Some(p) = &h.pause {
            tx.execute(
                "INSERT INTO paused (workflow, pin, at, why) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (workflow) DO UPDATE SET pin = ?2, at = ?3, why = ?4",
                params![p.workflow, p.pin, now, p.why],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

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
    let paused = db
        .query_row(
            "SELECT 1 FROM paused WHERE workflow = ?1",
            params![f.name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
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

/// A new run of `f`, pinned at its version, and its `run.started` event
/// carrying `input`; returns its id.
fn insert_run(
    db: &Connection,
    f: &File,
    args: &Map<String, Value>,
    input: &Value,
    now: &str,
) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let pin = workflow::pin(f);
    let rt = Runtime {
        state: f.start.clone(),
        chain: None,
        failed: None,
    };
    db.execute(
        "INSERT OR IGNORE INTO workflows (pin, definition) VALUES (?1, ?2)",
        params![pin, json(f)?],
    )?;
    db.execute(
        "INSERT INTO runs (id, workflow, pin, runtime, args, started) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, f.name, pin, json(&rt)?, json(args)?, now],
    )?;
    let e = Envelope::new(
        "daemon",
        "run.started",
        &subject(&id),
        now,
        None,
        input.clone(),
    );
    publish(db, &e)?;
    Ok(id)
}

fn set_schedule_last(db: &Connection, key: &str, at: &str) -> Result<()> {
    db.execute(
        "INSERT INTO schedules (key, last) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET last = ?2",
        params![key, at],
    )?;
    Ok(())
}

fn publish(db: &Connection, e: &Envelope) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO events (id, time, type, source, subject, cause, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![e.id, e.time, e.kind, e.source, e.subject, e.cause, json(&e.data)?],
    )?;
    Ok(())
}
