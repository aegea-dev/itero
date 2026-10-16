//! Sources: long-lived programs that turn what happens outside into
//! events. The protocol, the same for every source in any language:
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

/// `with.<key>` as a duration like `500ms`, or `default`.
fn duration(with: &Map<String, Value>, key: &str, default: Duration) -> Result<Duration> {
    match with.get(key) {
        None => Ok(default),
        Some(Value::String(s)) => {
            crate::workflow::expr::duration(s).map_err(|e| anyhow!("with.{key}: {e}"))
        }
        Some(other) => Err(anyhow!("with.{key} is a duration like 500ms, not {other}")),
    }
}

/// `watch@1`: a file or folder's changes, debounced (`with.quiet`, 500 ms
/// unless given; at most `with.max_wait`, 5 s), as events of type `emit`
/// with data `{path, kind}`.
pub struct Watch {
    emit: String,
    backend: Box<dyn crate::watch::Backend>,
    debouncer: crate::watch::Debouncer,
}

impl Watch {
    /// Checks the settings and sets the watch: what can go wrong goes wrong
    /// here, before anything runs.
    pub fn new(s: &Settings) -> Result<Watch> {
        let path = s
            .with
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("watch@1 needs with.path"))?;
        let emit = s
            .emit
            .clone()
            .ok_or_else(|| anyhow!("watch@1 needs emit"))?;
        let quiet = duration(&s.with, "quiet", Duration::from_millis(500))?;
        let max_wait = duration(&s.with, "max_wait", Duration::from_secs(5))?;
        let backend = crate::watch::open(std::path::Path::new(path))
            .with_context(|| format!("cannot watch {path}"))?;
        Ok(Watch {
            emit,
            backend,
            debouncer: crate::watch::Debouncer::new(quiet, max_wait),
        })
    }

    /// Prints the changes as events until the watch fails or `out` does.
    pub fn run(mut self, out: &mut dyn std::io::Write) -> Result<()> {
        let emit = self.emit;
        crate::watch::run(self.backend.as_mut(), &mut self.debouncer, |c| {
            writeln!(out, "{}", json!({ "type": emit, "data": c }))?;
            out.flush()
        })?;
        Ok(())
    }
}
