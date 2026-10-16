//! Events from outside (an app, a script, a person): checked, then written
//! to the journal and nothing more. The daemon's next pass takes them from
//! there, so an event sent while it is stopped waits for it.
use super::journal::{self, Envelope, Journal};
use crate::workflow::validate::is_word;
use crate::workspace::Workspace;
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum EmitError {
    /// The event itself is wrong.
    Bad(String),
    /// It names a run there is not.
    NoRun(String),
    /// It names a run that has ended.
    Ended(String),
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EmitError::Bad(m)
            | EmitError::NoRun(m)
            | EmitError::Ended(m)
            | EmitError::Internal(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EmitError {}

/// Types the engine sends itself: from outside they would forge a worker's
/// result, a timer, a run's start, a workflow's pause or a source's state.
pub fn engine_own(kind: &str) -> bool {
    ["action.", "timer.", "run.", "workflow.", "source."]
        .iter()
        .any(|p| kind.starts_with(p))
}

/// Saves an event for a run, or (with no run) for the workspace, where it
/// may start runs; returns its id.
pub fn emit(
    ws: &Workspace,
    kind: &str,
    data: Option<Value>,
    run: Option<&str>,
    now: &str,
) -> Result<String, EmitError> {
    if !kind.split('.').all(is_word) {
        return Err(EmitError::Bad(format!(
            "an event type is lowercase words joined by dots, like photo.added, not {kind}"
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
    let target = match run {
        None => "app".to_string(),
        Some(id) => {
            let r = j
                .run(id)
                .map_err(internal)?
                .ok_or_else(|| EmitError::NoRun(format!("no run {id}")))?;
            if r.ended.is_some() {
                return Err(EmitError::Ended(format!("run {id} has ended")));
            }
            id.to_string()
        }
    };
    let e = Envelope::new("outside", kind, &journal::subject(&target), now, None, data);
    j.publish(&e).map_err(internal)?;
    Ok(e.id)
}
