//! The workflow engine: the manager's pure decision for one run and one
//! event (`manager::decide`), the journal, the daemon's passes, and the
//! workers that run actions.
pub mod builtins;
pub mod daemon;
pub mod eval;
pub mod http;
pub mod journal;
pub mod manager;
pub mod schedule;
pub mod secrets;
pub mod worker;

use crate::workflow::File;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Kills this process at the named point when `ITERO_TEST_DIE_AT` names
/// it: how the kill tests stop the daemon, a manager or a worker at one
/// exact step. With `ITERO_TEST_DIE_ONCE` (a file's path), only the first
/// process to get there dies, making that file first. Unset, it does
/// nothing.
pub fn die_at(point: &str) {
    if std::env::var("ITERO_TEST_DIE_AT").ok().as_deref() != Some(point) {
        return;
    }
    if let Some(once) = std::env::var_os("ITERO_TEST_DIE_ONCE")
        && std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(once)
            .is_err()
    {
        return;
    }
    #[cfg(unix)]
    // SAFETY: signals only this process, as `kill -9` from outside would
    unsafe {
        libc::kill(libc::getpid(), libc::SIGKILL);
    }
    std::process::abort();
}

/// An event, as the manager sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: Value,
}

/// An event the manager asks the daemon to send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutEvent {
    #[serde(rename = "type")]
    pub kind: String,
    /// `worker`, or a run's id.
    pub target: String,
    #[serde(default)]
    pub data: Value,
}

/// A timer: when it is due, the daemon sends `timer.fired` with `data` to
/// the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub at: String,
    pub data: Value,
}

/// A transition whose chain of actions is running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    /// Names this chain: action results and timers carry it.
    pub id: String,
    /// The transition, by its place in the workflow.
    pub transition: usize,
    /// The action a worker is running.
    pub index: usize,
    /// The data of the event that started it.
    pub input: Value,
    /// The last finished action's output.
    pub prev: Value,
    /// The outputs of the actions with an `id`, by id.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub steps: Map<String, Value>,
    /// Memory changes held back until the chain succeeds.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub memory: Map<String, Value>,
    /// Events held back until the chain succeeds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<OutEvent>,
}

/// Where a failed run was, to resume from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failed {
    pub from: String,
    pub transition: usize,
    pub chain: String,
    pub input: Value,
}

/// A run's place in its workflow, kept by the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Runtime {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<Chain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<Failed>,
}

/// Everything the manager is given for one decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// The time, RFC 3339: the manager never reads a clock.
    pub now: String,
    /// The run's workflow, at its pinned version.
    pub workflow: File,
    /// The run: its `id`, `workflow` and `started`.
    pub run: Map<String, Value>,
    /// The arguments its trigger gave it.
    #[serde(default)]
    pub args: Map<String, Value>,
    /// The workflow's memory, as earlier runs left it.
    #[serde(default)]
    pub memory: Map<String, Value>,
    pub runtime: Runtime,
    pub event: Event,
}

/// What the manager decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// The run's place after the event.
    pub runtime: Runtime,
    /// Keys of the workflow's memory to set now.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub memory: Map<String, Value>,
    /// Events to send.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<OutEvent>,
    /// The run's timers from now on; none means unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timers: Option<Vec<Timer>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log: Vec<String>,
}

/// One action for a worker: what `itero worker` reads on stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub action: String,
    #[serde(default)]
    pub with: Map<String, Value>,
    /// The idempotency key: the same when the action runs again.
    pub key: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub prev: Value,
    #[serde(default)]
    pub run: Map<String, Value>,
    #[serde(default)]
    pub memory: Map<String, Value>,
    #[serde(default)]
    pub args: Map<String, Value>,
    /// The workspace's folder: where `command@1` runs.
    pub root: std::path::PathBuf,
    /// For a workspace action: the program that runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
    /// Seconds it may take (the daemon kills it after that).
    pub timeout: u64,
    /// Seconds its lease lasts: it renews it by touching `ITERO_HEARTBEAT`,
    /// or the daemon kills it and runs it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<u64>,
}

/// A workspace action as a program: given the job on stdin, it prints
/// its output as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    /// Where it runs.
    pub folder: std::path::PathBuf,
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
    /// For `python@` and `bun@`: what to prepare and call; the program is
    /// then the interpreter itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Interpreter>,
}

/// The interpreter a workspace action's function runs in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Interpreter {
    Python {
        version: String,
        /// The virtual environment, relative to the folder.
        home: String,
        requirements: Option<String>,
        module: String,
        callable: String,
    },
    Bun {
        file: String,
        export: String,
    },
}
