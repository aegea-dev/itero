//! `.itero/itero.log`: one JSON object per line, everything that happens.
//! Each line has `time`, `workspace`, `daemon` and `event`. The file is
//! opened for each line, so one renamed under it (rolled) is no trouble.
//! The daemon's last line is `daemon.stopped`: no line of this process
//! comes after it.
use crate::workspace::Workspace;
use serde_json::{Map, Value};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    workspace: String,
    daemon: String,
    lines: &'static Lines,
}

/// Held while a line is written: whether the last lines are written,
/// after which nothing is.
#[derive(Debug)]
struct Lines(Mutex<bool>);

/// The process's lines: every Log in it shares them, each pass's own too.
static LINES: Lines = Lines::new();

impl Lines {
    const fn new() -> Lines {
        Lines(Mutex::new(false))
    }

    /// Writes `event` as the last line of each of `logs`, once a line
    /// under way is written; nothing is written after them.
    fn end(&self, logs: &[Log], event: &str) {
        let mut ended = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for log in logs {
            log.append(event, Value::Object(Map::new()));
        }
        *ended = true;
    }
}

/// The daemon's last line, `event`, in each workspace's log: a line under
/// way is written first, and none after it in this process. Lines that
/// come later are not written; the journal keeps what the passes saved.
pub fn last(logs: &[Log], event: &str) {
    LINES.end(logs, event);
}

impl Log {
    /// The log of the workspace at `root`, for a step writing to it from
    /// its worker; none outside a workspace.
    pub fn of_root(root: &std::path::Path) -> Option<Log> {
        let ws = crate::workspace::find(root).ok()?;
        let id = super::journal::Journal::open(&ws)
            .and_then(|j| j.workspace())
            .ok()?;
        Some(Log::new(&ws, &id.id))
    }

    /// The log of `ws`, whose identity is `workspace`, written by this
    /// daemon's life.
    pub fn new(ws: &Workspace, workspace: &str) -> Log {
        Log {
            path: ws.root.join(".itero").join("itero.log"),
            workspace: workspace.into(),
            daemon: super::daemon::Daemon::current().id.clone(),
            lines: &LINES,
        }
    }

    /// Writes one line: `event` and `fields` (an object), unless the last
    /// lines are written. A line that cannot be written is said on
    /// stderr: the daemon goes on.
    pub fn write(&self, event: &str, fields: Value) {
        let ended = self.lines.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !*ended {
            self.append(event, fields);
        }
    }

    fn append(&self, event: &str, fields: Value) {
        let mut line = Map::new();
        line.insert(
            "time".into(),
            super::daemon::stamp(chrono::Utc::now()).into(),
        );
        line.insert("workspace".into(), self.workspace.clone().into());
        line.insert("daemon".into(), self.daemon.clone().into());
        line.insert("event".into(), event.into());
        if let Value::Object(f) = fields {
            line.extend(f);
        }
        let text = Value::Object(line).to_string() + "\n";
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = written {
            eprintln!("itero: cannot write {}: {e}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// After the daemon's last line nothing is written: by the logs it
    /// was given, or by a pass's own copy of one.
    #[test]
    fn nothing_is_written_after_the_last_line() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("itero.log");
        // its own lines: the process's would end every other test's log
        let lines: &'static Lines = Box::leak(Box::new(Lines::new()));
        let log = |workspace: &str| Log {
            path: path.clone(),
            workspace: workspace.into(),
            daemon: "gündüz".into(),
            lines,
        };
        let (bir, iki) = (log("bir"), log("iki"));
        bir.write("run.started", json!({ "workflow": "çalış" }));
        lines.end(&[bir.clone(), iki.clone()], "daemon.stopped");
        bir.write("run.ended", json!({ "workflow": "çalış" }));
        iki.write("step.done", json!({}));
        log("bir").write("run.ended", json!({}));
        let written: Vec<(String, String)> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .map(|l| (l["workspace"].to_string(), l["event"].to_string()))
            .collect();
        let want = [
            ("bir", "run.started"),
            ("bir", "daemon.stopped"),
            ("iki", "daemon.stopped"),
        ]
        .map(|(w, e)| (format!("\"{w}\""), format!("\"{e}\"")));
        assert_eq!(written, want);
    }
}
