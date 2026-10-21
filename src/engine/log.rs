//! `.itero/itero.log`: one JSON object per line, everything that happens.
//! Each line has `time`, `workspace`, `daemon` and `event`. The file is
//! opened for each line, so one renamed under it (rolled) is no trouble.
use crate::workspace::Workspace;
use serde_json::{Map, Value};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    workspace: String,
    daemon: String,
}

impl Log {
    /// The log of `ws`, whose identity is `workspace`, written by this
    /// daemon's life.
    pub fn new(ws: &Workspace, workspace: &str) -> Log {
        Log {
            path: ws.root.join(".itero").join("itero.log"),
            workspace: workspace.into(),
            daemon: super::daemon::Daemon::current().id.clone(),
        }
    }

    /// Writes one line: `event` and `fields` (an object). A line that
    /// cannot be written is said on stderr: the daemon goes on.
    pub fn write(&self, event: &str, fields: Value) {
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
