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

/// The built-in sources, at their versions.
pub const BUILT_IN: [&str; 1] = ["watch@1"];

/// A source's file, `sources/<name>.json`: a built-in (`source`) or a
/// program and its arguments (`program`), which may run in a container
/// (`image`); its settings (`with`, where `${env.NAME}` is a secret); and
/// the event type it sends (`emit`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub program: Vec<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub with: Map<String, Value>,
    #[serde(default)]
    pub emit: Option<String>,
}

fn check(d: &Definition) -> std::result::Result<(), String> {
    use crate::workflow::validate::{check_templates, is_word};
    match (&d.source, d.program.is_empty()) {
        (Some(_), true) | (None, false) => {}
        _ => return Err("a source is a built-in (source) or a program, one of them".into()),
    }
    if let Some(s) = &d.source {
        if !BUILT_IN.contains(&s.as_str()) {
            return Err(format!("no built-in source {s}"));
        }
        if d.image.is_some() {
            return Err("image is for a program source".into());
        }
        if !d.with.get("path").is_some_and(Value::is_string) {
            return Err("watch@1 needs with.path".into());
        }
        if d.emit.is_none() {
            return Err("watch@1 needs emit".into());
        }
    }
    if let Some(i) = &d.image
        && (i.trim().is_empty() || i.starts_with('-'))
    {
        return Err(format!("image \"{i}\" is not an image name"));
    }
    if let Some(e) = &d.emit {
        if !e.split('.').all(is_word) {
            return Err(format!("emit \"{e}\" is an event type, like mail.changed"));
        }
        if crate::engine::events::engine_own(e) {
            return Err(format!("emit \"{e}\" is the engine's own"));
        }
    }
    let mut texts = vec![];
    d.with
        .values()
        .for_each(|v| crate::workflow::validate::strings(v, &mut texts));
    for t in texts {
        check_templates(t)?;
    }
    Ok(())
}

/// Reads and checks a source's file; its name is the file's stem.
pub fn load_file(path: &std::path::Path) -> Result<Definition, crate::workflow::WorkflowError> {
    let err = |message: String| crate::workflow::WorkflowError {
        file: path.display().to_string(),
        at: None,
        message,
    };
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !crate::workflow::validate::is_word(stem) {
        return Err(err(format!(
            "a source's name \"{stem}\" is lowercase letters, digits and -"
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|e| err(format!("cannot read it: {e}")))?;
    let d: Definition = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
    check(&d).map_err(err)?;
    Ok(d)
}

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
