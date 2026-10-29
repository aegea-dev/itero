//! The workflow engine: the manager's pure decision for one run and one
//! event (`steps::decide`), the journal, the daemon's passes, and the
//! workers that run actions.
pub mod alarm;
pub mod daemon;
pub mod events;
pub mod http;
pub mod journal;
pub mod lock;
pub mod log;
pub mod progress;
pub mod running;
pub mod schedule;
pub mod secrets;
pub mod service;
pub mod steps;
pub mod stop;
pub mod watchdog;
pub mod worker;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Kills this process at the named point when `ITERO_TEST_DIE_AT` names
/// it: how the kill tests stop the daemon, a manager or a worker at one
/// exact step. With `ITERO_TEST_DIE_ONCE` (a file's path), only the first
/// process to get there dies, making that file first; with
/// `ITERO_TEST_DIE_WHILE` (a file's path), only while that file is there.
/// Unset, it does nothing.
pub fn die_at(point: &str) {
    if std::env::var("ITERO_TEST_DIE_AT").ok().as_deref() != Some(point) {
        return;
    }
    if std::env::var_os("ITERO_TEST_DIE_WHILE").is_some_and(|f| !std::path::Path::new(&f).exists())
    {
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

/// A timer: when it is due, the daemon sends `timer.fired` with `data` to
/// the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub at: String,
    pub data: Value,
}
