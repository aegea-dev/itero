//! Workflows: templates of steps that run again and again, as JSON files
//! (`workflows/<name>.json`): states, and transitions between them that run
//! chains of actions. A run is one instance of a workflow. This module
//! reads, checks and versions them; running them is the engine's job.
pub mod actions;
pub mod expr;
pub mod format;
pub mod validate;

use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

pub use format::File;

/// The engine's own state: a transition failed. Never declared.
pub const FAILED: &str = "failed";

/// The `from` of a transition from any state.
pub const ANY: &str = "*";

/// A problem with a workflow or action file, naming the file (when there is
/// one) and, when it is about one part, that part (`transition "a -> b"`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowError {
    pub file: String,
    pub at: Option<String>,
    pub message: String,
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.file.is_empty() {
            write!(f, "{}: ", self.file)?;
        }
        match &self.at {
            Some(at) => write!(f, "{at}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for WorkflowError {}

fn parse_named(file: &str, text: &str) -> Result<File, WorkflowError> {
    let f: File = serde_json::from_str(text).map_err(|e| WorkflowError {
        file: file.to_string(),
        at: None,
        message: e.to_string(),
    })?;
    validate::validate(file, &f)?;
    Ok(f)
}

/// Parses and checks a workflow's text. Project actions are checked
/// against the workspace's actions folders later (`validate::actions`).
pub fn parse(text: &str) -> Result<File, WorkflowError> {
    parse_named("", text)
}

/// Reads, parses and checks a workflow file; its name must be the file's.
pub fn load_file(path: &Path) -> Result<File, WorkflowError> {
    let file = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| WorkflowError {
        file: file.clone(),
        at: None,
        message: format!("cannot read it: {e}"),
    })?;
    let f = parse_named(&file, &text)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if f.name != stem {
        return Err(WorkflowError {
            file,
            at: None,
            message: format!("name \"{}\" must match the file name \"{stem}\"", f.name),
        });
    }
    Ok(f)
}

/// A workflow's version: a SHA-256 over its canonical form (JSON with
/// sorted keys, every description removed). Formatting, descriptions,
/// triggers, concurrency and pause_after never count: when and how often a
/// workflow runs is the workspace's business, not part of what a run is
/// pinned to.
pub fn version(f: &File) -> String {
    let mut f = f.clone();
    f.description.clear();
    f.triggers.clear();
    f.concurrency = None;
    f.pause_after = None;
    for t in &mut f.transitions {
        t.description.clear();
        for a in &mut t.actions {
            a.description.clear();
        }
    }
    // serde_json's maps are sorted (no preserve_order), so this is canonical
    let text = serde_json::to_string(&f).expect("a workflow serializes");
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `<name>@<version>`: what a run is pinned to.
pub fn pin(f: &File) -> String {
    format!("{}@{}", f.name, version(f))
}

/// `mail-check@f2c...` is ("mail-check", "f2c...").
pub fn split_pin(pin: &str) -> (&str, &str) {
    pin.split_once('@').unwrap_or((pin, ""))
}

/// A version as people see it: its first 12 characters.
pub fn short(version: &str) -> &str {
    &version[..version.len().min(12)]
}
