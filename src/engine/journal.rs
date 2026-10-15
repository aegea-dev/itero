//! The engine's journal: one SQLite file per workspace,
//! `.itero/journal.sqlite`, local and never synced. It is the event queue
//! and log, and keeps each run's place, the workflow versions runs are
//! pinned to, timers, schedules, and each workflow's memory.
use super::{Runtime, Timer};
use crate::workflow::{self, File};
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The journal's schema version, kept in its `meta` table.
pub const SCHEMA_VERSION: i64 = 1;

/// Why a job's run was lost when nothing else says: the daemon stopped
/// while it ran.
const STOPPED: &str = "the daemon stopped while it ran";

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
  error TEXT,
  note TEXT
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
        if let Some(v) = version {
            let v: i64 = v
                .parse()
                .with_context(|| format!("the journal's schema version is not a number: {v}"))?;
            if v > SCHEMA_VERSION {
                bail!(
                    "{} was made by a newer itero (journal schema {v}; this itero knows up to {SCHEMA_VERSION}): update itero",
                    path.display()
                );
            }
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

    /// Starts a run of a workflow, pinned at its version: the run, and its
    /// `run.started` event, in one transaction. With a schedule's key, that
    /// schedule's last slot becomes `now` in the same transaction, so a slot
    /// never starts two runs. Returns the run's id.
    pub fn start_run(
        &mut self,
        f: &File,
        args: &Map<String, Value>,
        now: &str,
        schedule: Option<&str>,
    ) -> Result<String> {
        let tx = self.db.transaction()?;
        let id = insert_run(&tx, f, args, &Value::Object(Map::new()), now)?;
        if let Some(key) = schedule {
            set_schedule_last(&tx, key, now)?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Starts a run for each of `starts` on app event `seq`, the event's
    /// data their input, and marks the event handled: one transaction, so
    /// an event delivered again starts nothing twice. No starts is noted,
    /// not an error.
    pub fn start_runs_for_event(
        &mut self,
        seq: i64,
        starts: &[(&File, &Map<String, Value>)],
        input: &Value,
        at: &str,
    ) -> Result<Vec<String>> {
        let tx = self.db.transaction()?;
        let mut ids = vec![];
        for (f, args) in starts {
            ids.push(insert_run(&tx, f, args, input, at)?);
        }
        let note = starts.is_empty().then_some("no workflow starts on it");
        tx.execute(
            "UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL",
            params![seq, at, note],
        )?;
        tx.commit()?;
        Ok(ids)
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
        tx.commit()?;
        Ok(())
    }
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
