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

/// One of the daemon's own messages, for the workspace.
pub fn own(kind: &str, data: Value, now: &str) -> Envelope {
    debug_assert!(kind.starts_with(OWN));
    Envelope::new("daemon", kind, &journal::subject("app"), now, None, data)
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
