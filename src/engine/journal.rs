//! The engine's journal: one SQLite file per workspace,
//! `.itero/journal.sqlite`, local and never synced. It is the message queue
//! and log, and keeps each run's place, the workflow versions runs are
//! pinned to, timers and schedules.
use super::Timer;
use super::steps::Runtime;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The journal's schema version, kept in its `meta` table.
pub const SCHEMA_VERSION: i64 = OLDEST + MIGRATIONS.len() as i64;

/// The oldest schema a journal may have, from itero 0.4.0 on. An older one
/// is refused: nobody depended on itero then.
const OLDEST: i64 = 7;

/// The steps from each version to the next: `MIGRATIONS[0]` takes `OLDEST`
/// to the one after, and so on. A step is only ever added, never changed:
/// a journal on someone's machine was made by the steps as they were.
const MIGRATIONS: [&str; 2] = [
    // 8: the tables nothing reads any more
    "DROP TABLE IF EXISTS queued; DROP TABLE IF EXISTS paused;
     DROP TABLE IF EXISTS cursors; DROP TABLE IF EXISTS memory;",
    // 9: callbacks are gone; a workflow posts its result itself
    "DROP TABLE IF EXISTS replies;
     ALTER TABLE events DROP COLUMN reply; ALTER TABLE runs DROP COLUMN reply;",
];

/// Why a job's run was lost when nothing else says: the daemon stopped
/// while it ran.
const STOPPED: &str = "the daemon stopped while it ran";

/// The note on a job whose step was stopped (its group ended without it):
/// handled, never run again, and its answer, if any, not saved.
pub const JOB_STOPPED: &str = "stopped: its group ended without it";

/// The tables at `SCHEMA_VERSION`: a new journal is made with them.
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
  ended TEXT,
  input TEXT,
  name TEXT,
  message TEXT
);
CREATE TABLE IF NOT EXISTS workflows (pin TEXT PRIMARY KEY, definition TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS workflow_ids (name TEXT PRIMARY KEY, id TEXT NOT NULL, created TEXT NOT NULL, itero TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS timers (id INTEGER PRIMARY KEY AUTOINCREMENT, run TEXT NOT NULL, at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs (seq INTEGER PRIMARY KEY, runs INTEGER NOT NULL, lost TEXT);
CREATE TABLE IF NOT EXISTS schedules (key TEXT PRIMARY KEY, last TEXT NOT NULL);
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
    /// Two random words, to say aloud and type.
    pub name: String,
    pub workflow: String,
    /// The version of its workflows it is pinned to: a UUIDv7.
    pub pin: String,
    pub runtime: Runtime,
    /// The data of the message that started it.
    pub input: Value,
    pub started: String,
    pub ended: Option<String>,
    /// The message that started it, or the run it split from.
    pub message: Option<String>,
}

/// A run's answer: what a waiting app or a callback gets when it ends.
pub fn answer(run: &Run, rt: &Runtime) -> Value {
    json!({
        "run": run.id,
        "name": run.name,
        "workflow": run.workflow,
        "status": super::steps::status(rt),
        "state": rt.state,
    })
}

/// What itero gives a workspace or a workflow, and nobody edits: an id, a
/// name (two random words, or a workflow's file name), when it was made,
/// and the itero that made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub created: String,
    pub itero: String,
}

/// This itero's version.
pub const ITERO: &str = env!("CARGO_PKG_VERSION");

/// Flags: `itero.log.full` was sent and the log not yet rolled.
pub const LOG_FULL: &str = "flag.log_full";
/// `itero.space.full` was sent and there is no room yet.
pub const SPACE_FULL: &str = "flag.space_full";

/// A run to start: its workflow, the workflows pinned with it (its own and
/// those it uses as steps), the message's data, and the message.
pub struct Start {
    pub workflow: String,
    pub flows: BTreeMap<String, Value>,
    pub input: Value,
    pub cause: Option<String>,
}

impl Start {
    fn new_run<'a>(&'a self, pin: &'a str, rt: &'a Runtime, now: &'a str) -> NewRun<'a> {
        NewRun {
            workflow: &self.workflow,
            pin,
            rt,
            input: &self.input,
            now,
            cause: self.cause.as_deref(),
            message: self.cause.as_deref(),
        }
    }
}

/// A run split from another: its own copy of the runtime, from the same
/// place.
pub struct Split<'a> {
    pub workflow: &'a str,
    pub pin: &'a str,
    pub runtime: &'a Runtime,
    pub input: &'a Value,
    /// The message of the run it splits from.
    pub message: Option<&'a str>,
}

/// A subject: `itero.run.<id>`, `itero.worker` for jobs, or `itero.app`
/// for messages for the workspace (which may start runs).
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
    /// The run's runtime after it, and whether the run has ended.
    pub run: Option<(&'a str, &'a Runtime, bool)>,
    /// The run's timers from now on; `None` leaves them.
    pub timers: Option<&'a [Timer]>,
    pub events: Vec<Envelope>,
    /// Runs split from this one.
    pub splits: Vec<Split<'a>>,
    /// Jobs (by `seq`) whose steps this answer stopped: handled, noted
    /// `JOB_STOPPED`, so none runs again.
    pub stopped: Vec<i64>,
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
        let has_meta: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
            [],
            |r| r.get(0),
        )?;
        let version: Option<String> = if has_meta {
            tx.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| {
                r.get(0)
            })
            .optional()?
        } else {
            None
        };
        match version {
            // a journal being made: the tables as they are now
            None => tx.execute_batch(SCHEMA)?,
            Some(v) => {
                let v: i64 = v.parse().with_context(|| {
                    format!("the journal's schema version is not a number: {v}")
                })?;
                if v > SCHEMA_VERSION {
                    bail!(
                        "{} was made by a newer itero (journal schema {v}; this itero knows up to {SCHEMA_VERSION}): update itero",
                        path.display()
                    );
                }
                if v < OLDEST {
                    bail!(
                        "{} was made by an itero before 0.4.0 (journal schema {v}): remove .itero/ to start again; its runs and history go with it",
                        path.display()
                    );
                }
                for step in &MIGRATIONS[(v - OLDEST) as usize..] {
                    tx.execute_batch(step)?;
                }
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        let workspace = Identity {
            id: uuid::Uuid::now_v7().to_string(),
            name: crate::names::random(),
            created: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            itero: ITERO.into(),
        };
        tx.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('workspace', ?1)",
            params![json(&workspace)?],
        )?;
        tx.commit()?;
        Ok(Journal { db })
    }

    /// Saves an event; an id already saved is kept as it was.
    pub fn publish(&self, e: &Envelope) -> Result<()> {
        publish(&self.db, e)
    }

    /// The oldest event not yet handled, apart from those for workers and
    /// those `held` (messages waiting for room).
    pub fn next(&self, held: &[i64]) -> Result<Option<Recorded>> {
        let held: Vec<String> = held.iter().map(i64::to_string).collect();
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND subject != 'itero.worker'
             AND seq NOT IN ({}) ORDER BY seq LIMIT 1",
            held.join(",")
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

    /// The messages for the workspace since `time`, apart from `except`
    /// (the one that started a run) and the daemon's own (`itero.`, which
    /// no run waits for): what a run starting to wait may have missed.
    pub fn messages_since(&self, time: &str, except: Option<&str>) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE subject = 'itero.app' AND time >= ?1
               AND (?2 IS NULL OR id != ?2) AND substr(type, 1, 6) != 'itero.' ORDER BY seq"
        );
        let mut s = self.db.prepare(&sql)?;
        let out = s
            .query_map(params![time, except], row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// The message that started a run, if one did.
    pub fn started_by(&self, run: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT cause FROM events WHERE subject = ?1 AND type = 'run.started' ORDER BY seq LIMIT 1",
                params![subject(run)],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// The type of the message that started a run, through the runs it
    /// split from; `None` when no message did (a schedule, a split's lost
    /// parent).
    pub fn started_on(&self, run: &Run) -> Result<Option<String>> {
        let mut cause = run.message.clone();
        while let Some(id) = cause {
            if let Some(parent) = self.run(&id)? {
                cause = parent.message;
                continue;
            }
            return Ok(self
                .db
                .query_row("SELECT type FROM events WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .optional()?);
        }
        Ok(None)
    }

    /// The runs a message started (and those split from them), oldest
    /// first; `None` while no daemon has taken the message.
    pub fn runs_of(&self, message: &str) -> Result<Option<Vec<Run>>> {
        let handled: Option<Option<String>> = self
            .db
            .query_row(
                "SELECT handled FROM events WHERE id = ?1",
                params![message],
                |r| r.get(0),
            )
            .optional()?;
        if handled.flatten().is_none() {
            return Ok(None);
        }
        let mut s = self.db.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE message = ?1 ORDER BY started, id"
        ))?;
        let rows: Vec<Result<Run>> = s
            .query_map(params![message], run_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Some(rows.into_iter().collect::<Result<_>>()?))
    }

    /// A run by id.
    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        self.db
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
                params![id],
                run_row,
            )
            .optional()?
            .transpose()
    }

    /// A flag the daemon keeps between passes, like `LOG_FULL`.
    pub fn flag(&self, key: &str) -> Result<bool> {
        let v: Option<String> = self
            .db
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(v.as_deref() == Some("1"))
    }

    pub fn set_flag(&self, key: &str, on: bool) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, if on { "1" } else { "0" }],
        )?;
        Ok(())
    }

    /// Gives back the room of what was forgotten: the file shrinks.
    pub fn compact(&self) -> Result<()> {
        self.db
            .execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// How many messages for the workspace no daemon has taken yet.
    pub fn waiting_messages(&self) -> Result<usize> {
        let n: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM events WHERE subject = 'itero.app' AND handled IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// How many runs are under way, those of the workflows in `apart` left
    /// out.
    pub fn active_count(&self, apart: &[String]) -> Result<usize> {
        let n: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM runs WHERE ended IS NULL
             AND workflow NOT IN (SELECT value FROM json_each(?1))",
            params![json(&apart)?],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// The runs with this id or name, newest first: a name may be given
    /// twice.
    pub fn run_by(&self, id_or_name: &str) -> Result<Vec<Run>> {
        let mut s = self.db.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1 OR name = ?1 ORDER BY started DESC, id DESC"
        ))?;
        let rows: Vec<Result<Run>> = s
            .query_map(params![id_or_name], run_row)?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter().collect()
    }

    /// The workspace's identity, made with its journal.
    pub fn workspace(&self) -> Result<Identity> {
        let t: String =
            self.db
                .query_row("SELECT value FROM meta WHERE key = 'workspace'", [], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(&t)?)
    }

    /// A workflow's identity by its name, made the first time it is seen.
    pub fn workflow_id(&self, name: &str, at: &str) -> Result<Identity> {
        see_workflow(&self.db, name, at)?;
        Ok(self.db.query_row(
            "SELECT id, name, created, itero FROM workflow_ids WHERE name = ?1",
            params![name],
            |r| {
                Ok(Identity {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created: r.get(2)?,
                    itero: r.get(3)?,
                })
            },
        )?)
    }

    /// Makes the identities of workflows not seen before, in one go.
    pub fn see_workflows<'a>(
        &mut self,
        names: impl IntoIterator<Item = &'a String>,
        at: &str,
    ) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for n in names {
            see_workflow(&tx, n, at)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The runs not ended, oldest first.
    pub fn active_runs(&self) -> Result<Vec<Run>> {
        let mut s = self.db.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs
             WHERE ended IS NULL ORDER BY started, id"
        ))?;
        let rows: Vec<Result<Run>> = s.query_map([], run_row)?.collect::<rusqlite::Result<_>>()?;
        rows.into_iter().collect()
    }

    /// The newest `limit` runs, of one workflow or all but those in
    /// `apart`, ended one way (the kind of `ended`: done, failed, error,
    /// split) or any, newest first.
    pub fn runs_page(
        &self,
        workflow: Option<&str>,
        ended: Option<&str>,
        apart: &[String],
        limit: usize,
    ) -> Result<Vec<Run>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut s = self.db.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs
             WHERE (?1 IS NULL OR workflow = ?1)
               AND (?2 IS NULL OR json_extract(runtime, '$.ended.kind') = ?2)
               AND workflow NOT IN (SELECT value FROM json_each(?3))
             ORDER BY started DESC, id DESC LIMIT ?4"
        ))?;
        let rows: Vec<Result<Run>> = s
            .query_map(params![workflow, ended, json(&apart)?, limit], run_row)?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter().collect()
    }

    /// Every run, oldest first.
    pub fn runs(&self) -> Result<Vec<Run>> {
        let mut out = self.runs_page(None, None, &[], usize::MAX)?;
        out.reverse();
        Ok(out)
    }

    /// The pin of a version of a run's workflows, made the first time that
    /// version is seen: a UUIDv7.
    pub fn pin(&self, workflow: &str, flows: &BTreeMap<String, Value>) -> Result<String> {
        pin(&self.db, workflow, flows)
    }

    /// The workflows pinned under `pin`: the run's own name, and all of
    /// them by name.
    pub fn pinned(&self, pin: &str) -> Result<Option<(String, BTreeMap<String, Value>)>> {
        let t: Option<String> = self
            .db
            .query_row(
                "SELECT definition FROM workflows WHERE pin = ?1",
                params![pin],
                |r| r.get(0),
            )
            .optional()?;
        let Some(t) = t else {
            return Ok(None);
        };
        let v: Value = serde_json::from_str(&t)?;
        let name = v["workflow"].as_str().unwrap_or("").to_string();
        let flows: BTreeMap<String, Value> = serde_json::from_value(v["flows"].clone())?;
        Ok(Some((name, flows)))
    }

    /// Starts a run, its `run.started` event carrying the message's data,
    /// in one transaction. With a schedule's key, that schedule's last slot
    /// becomes `now` in the same transaction, so a slot never starts two
    /// runs, whatever became of the start.
    pub fn start_run(&mut self, s: &Start, now: &str, schedule: Option<&str>) -> Result<String> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pin = pin(&tx, &s.workflow, &s.flows)?;
        let id = insert_run(&tx, s.new_run(&pin, &Runtime::default(), now))?;
        if let Some(key) = schedule {
            set_schedule_last(&tx, key, now)?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Handles a message for the workspace, `r`: starts each of `starts`,
    /// copies the message to each run in `waiting`, and marks it handled.
    /// One transaction, so a message taken again starts and copies nothing
    /// twice. Nothing started and nothing copied is noted, not an error.
    /// Returns the runs started.
    pub fn route(
        &mut self,
        r: &Recorded,
        starts: &[Start],
        waiting: &[String],
        at: &str,
    ) -> Result<Vec<String>> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut started = vec![];
        for s in starts {
            let pin = pin(&tx, &s.workflow, &s.flows)?;
            started.push(insert_run(&tx, s.new_run(&pin, &Runtime::default(), at))?);
        }
        for id in waiting {
            publish(&tx, &copy(&r.event, id, at))?;
        }
        let note = (starts.is_empty() && waiting.is_empty())
            .then_some("no workflow starts on it, and no run waits for it");
        tx.execute(
            "UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL",
            params![r.seq, at, note],
        )?;
        tx.commit()?;
        Ok(started)
    }

    /// When a schedule (`<workflow>#<cron>`) last started a run, or was
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

    /// The earliest time a waiting run's timer is set for.
    pub fn next_timer(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT MIN(at) FROM timers", [], |r| r.get(0))?)
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
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        publish(&tx, e)?;
        tx.execute("DELETE FROM timers WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Saves an answer: all of it, or (on any error) none of it.
    pub fn handle(&mut self, h: Handled) -> Result<()> {
        let now = h.at;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        for seq in &h.stopped {
            tx.execute(
                "UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL",
                params![seq, now, JOB_STOPPED],
            )?;
        }
        for e in &h.events {
            publish(&tx, e)?;
        }
        for s in &h.splits {
            insert_run(
                &tx,
                NewRun {
                    workflow: s.workflow,
                    pin: s.pin,
                    rt: s.runtime,
                    input: s.input,
                    now,
                    cause: None,
                    message: s.message,
                },
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets what ended more than a day before `cutoff` says: runs that
    /// ended before it, with their events and timers; jobs done before it;
    /// and messages handled before it that no run under way may still
    /// want (one newer than the oldest run under way's start is kept).
    pub fn prune(&self, cutoff: &str) -> Result<()> {
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let r = (|| -> Result<()> {
            let ended = "SELECT id FROM runs WHERE ended IS NOT NULL AND ended < ?1";
            self.db.execute(
                &format!("DELETE FROM timers WHERE run IN ({ended})"),
                params![cutoff],
            )?;
            self.db.execute(
                &format!(
                    "DELETE FROM events WHERE subject IN (SELECT 'itero.run.' || id FROM ({ended}))"
                ),
                params![cutoff],
            )?;
            self.db.execute(
                "DELETE FROM runs WHERE ended IS NOT NULL AND ended < ?1",
                params![cutoff],
            )?;
            self.db.execute(
                "DELETE FROM jobs WHERE seq IN (SELECT seq FROM events WHERE subject = 'itero.worker' AND handled IS NOT NULL AND handled < ?1)",
                params![cutoff],
            )?;
            self.db.execute(
                "DELETE FROM events WHERE subject = 'itero.worker' AND handled IS NOT NULL AND handled < ?1",
                params![cutoff],
            )?;
            self.db.execute(
                "DELETE FROM events WHERE subject = 'itero.app' AND handled IS NOT NULL AND handled < ?1
                   AND time < COALESCE((SELECT MIN(started) FROM runs WHERE ended IS NULL), '9999')",
                params![cutoff],
            )?;
            Ok(())
        })();
        match r {
            Ok(()) => self.db.execute_batch("COMMIT")?,
            Err(_) => self.db.execute_batch("ROLLBACK")?,
        }
        r
    }
}

const RUN_COLUMNS: &str = "id, workflow, pin, runtime, input, started, ended, name, message";

fn run_row(r: &rusqlite::Row) -> rusqlite::Result<Result<Run>> {
    let (rt, input): (String, Option<String>) = (r.get(3)?, r.get(4)?);
    let (id, workflow, pin, started, ended, name, message) = (
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    );
    Ok((|| {
        Ok(Run {
            id,
            name,
            workflow,
            pin,
            runtime: serde_json::from_str(&rt)?,
            input: match input {
                Some(t) => serde_json::from_str(&t)?,
                None => Value::Null,
            },
            started,
            ended,
            message,
        })
    })())
}

/// A message copied to a run: the manager decides whether it is the run's.
pub fn copy(e: &Envelope, run: &str, at: &str) -> Envelope {
    Envelope::new(
        &e.source,
        "message",
        &subject(run),
        at,
        Some(e.id.clone()),
        json!({"id": e.id, "type": e.kind, "data": e.data}),
    )
}

fn pin(db: &Connection, workflow: &str, flows: &BTreeMap<String, Value>) -> Result<String> {
    let definition = json(&json!({"workflow": workflow, "flows": flows}))?;
    let known: Option<String> = db
        .query_row(
            "SELECT pin FROM workflows WHERE definition = ?1",
            params![definition],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(p) = known {
        return Ok(p);
    }
    let p = uuid::Uuid::now_v7().to_string();
    db.execute(
        "INSERT INTO workflows (pin, definition) VALUES (?1, ?2)",
        params![p, definition],
    )?;
    Ok(p)
}

/// A new run, and its `run.started` event carrying `input`, caused by the
/// message that started it; returns its id.
/// A run to insert.
struct NewRun<'a> {
    workflow: &'a str,
    pin: &'a str,
    rt: &'a Runtime,
    input: &'a Value,
    now: &'a str,
    /// The message that started it: its `run.started` event's cause.
    cause: Option<&'a str>,
    /// The message whose answer it is part of: its cause, or (split) its
    /// parent's.
    message: Option<&'a str>,
}

fn insert_run(db: &Connection, r: NewRun) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    db.execute(
        "INSERT INTO runs (id, name, workflow, pin, runtime, args, input, started, message)
         VALUES (?1, ?2, ?3, ?4, ?5, '{}', ?6, ?7, ?8)",
        params![
            id,
            crate::names::random(),
            r.workflow,
            r.pin,
            json(r.rt)?,
            json(r.input)?,
            r.now,
            r.message
        ],
    )?;
    let e = Envelope::new(
        "daemon",
        "run.started",
        &subject(&id),
        r.now,
        r.cause.map(str::to_string),
        r.input.clone(),
    );
    publish(db, &e)?;
    Ok(id)
}

/// A workflow's identity, made unless it has one.
fn see_workflow(db: &Connection, name: &str, at: &str) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO workflow_ids (name, id, created, itero) VALUES (?1, ?2, ?3, ?4)",
        params![name, uuid::Uuid::now_v7().to_string(), at, ITERO],
    )?;
    Ok(())
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
