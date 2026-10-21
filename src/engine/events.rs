//! Messages from outside (an app, a script, a person): checked, then
//! written to the journal and nothing more. The daemon's next pass takes
//! them from there, so a message sent while it is stopped waits for it.
use super::journal::{self, Envelope, Journal};
use crate::workspace::Workspace;
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum EmitError {
    /// The message itself is wrong.
    Bad(String),
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EmitError::Bad(m) | EmitError::Internal(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EmitError {}

/// The start of the types of the daemon's own messages, `itero.run.error`:
/// workflows may run on them, and only the daemon sends them.
pub const OWN: &str = "itero.";

/// Types the engine sends itself: from outside they would forge a worker's
/// result, a timer, a run's start, a source's state or the daemon's own
/// message.
pub fn engine_own(kind: &str) -> bool {
    ["action.", "timer.", "run.", "workflow.", "source.", OWN]
        .iter()
        .any(|p| kind.starts_with(p))
}

/// A watched path changed: `{"path", "kind"}`. It starts the workflows
/// whose `on` names the path.
pub const PATH_CHANGED: &str = "itero.path.changed";

/// One of the daemon's own messages, for the workspace.
pub fn own(kind: &str, data: Value, now: &str) -> Envelope {
    debug_assert!(kind.starts_with(OWN));
    Envelope::new("daemon", kind, &journal::subject("app"), now, None, data)
}

/// What a message will do, as the workflows on disk say: the workflows it
/// starts, how many runs wait (it may reach them), and, when nothing
/// starts on its type, the nearest type something does start on.
#[derive(Debug, Default, PartialEq)]
pub struct Fate {
    pub starts: Vec<String>,
    pub waiting: usize,
    pub nearest: Option<String>,
}

pub fn fate(ws: &Workspace, kind: &str, data: &Map<String, Value>) -> anyhow::Result<Fate> {
    let now = chrono::Utc::now();
    let defined = |n: &str| super::secrets::get(n).is_some();
    let flows = ws.flows()?;
    let starts = flows
        .iter()
        .filter(|(_, f)| {
            f.on.events.iter().any(|e| e == kind)
                && crate::flow::active(&f.meta, now)
                && crate::flow::holds(&f.on, data, &defined)
        })
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>();
    let waiting = Journal::open(ws)?
        .active_runs()?
        .iter()
        .filter(|r| super::steps::is_waiting(&r.runtime))
        .count();
    // a type a letter or three away is a typo
    let nearest = if starts.is_empty() {
        flows
            .iter()
            .flat_map(|(_, f)| f.on.events.iter())
            .map(|e| (distance(e, kind), e))
            .filter(|(d, _)| (1..=3).contains(d))
            .min()
            .map(|(_, e)| e.clone())
    } else {
        None
    };
    Ok(Fate {
        starts,
        waiting,
        nearest,
    })
}

/// The edits (a letter added, removed or changed) from `a` to `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// Saves a message for the workspace, where it may start runs and reach
/// waiting ones; returns its id.
pub fn emit(
    ws: &Workspace,
    kind: &str,
    data: Option<Value>,
    now: &str,
) -> Result<String, EmitError> {
    if !crate::flow::is_message_type(kind) {
        return Err(EmitError::Bad(format!(
            "a message type is two or more words joined by dots, like order.placed, not {kind}"
        )));
    }
    if engine_own(kind) {
        return Err(EmitError::Bad(format!(
            "{kind} is the engine's own: it cannot be sent from outside"
        )));
    }
    let data = match data {
        None => Value::Object(Map::new()),
        Some(v @ Value::Object(_)) => v,
        Some(_) => return Err(EmitError::Bad("data must be a JSON object".into())),
    };
    let internal = |e: anyhow::Error| EmitError::Internal(format!("{e:#}"));
    let j = Journal::open(ws).map_err(internal)?;
    let e = Envelope::new("outside", kind, &journal::subject("app"), now, None, data);
    j.publish(&e).map_err(internal)?;
    Ok(e.id)
}
