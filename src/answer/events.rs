//! Events: the queue (`itero ws ev list`, `GET /events`), taking one
//! back (`itero ws ev cancel`, `DELETE /events/{id}`) and sending one
//! (`itero ws emit`, `POST /events`).
use super::Fault;
use crate::engine::events::{self, EmitError, Waited};
use crate::engine::journal::{Cancelled as Undone, Journal, Queued};
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::time::Duration;

/// The events no daemon has handled yet, oldest first.
pub fn queued(ws: &Workspace) -> Result<Vec<Queued>, Fault> {
    Ok(Journal::open(ws)?.queued()?)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cancelled {
    pub cancelled: String,
}

/// Takes back an event no daemon has handled yet: it starts and reaches
/// nothing.
pub fn cancel(ws: &Workspace, id: &str) -> Result<Cancelled, Fault> {
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    match Journal::open(ws)?.cancel(id, &now)? {
        Undone::Done => Ok(Cancelled {
            cancelled: id.to_string(),
        }),
        Undone::Handled { note, started } => {
            let what = match (started.is_empty(), &note) {
                (false, _) => format!("it started {}", started.join(", ")),
                (true, Some(n)) => n.clone(),
                (true, None) => "it reached the runs waiting for it".into(),
            };
            let mut why = Map::new();
            why.insert("note".into(), json!(note));
            why.insert("started".into(), json!(started));
            Err(Fault::Conflict(
                format!("{id} was already handled: {what}"),
                why,
            ))
        }
        Undone::Reached { runs } => {
            let message = format!("{id} has reached runs waiting for it: {}", runs.join(", "));
            let mut why = Map::new();
            why.insert("reached".into(), json!(runs));
            Err(Fault::Conflict(message, why))
        }
        Undone::Missing => Err(Fault::Missing(format!(
            "no event {id} in this workspace: itero ws ev list lists those not yet handled"
        ))),
    }
}

/// Wakes the daemon of `ws` from another process: saved, it takes the
/// event at its next start otherwise.
pub fn nudge(ws: &Workspace) {
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    if let Err(err) = events::nudge(ws, &now) {
        eprintln!(
            "itero: saved, but could not wake the daemon ({}: {err}); it takes the message at its next start or wake",
            ws.wake_file().display()
        );
    }
}

/// An event to send.
/// The longest `POST /events?wait=` waits for its runs, in seconds: the
/// route's cap, which a context's CLI says before it sends.
pub const MAX_WAIT: u64 = 600;

#[derive(Debug, Clone, PartialEq)]
pub struct Emit {
    pub kind: String,
    pub data: Option<Value>,
    /// Wait this long for the runs it starts to end, and answer with them.
    pub wait: Option<Duration>,
}

/// What sending an event answers: what it will do, or, when waited for,
/// the runs it started.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Emitted {
    Waited(WaitedFor),
    Sent(Sent),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sent {
    pub id: String,
    /// The workflows it starts, from the files on disk.
    pub starts: Vec<String>,
    /// The runs waiting for data it may reach.
    pub waiting: usize,
    /// When it starts nothing: the nearest type that starts something.
    pub nearest: Option<String>,
    pub daemon: DaemonSays,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonSays {
    /// Whether a daemon runs the workspace.
    pub running: bool,
    /// When none does: what this machine's daemon says of it (the CLI's
    /// own note, about the machine it runs on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitedFor {
    pub id: String,
    pub message: String,
    /// The runs it started, each as it ended (or is, when time ran out).
    pub runs: Vec<Value>,
    /// `done`, `failed` (one failed or ended in error) or `timed out`.
    pub waited: String,
}

impl WaitedFor {
    pub fn how(&self) -> Waited {
        match self.waited.as_str() {
            "done" => Waited::Done,
            "failed" => Waited::Failed,
            _ => Waited::TimedOut,
        }
    }
}

fn not_sent(e: EmitError) -> Fault {
    match e {
        EmitError::Bad(m) => Fault::Bad(m),
        EmitError::Nowhere(m) => Fault::Conflict(m, Map::new()),
        EmitError::Internal(m) => Fault::Internal(anyhow::anyhow!(m)),
    }
}

/// Sends `e` to the workspace, saved at once, then `wake`s its daemon (as
/// the caller can: the daemon's own loop, or `nudge` from outside) before
/// any waiting.
pub fn emit(ws: &Workspace, e: &Emit, wake: impl FnOnce()) -> Result<Emitted, Fault> {
    let running = || crate::engine::lock::held(ws).map_err(|m| Fault::Internal(anyhow::anyhow!(m)));
    // with no daemon nothing would answer: say so, sending nothing
    if e.wait.is_some() && !running()? {
        return Err(Fault::Conflict(
            "no daemon is running: start itero daemon (or itero ws add, with the machine's daemon), then send it again".into(),
            Map::new(),
        ));
    }
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    let id = events::emit(ws, &e.kind, e.data.clone(), &now).map_err(not_sent)?;
    wake();
    if let Some(timeout) = e.wait {
        let (answer, how) = events::wait_for(ws, &id, timeout)?;
        return Ok(Emitted::Waited(WaitedFor {
            id: id.clone(),
            message: answer["message"].as_str().unwrap_or(&id).to_string(),
            runs: answer["runs"].as_array().cloned().unwrap_or_default(),
            waited: match how {
                Waited::Done => "done",
                Waited::Failed => "failed",
                Waited::TimedOut => "timed out",
            }
            .into(),
        }));
    }
    let fields = e
        .data
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let fate = events::fate(ws, &e.kind, &fields)?;
    Ok(Emitted::Sent(Sent {
        id,
        starts: fate.starts,
        waiting: fate.waiting,
        nearest: fate.nearest,
        daemon: DaemonSays {
            running: running()?,
            machine: None,
        },
    }))
}
