//! `.itero/itero.log`: one JSON object per line, everything that happens.
//! Each line has `time`, `workspace`, `daemon` and `event`. The file is
//! kept open, one per path in the process, and each line is one write:
//! after `#roll-log` it is opened anew at once ([`Log::rolled`]), and one
//! renamed by something else is found within a second. The daemon's last
//! line is `daemon.stopped`: no line of this process comes after it.
use crate::workspace::Workspace;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    workspace: String,
    daemon: String,
    lines: &'static Lines,
}

/// Held while a line is written: whether the last lines are written,
/// after which nothing is, and the files open.
#[derive(Debug)]
struct Lines(Mutex<State>);

#[derive(Debug)]
struct State {
    ended: bool,
    open: BTreeMap<PathBuf, Open>,
}

/// A log's file, open, and when its path was last looked at.
#[derive(Debug)]
struct Open {
    file: std::fs::File,
    looked: Instant,
}

/// How often an open log's path is looked at: renamed (by logrotate, say),
/// it is opened anew.
const LOOK: Duration = Duration::from_secs(1);

impl State {
    /// Writes `text` to the log at `path`, opening it when it is not open,
    /// or was renamed or removed since it was opened.
    fn write(&mut self, path: &Path, text: &[u8]) -> std::io::Result<()> {
        if let Some(o) = self.open.get_mut(path)
            && o.looked.elapsed() >= LOOK
        {
            o.looked = Instant::now();
            if !same_file(&o.file, path) {
                self.open.remove(path);
            }
        }
        let o = match self.open.entry(path.to_path_buf()) {
            std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::btree_map::Entry::Vacant(v) => v.insert(Open {
                file: std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?,
                looked: Instant::now(),
            }),
        };
        let written = o.file.write_all(text);
        if written.is_err() {
            // opened anew for the next line
            self.open.remove(path);
        }
        written
    }
}

/// Whether `path` is still the file `file` is.
fn same_file(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// The process's lines: every Log in it shares them, each pass's own too.
static LINES: Lines = Lines::new();

impl Lines {
    const fn new() -> Lines {
        Lines(Mutex::new(State {
            ended: false,
            open: BTreeMap::new(),
        }))
    }

    /// Writes `event` as the last line of each of `logs`, once a line
    /// under way is written; nothing is written after them.
    fn end(&self, logs: &[Log], event: &str) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for log in logs {
            let text = log.line(event, Value::Object(Map::new()));
            log.append(&mut state, &text);
        }
        state.ended = true;
        // nothing is written after: closed
        state.open.clear();
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
        let mut state = self.lines.0.lock().unwrap_or_else(PoisonError::into_inner);
        // stamped while held: the file's lines are in the order of their times
        if !state.ended {
            let text = self.line(event, fields);
            self.append(&mut state, &text);
        }
    }

    /// Its file was rolled (renamed): the next line opens it anew.
    pub fn rolled(&self) {
        let mut state = self.lines.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.open.remove(&self.path);
    }

    /// The line of `event` and `fields`, its time now.
    fn line(&self, event: &str, fields: Value) -> String {
        let mut line = Map::new();
        // to the microsecond: what happens within a millisecond is told apart
        let time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        line.insert("time".into(), time.into());
        line.insert("workspace".into(), self.workspace.clone().into());
        line.insert("daemon".into(), self.daemon.clone().into());
        line.insert("event".into(), event.into());
        if let Value::Object(f) = fields {
            line.extend(f);
        }
        Value::Object(line).to_string() + "\n"
    }

    fn append(&self, state: &mut State, text: &str) {
        if let Err(e) = state.write(&self.path, text.as_bytes()) {
            crate::say!("itero: cannot write {}: {e}", self.path.display());
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

    /// A log of its own lines, at `path`.
    fn own(path: &std::path::Path) -> Log {
        Log {
            path: path.to_path_buf(),
            workspace: "bir".into(),
            daemon: "gündüz".into(),
            lines: Box::leak(Box::new(Lines::new())),
        }
    }

    fn numbers(f: &std::path::Path) -> Vec<u64> {
        std::fs::read_to_string(f)
            .unwrap_or_default()
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["sıra"]
                    .as_u64()
                    .unwrap()
            })
            .collect()
    }

    /// Rolled by `#roll-log` (`rolled`), the next line goes to the new file.
    #[test]
    fn after_a_roll_the_next_line_is_in_the_new_file() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("itero.log");
        let log = own(&path);
        log.write("satır", json!({ "sıra": 0 }));
        std::fs::rename(&path, d.path().join("itero.log.1")).unwrap();
        log.rolled();
        log.write("satır", json!({ "sıra": 1 }));
        assert_eq!(numbers(&d.path().join("itero.log.1")), [0]);
        assert_eq!(numbers(&path), [1]);
    }

    /// Renamed by something else (logrotate), the log is found renamed
    /// within a second: lines until then go to the renamed file, none is
    /// lost and none written twice.
    #[test]
    fn a_log_renamed_by_another_is_written_anew_within_a_second() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("itero.log");
        let log = own(&path);
        log.write("satır", json!({ "sıra": 0 }));
        std::fs::rename(&path, d.path().join("eski.log")).unwrap();
        let since = std::time::Instant::now();
        let mut n = 0;
        while numbers(&path).is_empty() {
            assert!(
                since.elapsed() < std::time::Duration::from_secs(3),
                "not found in 3 s"
            );
            n += 1;
            log.write("satır", json!({ "sıra": n }));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let (old, new) = (numbers(&d.path().join("eski.log")), numbers(&path));
        let all: Vec<u64> = old.iter().chain(&new).copied().collect();
        assert_eq!(all, (0..=n).collect::<Vec<_>>());
        assert_eq!(new, [n]);
    }

    /// The file is kept open: a line costs a write, not an open.
    #[test]
    fn the_log_is_kept_open_between_lines() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("itero.log");
        let log = own(&path);
        log.write("satır", json!({ "sıra": 0 }));
        // renamed at once: a line that opened its file anew would make a
        // new one
        std::fs::rename(&path, d.path().join("aynı.log")).unwrap();
        log.write("satır", json!({ "sıra": 1 }));
        assert_eq!(numbers(&d.path().join("aynı.log")), [0, 1]);
    }
}
