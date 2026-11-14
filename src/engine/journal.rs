//! The engine's journal: one SQLite file per workspace,
//! `.itero/journal.sqlite`, local and never synced. It is the message queue
//! and log, and keeps each run's place, the workflow versions runs are
//! pinned to, timers and schedules.
use super::Timer;
use super::steps::Runtime;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The journal's schema version, kept in its `meta` table.
pub const SCHEMA_VERSION: i64 = OLDEST + MIGRATIONS.len() as i64;

/// The oldest schema a journal may have, from itero 0.4.0 on. An older one
/// is refused: nobody depended on itero then.
const OLDEST: i64 = 7;

/// The steps from each version to the next: `MIGRATIONS[0]` takes `OLDEST`
/// to the one after, and so on. A step is only ever added, never changed:
/// a journal on someone's machine was made by the steps as they were.
const MIGRATIONS: [&str; 6] = [
    // 8: the tables nothing reads any more
    "DROP TABLE IF EXISTS queued; DROP TABLE IF EXISTS paused;
     DROP TABLE IF EXISTS cursors; DROP TABLE IF EXISTS memory;",
    // 9: callbacks are gone; a workflow posts its result itself
    "DROP TABLE IF EXISTS replies;
     ALTER TABLE events DROP COLUMN reply; ALTER TABLE runs DROP COLUMN reply;",
    // 10: a workflow used as a step is a run of its own, linked to its parent
    "ALTER TABLE runs ADD COLUMN parent TEXT; ALTER TABLE runs ADD COLUMN parent_path TEXT;",
    // 11: the windows in a workflow's on, and what they have let through
    "CREATE TABLE IF NOT EXISTS seen (key TEXT NOT NULL, value TEXT NOT NULL, until TEXT, PRIMARY KEY (key, value));",
    // 12: the events a batch or at_least keeps for a run, and the batches open
    "CREATE TABLE IF NOT EXISTS kept (key TEXT NOT NULL, seq INTEGER NOT NULL, at TEXT NOT NULL, PRIMARY KEY (key, seq));
     CREATE TABLE IF NOT EXISTS batches (key TEXT PRIMARY KEY, workflow TEXT NOT NULL, closes TEXT, per TEXT);",
    // 13: whether a run waits (set for the runs under way once these run,
    // `WAITING_FROM`), the runs under way counted by workflow as they start
    // and end, and the indexes a pass asks by
    "ALTER TABLE runs ADD COLUMN waiting INTEGER NOT NULL DEFAULT 0;
CREATE TABLE IF NOT EXISTS under_way (workflow TEXT PRIMARY KEY, runs INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS run_began AFTER INSERT ON runs WHEN NEW.ended IS NULL BEGIN
  INSERT INTO under_way (workflow, runs) VALUES (NEW.workflow, 1)
    ON CONFLICT (workflow) DO UPDATE SET runs = runs + 1;
END;
CREATE TRIGGER IF NOT EXISTS run_ended AFTER UPDATE OF ended ON runs
  WHEN OLD.ended IS NULL AND NEW.ended IS NOT NULL BEGIN
  UPDATE under_way SET runs = runs - 1 WHERE workflow = NEW.workflow;
END;
CREATE TRIGGER IF NOT EXISTS run_again AFTER UPDATE OF ended ON runs
  WHEN OLD.ended IS NOT NULL AND NEW.ended IS NULL BEGIN
  INSERT INTO under_way (workflow, runs) VALUES (NEW.workflow, 1)
    ON CONFLICT (workflow) DO UPDATE SET runs = runs + 1;
END;
CREATE TRIGGER IF NOT EXISTS run_gone AFTER DELETE ON runs WHEN OLD.ended IS NULL BEGIN
  UPDATE under_way SET runs = runs - 1 WHERE workflow = OLD.workflow;
END;
INSERT INTO under_way (workflow, runs) SELECT workflow, COUNT(*) FROM runs WHERE ended IS NULL GROUP BY workflow;
CREATE INDEX IF NOT EXISTS runs_open ON runs (workflow) WHERE ended IS NULL;
CREATE INDEX IF NOT EXISTS runs_waiting ON runs (started, id) WHERE ended IS NULL AND waiting = 1;
CREATE INDEX IF NOT EXISTS events_queue ON events (subject = 'itero.worker', seq) WHERE handled IS NULL;
CREATE INDEX IF NOT EXISTS jobs_by_run ON events (json_extract(data, '$.run'))
  WHERE handled IS NULL AND subject = 'itero.worker';",
];

/// The schema from which `runs.waiting` is kept: a journal migrated from
/// before has it worked out for its runs under way.
const WAITING_FROM: i64 = 13;

/// Why a job's run was lost when nothing else says: the daemon stopped
/// while it ran.
const STOPPED: &str = "the daemon stopped while it ran";

/// The note on an event cancelled before any daemon handled it.
pub const CANCELLED: &str = "cancelled by command";

/// An event for the workspace no daemon has handled yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queued {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub time: String,
    pub data: Value,
}

/// What cancelling an event did.
#[derive(Debug, Clone, PartialEq)]
pub enum Cancelled {
    Done,
    /// A daemon had handled it: what it noted, and the runs it started.
    Handled {
        note: Option<String>,
        started: Vec<String>,
    },
    /// No daemon had routed it, but runs that started to wait had it
    /// already (copied by `messages_since`): those runs.
    Reached {
        runs: Vec<String>,
    },
    /// No event of the workspace has that id.
    Missing,
}

/// The note on a job whose run was stopped by command.
pub const JOB_RUN_STOPPED: &str = "stopped: its run was stopped";

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
  message TEXT,
  parent TEXT,
  parent_path TEXT,
  waiting INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS under_way (workflow TEXT PRIMARY KEY, runs INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS run_began AFTER INSERT ON runs WHEN NEW.ended IS NULL BEGIN
  INSERT INTO under_way (workflow, runs) VALUES (NEW.workflow, 1)
    ON CONFLICT (workflow) DO UPDATE SET runs = runs + 1;
END;
CREATE TRIGGER IF NOT EXISTS run_ended AFTER UPDATE OF ended ON runs
  WHEN OLD.ended IS NULL AND NEW.ended IS NOT NULL BEGIN
  UPDATE under_way SET runs = runs - 1 WHERE workflow = NEW.workflow;
END;
CREATE TRIGGER IF NOT EXISTS run_again AFTER UPDATE OF ended ON runs
  WHEN OLD.ended IS NOT NULL AND NEW.ended IS NULL BEGIN
  INSERT INTO under_way (workflow, runs) VALUES (NEW.workflow, 1)
    ON CONFLICT (workflow) DO UPDATE SET runs = runs + 1;
END;
CREATE TRIGGER IF NOT EXISTS run_gone AFTER DELETE ON runs WHEN OLD.ended IS NULL BEGIN
  UPDATE under_way SET runs = runs - 1 WHERE workflow = OLD.workflow;
END;
CREATE INDEX IF NOT EXISTS runs_open ON runs (workflow) WHERE ended IS NULL;
CREATE INDEX IF NOT EXISTS runs_waiting ON runs (started, id) WHERE ended IS NULL AND waiting = 1;
CREATE INDEX IF NOT EXISTS events_queue ON events (subject = 'itero.worker', seq) WHERE handled IS NULL;
CREATE INDEX IF NOT EXISTS jobs_by_run ON events (json_extract(data, '$.run'))
  WHERE handled IS NULL AND subject = 'itero.worker';
CREATE TABLE IF NOT EXISTS workflows (pin TEXT PRIMARY KEY, definition TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS workflow_ids (name TEXT PRIMARY KEY, id TEXT NOT NULL, created TEXT NOT NULL, itero TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS timers (id INTEGER PRIMARY KEY AUTOINCREMENT, run TEXT NOT NULL, at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS jobs (seq INTEGER PRIMARY KEY, runs INTEGER NOT NULL, lost TEXT);
CREATE TABLE IF NOT EXISTS schedules (key TEXT PRIMARY KEY, last TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS seen (key TEXT NOT NULL, value TEXT NOT NULL, until TEXT, PRIMARY KEY (key, value));
CREATE TABLE IF NOT EXISTS kept (key TEXT NOT NULL, seq INTEGER NOT NULL, at TEXT NOT NULL, PRIMARY KEY (key, seq));
CREATE TABLE IF NOT EXISTS batches (key TEXT PRIMARY KEY, workflow TEXT NOT NULL, closes TEXT, per TEXT);
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

/// An action and how it breaks runs: how often, when and why last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Breaks {
    pub action: String,
    pub errors: usize,
    pub last: String,
    pub why: Option<String>,
    /// The last run it broke, by name.
    pub run: String,
}

/// A run an action broke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Broke {
    pub when: String,
    pub workflow: String,
    pub run: String,
    pub id: String,
    pub why: Option<String>,
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
    /// The run whose step started it, when it is a workflow used as a step.
    pub parent: Option<String>,
    /// That step's path in the parent's position: where its end goes back.
    pub parent_path: Option<Vec<usize>>,
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

/// The workspace's id. A journal already made is read without its write
/// lock, so a secret can be looked up while a pass holds it; one not made
/// yet (or half made) is opened as `Journal::open` does. A journal too busy
/// to read is an error: opening it for writing would only wait longer.
pub fn read_id(ws: &Workspace) -> Result<String> {
    let read = || -> Result<String> {
        let db = Connection::open_with_flags(ws.journal(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(BUSY_WAIT)?;
        let t: String =
            db.query_row("SELECT value FROM meta WHERE key = 'workspace'", [], |r| {
                r.get(0)
            })?;
        Ok(serde_json::from_str::<Identity>(&t)?.id)
    };
    match read() {
        Ok(id) => Ok(id),
        Err(e) if busy(&e) => Err(e.context(format!("{} is busy", ws.journal().display()))),
        Err(_) => Ok(Journal::open(ws)?.workspace()?.id),
    }
}

/// How long a process waits for another's hold on the journal.
const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Puts the journal in WAL mode. A new one is not in it yet, and two
/// processes may make it at once: the switch takes an exclusive lock with
/// one try, never waiting as `busy_timeout` does, so the wait is here. A
/// journal already in WAL mode changes nothing and takes no such lock.
fn write_ahead(db: &Connection) -> Result<()> {
    let end = std::time::Instant::now() + BUSY_WAIT;
    loop {
        match db.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => return Ok(()),
            Err(e) => {
                let e = anyhow::Error::from(e);
                if !busy(&e) || std::time::Instant::now() >= end {
                    return Err(e);
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Whether SQLite said the database is busy or locked.
fn busy(e: &anyhow::Error) -> bool {
    use rusqlite::ErrorCode::{DatabaseBusy, DatabaseLocked};
    e.downcast_ref::<rusqlite::Error>()
        .and_then(rusqlite::Error::sqlite_error_code)
        .is_some_and(|c| matches!(c, DatabaseBusy | DatabaseLocked))
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
    /// The windows of its workflow's `on` it must pass, with this event's
    /// values: all must let it through, and then remember it.
    pub gates: Vec<Gated>,
}

/// A window a run's start passes: its key (the workflow, the window, the
/// `per` value's hash) and what it asks, by the runs it let through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gated {
    pub key: String,
    pub gate: Gate,
    /// The `per` value itself, given to a run of kept events as `per`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per: Option<Value>,
}

/// The gate of window `w` at `key` (whose window it is: a workflow's
/// `on/<i>`, a step's path), with the values `state` gives it at `now`. A
/// value the state lacks is `null`, a value as any other. Values and the
/// `per` value are kept as hashes, so a row stays small.
pub fn gated(
    key: &str,
    w: &crate::flow::Window,
    state: &Map<String, Value>,
    now: chrono::DateTime<chrono::Utc>,
) -> Gated {
    use crate::flow;
    let value = |path: &str| flow::lookup(state, path).cloned().unwrap_or(Value::Null);
    let hash = |path: &str| {
        use sha2::{Digest, Sha256};
        Sha256::digest(value(path).to_string().as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let shift =
        |d: &std::time::Duration| chrono::Duration::from_std(*d).unwrap_or(chrono::Duration::MAX);
    let stamp = |t: chrono::DateTime<chrono::Utc>| super::daemon::stamp(t);
    Gated {
        key: format!(
            "{key}\u{1f}{}",
            w.per.as_deref().map(hash).unwrap_or_default()
        ),
        per: w.per.as_deref().map(value),
        gate: match &w.gate {
            flow::Gate::Unique { value, within } => Gate::Unique {
                value: hash(value),
                until: within.as_ref().map(|d| stamp(now + shift(d))),
            },
            flow::Gate::Changed { value } => Gate::Changed { value: hash(value) },
            flow::Gate::AtMost { every } => Gate::AtMost {
                until: stamp(now + shift(every)),
            },
            flow::Gate::Batch { count, within } => Gate::Batch {
                count: *count,
                closes: within.as_ref().map(|d| stamp(now + shift(d))),
            },
            flow::Gate::AtLeast { count, within } => Gate::AtLeast {
                count: *count,
                since: stamp(now - shift(within)),
            },
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Gate {
    /// No run started with `value` (a hash) that is still remembered;
    /// remembered until `until`, for good without.
    Unique {
        value: String,
        until: Option<String>,
    },
    /// The last run started with another `value` (a hash), or none did.
    Changed { value: String },
    /// No run started within its period; the next period ends at `until`.
    AtMost { until: String },
    /// Kept for one run of `count` events, or of those kept when the batch
    /// `closes`, opened by its first; whichever comes first.
    Batch {
        count: Option<u32>,
        closes: Option<String>,
    },
    /// Kept for one run once `count` were kept since `since`.
    AtLeast { count: u32, since: String },
}

impl Gate {
    fn keeps(&self) -> bool {
        matches!(self, Gate::Batch { .. } | Gate::AtLeast { .. })
    }
}

/// The `$event` of a run of kept events: their data in order, how many,
/// the times of the first and the last, and its window's `per` value.
fn kept_input(tx: &rusqlite::Connection, key: &str) -> Result<Option<Value>> {
    let per: Option<String> = tx
        .query_row(
            "SELECT per FROM batches WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let mut q = tx.prepare(
        "SELECT e.data, e.time FROM kept k JOIN events e ON e.seq = k.seq
         WHERE k.key = ?1 ORDER BY k.seq",
    )?;
    let rows: Vec<(String, String)> = q
        .query_map(params![key], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(None);
    };
    let events: Vec<Value> = rows
        .iter()
        .map(|(d, _)| serde_json::from_str(d).unwrap_or(Value::Null))
        .collect();
    let mut input = json!({
        "events": events,
        "count": rows.len(),
        "first": first.1,
        "last": last.1,
    });
    if let Some(per) = per {
        input["per"] = serde_json::from_str(&per).unwrap_or(Value::Null);
    }
    Ok(Some(input))
}

/// Takes the events `key` kept, for a run: its `$event`; none when it
/// kept none. The batch, if one was open, is closed.
fn take_kept(tx: &rusqlite::Connection, key: &str) -> Result<Option<Value>> {
    let input = kept_input(tx, key)?;
    tx.execute("DELETE FROM kept WHERE key = ?1", params![key])?;
    tx.execute("DELETE FROM batches WHERE key = ?1", params![key])?;
    Ok(input)
}

/// Keeps the message `seq` in `g`'s window for `workflow`: the `$event`
/// of the run it starts now (a full batch, enough at_least), or none.
fn keep(
    tx: &rusqlite::Connection,
    g: &Gated,
    workflow: &str,
    seq: i64,
    at: &str,
) -> Result<Option<Value>> {
    if let Gate::AtLeast { since, .. } = &g.gate {
        tx.execute(
            "DELETE FROM kept WHERE key = ?1 AND at < ?2",
            params![g.key, since],
        )?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO kept (key, seq, at) VALUES (?1, ?2, ?3)",
        params![g.key, seq, at],
    )?;
    // an at_least's row has no time to close at: it holds its per only
    let closes = match &g.gate {
        Gate::Batch { closes, .. } => closes.clone(),
        _ => None,
    };
    let per = g.per.as_ref().map(Value::to_string);
    tx.execute(
        "INSERT OR IGNORE INTO batches (key, workflow, closes, per) VALUES (?1, ?2, ?3, ?4)",
        params![g.key, workflow, closes, per],
    )?;
    let count = match &g.gate {
        Gate::Batch { count, .. } => *count,
        Gate::AtLeast { count, .. } => Some(*count),
        _ => None,
    };
    let n: i64 = tx.query_row(
        "SELECT COUNT(*) FROM kept WHERE key = ?1",
        params![g.key],
        |r| r.get(0),
    )?;
    match count {
        Some(c) if n >= i64::from(c) => take_kept(tx, &g.key),
        _ => Ok(None),
    }
}

/// What routing a message did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routed {
    /// The runs started.
    pub started: Vec<String>,
    /// The workflows a window held back, each with why.
    pub held: Vec<(String, String)>,
    /// The workflows whose batch or at_least kept it, for a later run.
    pub kept: Vec<String>,
}

/// Why a window holds `g` back, by what it let through before; none when it
/// lets it through.
fn held_back(tx: &rusqlite::Connection, g: &Gated, at: &str) -> Result<Option<String>> {
    Ok(match &g.gate {
        Gate::Unique { value, .. } => tx
            .query_row(
                "SELECT until FROM seen WHERE key = ?1 AND value = ?2",
                params![g.key, value],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .filter(|until| until.as_deref().is_none_or(|u| u > at))
            .map(|until| match until {
                Some(u) => format!("unique: a run started with this value, remembered until {u}"),
                None => "unique: a run started with this value".into(),
            }),
        Gate::Changed { value } => tx
            .query_row(
                "SELECT value FROM seen WHERE key = ?1",
                params![g.key],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .filter(|last| last == value)
            .map(|_| "changed: the same value as the last run's".into()),
        Gate::AtMost { .. } => tx
            .query_row(
                "SELECT until FROM seen WHERE key = ?1 AND value = ''",
                params![g.key],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .filter(|until| until.as_str() > at)
            .map(|until| format!("at_most: a run started in this period, which ends at {until}")),
        // these keep what they let through (`keep`): they hold nothing back
        Gate::Batch { .. } | Gate::AtLeast { .. } => None,
    })
}

/// Remembers that `g` let a run through.
fn let_through(tx: &rusqlite::Connection, g: &Gated) -> Result<()> {
    match &g.gate {
        Gate::Unique { value, until } => tx.execute(
            "INSERT OR REPLACE INTO seen (key, value, until) VALUES (?1, ?2, ?3)",
            params![g.key, value, until],
        )?,
        Gate::Changed { value } => {
            tx.execute("DELETE FROM seen WHERE key = ?1", params![g.key])?;
            tx.execute(
                "INSERT INTO seen (key, value, until) VALUES (?1, ?2, NULL)",
                params![g.key, value],
            )?
        }
        Gate::AtMost { until } => tx.execute(
            "INSERT OR REPLACE INTO seen (key, value, until) VALUES (?1, '', ?2)",
            params![g.key, until],
        )?,
        // kept by `keep`, not remembered here
        Gate::Batch { .. } | Gate::AtLeast { .. } => 0,
    };
    Ok(())
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
            parent: None,
        }
    }
}

/// A repeated workflow's next round: a new run of the same version, its
/// runtime seeded with the round before's results.
pub struct NextRound<'a> {
    pub workflow: &'a str,
    pub pin: &'a str,
    pub runtime: &'a Runtime,
    pub input: &'a Value,
    /// The message of the round before.
    pub message: Option<&'a str>,
    /// The round before's parent and step, when it is a workflow used as
    /// a step: the last round answers that step.
    pub parent: Option<(&'a str, &'a [usize])>,
}

/// A run a step of another starts: a workflow used as a step. Its
/// `$event` is the args the step gives it; it runs the versions its parent
/// is pinned to.
pub struct Child<'a> {
    pub workflow: &'a str,
    pub flows: &'a BTreeMap<String, Value>,
    pub input: &'a Value,
    /// Its first state beside `$event` (a repeated step's round).
    pub seed: &'a Map<String, Value>,
    /// The step's path in the parent's position.
    pub path: Vec<usize>,
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
    /// When (RFC 3339): the daemon's, as its pass tells the time.
    pub at: &'a str,
    pub error: Option<String>,
    /// The run's runtime after it, and whether the run has ended.
    pub run: Option<(&'a str, &'a Runtime, bool)>,
    /// The run's timers from now on; `None` leaves them.
    pub timers: Option<&'a [Timer]>,
    pub events: Vec<Envelope>,
    /// Events about a run that only its story shows (a step stopped): saved
    /// handled, so no manager is given them.
    pub shown: Vec<Envelope>,
    /// A repeated workflow's next round, started by this answer.
    pub next_round: Option<NextRound<'a>>,
    /// Runs its steps start, each linked to this run (`run` names it).
    pub children: Vec<Child<'a>>,
    /// Jobs (by `seq`) whose steps this answer stopped: handled, noted
    /// `stopped_why`, so none runs again.
    pub stopped: Vec<i64>,
    /// Why they were: `JOB_STOPPED` (a group), `JOB_RUN_STOPPED` (the run).
    pub stopped_why: &'a str,
    /// The windows of the steps it starts, which let them through: each
    /// remembers it.
    pub let_through: Vec<Gated>,
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
        db.busy_timeout(BUSY_WAIT)?;
        write_ahead(&db)?;
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
                if v < WAITING_FROM {
                    mark_waiting(&tx)?;
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

    /// The oldest event not yet handled after `after`, apart from those for
    /// workers. Events are taken in order, so a pass asks after the last it
    /// took: what it has not handled up to there it holds (messages waiting
    /// for room), and takes again by `unhandled`.
    pub fn next(&self, after: i64) -> Result<Option<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND (subject = 'itero.worker') = 0
             AND seq > ?1 ORDER BY seq LIMIT 1"
        );
        Ok(self
            .db
            .prepare_cached(&sql)?
            .query_row([after], row)
            .optional()?)
    }

    /// Event `seq`, while no daemon has handled it.
    pub fn unhandled(&self, seq: i64) -> Result<Option<Recorded>> {
        let sql = format!("SELECT {COLUMNS} FROM events WHERE seq = ?1 AND handled IS NULL");
        Ok(self
            .db
            .prepare_cached(&sql)?
            .query_row([seq], row)
            .optional()?)
    }

    /// Up to `room` jobs not yet run and not among `running`, oldest first:
    /// the ones a pass starts next.
    pub fn jobs_to_start(&self, running: &[i64], room: usize) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND (subject = 'itero.worker') = 1
             AND seq NOT IN (SELECT value FROM json_each(?1)) ORDER BY seq LIMIT ?2"
        );
        let mut s = self.db.prepare_cached(&sql)?;
        let out = s
            .query_map(params![json(&running)?, room as i64], row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Run `id`'s jobs not yet handled, oldest first.
    pub fn jobs_of(&self, id: &str) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE handled IS NULL AND subject = 'itero.worker'
             AND json_extract(data, '$.run') = ?1 ORDER BY seq"
        );
        let mut s = self.db.prepare_cached(&sql)?;
        let out = s.query_map([id], row)?.collect::<rusqlite::Result<_>>()?;
        Ok(out)
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

    /// Whether job `seq` can never start again: handled, or forgotten.
    pub fn job_done(&self, seq: i64) -> Result<bool> {
        let handled: Option<Option<String>> = self
            .db
            .query_row(
                "SELECT handled FROM events WHERE seq = ?1 AND subject = 'itero.worker'",
                params![seq],
                |r| r.get(0),
            )
            .optional()?;
        Ok(handled.is_none_or(|h| h.is_some()))
    }

    /// Why a job's run was lost; the job stays waiting, to run again.
    pub fn lose_job(&self, seq: i64, why: &str) -> Result<()> {
        self.db.execute(
            "UPDATE jobs SET lost = ?2 WHERE seq = ?1",
            params![seq, why],
        )?;
        Ok(())
    }

    /// The newest event's seq, 0 for none: where following from now
    /// starts.
    pub fn last_seq(&self) -> Result<i64> {
        Ok(self
            .db
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?)
    }

    /// The seq of the event `id` that came into the workspace, while the
    /// journal keeps it.
    pub fn seq_came_in(&self, id: &str) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "SELECT seq FROM events WHERE id = ?1 AND subject = ?2",
                params![id, subject("app")],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Up to `limit` of the events that came into the workspace after
    /// `seq`, oldest first: not the engine's inner messages (a worker's
    /// job, a timer or a stop for one run).
    pub fn came_in_after(&self, seq: i64, limit: usize) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE seq > ?1 AND subject = ?2 ORDER BY seq LIMIT ?3"
        );
        let mut s = self.db.prepare(&sql)?;
        let out = s
            .query_map(params![seq, subject("app"), limit as i64], row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// SQLite's `data_version`: it changes when another connection has
    /// written to the journal, so a reader looks again only then.
    pub fn changed(&self) -> Result<i64> {
        Ok(self.db.query_row("PRAGMA data_version", [], |r| r.get(0))?)
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
    /// (the one that started a run), the daemon's own (`itero.`, which no
    /// run waits for) and those cancelled: what a run starting to wait may
    /// have missed. Those not yet routed are in it: one held for room
    /// under `limits.runs` reaches the run all the same.
    pub fn messages_since(&self, time: &str, except: Option<&str>) -> Result<Vec<Recorded>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM events WHERE subject = 'itero.app' AND time >= ?1
               AND (?2 IS NULL OR id != ?2) AND substr(type, 1, 6) != 'itero.'
               AND (note IS NULL OR note != ?3) ORDER BY seq"
        );
        let mut s = self.db.prepare(&sql)?;
        let out = s
            .query_map(params![time, except, CANCELLED], row)?
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
    /// split from and the runs whose steps started it; `None` when no
    /// message did (a schedule, a split's lost parent).
    pub fn started_on(&self, run: &Run) -> Result<Option<String>> {
        let mut cause = run.message.clone().or_else(|| run.parent.clone());
        while let Some(id) = cause {
            if let Some(parent) = self.run(&id)? {
                cause = parent.message.or(parent.parent);
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
    /// The runs under way with a step waiting for data, oldest first: the
    /// ones a message for the workspace reaches.
    pub fn waiting_runs(&self) -> Result<Vec<String>> {
        let mut s = self.db.prepare_cached(
            "SELECT id FROM runs WHERE ended IS NULL AND waiting = 1 ORDER BY started, id",
        )?;
        let out = s
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// The runs under way, apart from those of the workflows `apart`: read
    /// from the counts the journal keeps as runs start and end
    /// (`under_way`), one row a workflow, not counted run by run.
    pub fn active_count(&self, apart: &[String]) -> Result<usize> {
        let n: i64 = self
            .db
            .prepare_cached(
                "SELECT COALESCE(SUM(runs), 0) FROM under_way
                 WHERE workflow NOT IN (SELECT value FROM json_each(?1))",
            )?
            .query_row(params![json(&apart)?], |r| r.get(0))?;
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

    /// Gives the workspace another name; its id stays.
    pub fn rename_workspace(&self, name: &str) -> Result<()> {
        let mut me = self.workspace()?;
        me.name = name.to_string();
        self.db.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'workspace'",
            params![json(&me)?],
        )?;
        Ok(())
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
    /// Each action runs ended in error at, newest failure first, at most
    /// `limit`: its name (without `#`), how many runs it broke, and its
    /// newest failure's time, why and run name. Only those rows are read.
    pub fn breaks(&self, limit: usize) -> Result<Vec<Breaks>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut s = self.db.prepare(
            "SELECT substr(step, 2), errors, at, why, name FROM (
               SELECT json_extract(runtime, '$.failed_step') AS step,
                      COALESCE(ended, started) AS at,
                      json_extract(runtime, '$.why') AS why, name, started, id,
                      COUNT(*) OVER (PARTITION BY json_extract(runtime, '$.failed_step')) AS errors,
                      ROW_NUMBER() OVER (PARTITION BY json_extract(runtime, '$.failed_step')
                                         ORDER BY started DESC, id DESC) AS nth
               FROM runs
               WHERE json_extract(runtime, '$.ended.kind') = 'error'
                 AND json_extract(runtime, '$.failed_step') LIKE '#%')
             WHERE nth = 1 ORDER BY started DESC, id DESC LIMIT ?1",
        )?;
        let rows = s
            .query_map(params![limit], |r| {
                Ok(Breaks {
                    action: r.get(0)?,
                    errors: r.get::<_, i64>(1)? as usize,
                    last: r.get(2)?,
                    why: r.get(3)?,
                    run: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// The runs that ended in error at action `name` (without `#`), newest
    /// first, at most `limit`: when, workflow, run name, id, why.
    pub fn broken_at(&self, name: &str, limit: usize) -> Result<Vec<Broke>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut s = self.db.prepare(
            "SELECT COALESCE(ended, started), workflow, name, id, json_extract(runtime, '$.why')
             FROM runs
             WHERE json_extract(runtime, '$.ended.kind') = 'error'
               AND json_extract(runtime, '$.failed_step') = '#' || ?1
             ORDER BY started DESC, id DESC LIMIT ?2",
        )?;
        let rows = s
            .query_map(params![name, limit], |r| {
                Ok(Broke {
                    when: r.get(0)?,
                    workflow: r.get(1)?,
                    run: r.get(2)?,
                    id: r.get(3)?,
                    why: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

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

    /// Handles a message for the workspace, `r`: starts each of `starts`
    /// its windows let through (and remembers it), copies the message to
    /// each run in `waiting`, and marks it handled. One transaction, so a
    /// message taken again starts, copies and counts nothing twice. Nothing
    /// started and nothing copied is noted, not an error.
    pub fn route(
        &mut self,
        r: &Recorded,
        starts: &[Start],
        waiting: &[String],
        at: &str,
    ) -> Result<Routed> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let note = (starts.is_empty() && waiting.is_empty())
            .then_some("no workflow starts on it, and no run waits for it");
        // handled first: one cancelled (or handled) since it was read starts
        // nothing
        let marked = tx.execute(
            "UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL",
            params![r.seq, at, note],
        )?;
        if marked == 0 {
            return Ok(Routed::default());
        }
        let mut routed = Routed::default();
        'starts: for s in starts {
            // the windows that let through or hold back first, then the one
            // that keeps; an event kept counts for them, as one that starts
            let (keeps, passes): (Vec<&Gated>, Vec<&Gated>) =
                s.gates.iter().partition(|g| g.gate.keeps());
            for g in &passes {
                if let Some(why) = held_back(&tx, g, at)? {
                    routed.held.push((s.workflow.clone(), why));
                    continue 'starts;
                }
            }
            for g in &passes {
                let_through(&tx, g)?;
            }
            let input = match keeps.first() {
                None => None,
                Some(g) => match keep(&tx, g, &s.workflow, r.seq, at)? {
                    Some(input) => Some(input),
                    None => {
                        routed.kept.push(s.workflow.clone());
                        continue;
                    }
                },
            };
            let pin = pin(&tx, &s.workflow, &s.flows)?;
            let fresh = Runtime::default();
            let mut run = s.new_run(&pin, &fresh, at);
            if let Some(input) = &input {
                run.input = input;
            }
            routed.started.push(insert_run(&tx, run)?);
        }
        if routed.started.is_empty()
            && waiting.is_empty()
            && (!routed.held.is_empty() || !routed.kept.is_empty())
        {
            let mut why: Vec<String> = routed
                .held
                .iter()
                .map(|(w, why)| format!("{w}: {why}"))
                .collect();
            why.extend(
                routed
                    .kept
                    .iter()
                    .map(|w| format!("{w}: kept for a run of many")),
            );
            tx.execute(
                "UPDATE events SET note = ?2 WHERE seq = ?1",
                params![r.seq, format!("by a window: {}", why.join("; "))],
            )?;
        }
        for id in waiting {
            publish(&tx, &copy(&r.event, id, at))?;
        }
        tx.commit()?;
        Ok(routed)
    }

    /// The open batches whose time has come at `now`: each key and its
    /// workflow.
    pub fn batches_due(&self, now: &str) -> Result<Vec<(String, String)>> {
        let mut q = self.db.prepare(
            "SELECT key, workflow FROM batches WHERE closes IS NOT NULL AND closes <= ?1 ORDER BY closes, key",
        )?;
        let rows = q
            .query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// When the next open batch closes.
    pub fn next_batch_close(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT MIN(closes) FROM batches", [], |r| r.get(0))?)
    }

    /// Closes the batch `key`: starts `s` with the events it kept as its
    /// `$event`, in one transaction. None when it kept none (taken already).
    pub fn close_batch(&mut self, key: &str, s: &Start, at: &str) -> Result<Option<String>> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(input) = take_kept(&tx, key)? else {
            tx.commit()?;
            return Ok(None);
        };
        let pin = pin(&tx, &s.workflow, &s.flows)?;
        let fresh = Runtime::default();
        let mut run = s.new_run(&pin, &fresh, at);
        run.input = &input;
        let id = insert_run(&tx, run)?;
        tx.commit()?;
        Ok(Some(id))
    }

    /// Drops the batch `key` and the events it kept for no run (its
    /// workflow is gone or off).
    pub fn drop_batch(&mut self, key: &str) -> Result<usize> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute("DELETE FROM kept WHERE key = ?1", params![key])?;
        tx.execute("DELETE FROM batches WHERE key = ?1", params![key])?;
        tx.commit()?;
        Ok(n)
    }

    /// Why the window of `g` holds it back at `at`, by what it let through
    /// before; none when it lets it through. Nothing is written.
    pub fn held_back(&self, g: &Gated, at: &str) -> Result<Option<String>> {
        held_back(&self.db, g, at)
    }

    /// Forgets what windows let through that they no longer hold back for:
    /// `unique` values and `at_most` periods past their end at `now`.
    pub fn forget_seen(&self, now: &str) -> Result<usize> {
        Ok(self.db.execute(
            "DELETE FROM seen WHERE until IS NOT NULL AND until <= ?1",
            params![now],
        )?)
    }

    /// The events for the workspace no daemon has handled yet, oldest
    /// first: those held back for room under `limits.runs`, or waiting for
    /// a daemon.
    pub fn queued(&self) -> Result<Vec<Queued>> {
        let mut s = self.db.prepare(
            "SELECT id, type, time, data FROM events
             WHERE subject = 'itero.app' AND handled IS NULL ORDER BY seq",
        )?;
        let rows = s
            .query_map([], |r| {
                let data: String = r.get(3)?;
                Ok(Queued {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    time: r.get(2)?,
                    data: serde_json::from_str(&data).unwrap_or(Value::Null),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Cancels an event for the workspace that no daemon has handled yet:
    /// handled now, starting and reaching nothing, noted `CANCELLED`. One
    /// transaction, so a daemon routing it at once does one or the other.
    pub fn cancel(&mut self, id: &str, at: &str) -> Result<Cancelled> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: Option<(Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT handled, note FROM events WHERE id = ?1 AND subject = 'itero.app'",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let out = match found {
            None => Cancelled::Missing,
            Some((Some(_), note)) => {
                let mut s =
                    tx.prepare("SELECT id FROM runs WHERE message = ?1 ORDER BY started, id")?;
                let started = s
                    .query_map(params![id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                Cancelled::Handled { note, started }
            }
            Some((None, _)) => {
                // copied to runs that started to wait before a daemon
                // routed it: they have it, so it is not cancelled
                let mut s = tx.prepare(
                    "SELECT subject FROM events WHERE type = 'message' AND cause = ?1 ORDER BY seq",
                )?;
                let runs: Vec<String> = s
                    .query_map(params![id], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .iter()
                    .filter_map(|subject| run_of(subject).map(str::to_string))
                    .collect();
                drop(s);
                if runs.is_empty() {
                    tx.execute(
                        "UPDATE events SET handled = ?2, note = ?3 WHERE id = ?1 AND handled IS NULL",
                        params![id, at, CANCELLED],
                    )?;
                    Cancelled::Done
                } else {
                    Cancelled::Reached { runs }
                }
            }
        };
        tx.commit()?;
        Ok(out)
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
                "UPDATE runs SET runtime = ?2, waiting = ?5,
                 ended = CASE WHEN ?3 THEN COALESCE(ended, ?4) ELSE NULL END WHERE id = ?1",
                params![
                    id,
                    json(rt)?,
                    ended,
                    now,
                    !ended && super::steps::is_waiting(rt)
                ],
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
                params![seq, now, h.stopped_why],
            )?;
        }
        for e in &h.events {
            publish(&tx, e)?;
        }
        for g in &h.let_through {
            let_through(&tx, g)?;
        }
        for e in &h.shown {
            publish(&tx, e)?;
            tx.execute(
                "UPDATE events SET handled = ?2 WHERE id = ?1",
                params![e.id, now],
            )?;
        }
        if let Some(n) = &h.next_round {
            insert_run(
                &tx,
                NewRun {
                    workflow: n.workflow,
                    pin: n.pin,
                    rt: n.runtime,
                    input: n.input,
                    now,
                    cause: None,
                    message: n.message,
                    parent: n.parent,
                },
            )?;
        }
        if !h.children.is_empty() {
            let Some((parent, ..)) = h.run else {
                bail!("a run started by a step needs the run whose step it is");
            };
            let cause = tx.query_row(
                "SELECT id FROM events WHERE seq = ?1",
                params![h.seq],
                |r| r.get::<_, String>(0),
            )?;
            for c in &h.children {
                let pin = pin(&tx, c.workflow, c.flows)?;
                let rt = Runtime {
                    state: c.seed.clone(),
                    ..Runtime::default()
                };
                insert_run(
                    &tx,
                    NewRun {
                        workflow: c.workflow,
                        pin: &pin,
                        rt: &rt,
                        input: c.input,
                        now,
                        cause: Some(&cause),
                        message: None,
                        parent: Some((parent, &c.path)),
                    },
                )?;
            }
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
                   AND time < COALESCE((SELECT MIN(started) FROM runs WHERE ended IS NULL), '9999')
                   AND seq NOT IN (SELECT seq FROM kept)",
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

const RUN_COLUMNS: &str =
    "id, workflow, pin, runtime, input, started, ended, name, message, parent, parent_path";

fn run_row(r: &rusqlite::Row) -> rusqlite::Result<Result<Run>> {
    let (rt, input): (String, Option<String>) = (r.get(3)?, r.get(4)?);
    let (parent, parent_path): (Option<String>, Option<String>) = (r.get(9)?, r.get(10)?);
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
            parent,
            parent_path: parent_path.map(|t| serde_json::from_str(&t)).transpose()?,
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
    /// The run whose step started it, and that step's path.
    parent: Option<(&'a str, &'a [usize])>,
}

fn insert_run(db: &Connection, r: NewRun) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    db.execute(
        "INSERT INTO runs (id, name, workflow, pin, runtime, args, input, started, message, parent, parent_path, waiting)
         VALUES (?1, ?2, ?3, ?4, ?5, '{}', ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            crate::names::random(),
            r.workflow,
            r.pin,
            json(r.rt)?,
            json(r.input)?,
            r.now,
            r.message,
            r.parent.map(|(id, _)| id),
            r.parent.map(|(_, path)| json(&path)).transpose()?,
            super::steps::is_waiting(r.rt)
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

/// Works out `runs.waiting` for the runs under way, once, for a journal
/// from before it was kept.
fn mark_waiting(db: &Connection) -> Result<()> {
    let mut s = db.prepare("SELECT id, runtime FROM runs WHERE ended IS NULL")?;
    let runs: Vec<(String, String)> = s
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, rt) in runs {
        let rt: Runtime = serde_json::from_str(&rt)
            .with_context(|| format!("run {id}'s place in the journal"))?;
        db.execute(
            "UPDATE runs SET waiting = ?2 WHERE id = ?1",
            params![id, super::steps::is_waiting(&rt)],
        )?;
    }
    Ok(())
}
