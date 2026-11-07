//! A `#browser` step run as the worker runs it, on a tab of a test's
//! Chrome, in a workspace of its own (its log is read by some tests).
#![allow(dead_code)]
use itero::browser::cdp;
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::{Map, Value};
use std::path::PathBuf;

pub struct Tab {
    pub ws: String,
    pub root: tempfile::TempDir,
}

pub fn tab(port: u16) -> Tab {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("itero.config.json"), "{}").unwrap();
    Tab {
        ws: cdp::new_window(port, "about:blank").unwrap().ws,
        root,
    }
}

/// The step in a headless Chrome.
pub fn step(t: &Tab, args: Value) -> Result<Value, String> {
    step_as(t, "headless", args, &|_| ())
}

/// The step as if its Chrome were in `mode`, what it says given to
/// `told`.
pub fn step_as(
    t: &Tab,
    mode: &str,
    args: Value,
    told: &(dyn Fn(Option<&str>) + Sync),
) -> Result<Value, String> {
    let mut input = Map::new();
    input.insert("args".into(), args);
    let r = StepRun {
        file: PathBuf::from(worker::BROWSER),
        root: t.root.path().to_path_buf(),
        workflow: "tarayıcı".into(),
        env: [
            ("ITERO_BROWSER_TAB", t.ws.as_str()),
            ("ITERO_BROWSER_MODE", mode),
            ("ITERO_BROWSER_RUN", "koşu"),
            ("ITERO_BROWSER_STEP", "2"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
        input,
        memory: t.root.path().join("hafıza"),
        waiting_since: None,
        quiet: 60,
        most: 60,
        now: String::new(),
    };
    match worker::run_step_telling(&r, told) {
        Ok(Outcome::Done(v)) => Ok(v),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e.to_string()),
    }
}

/// The workspace's log lines of `event`.
pub fn logged(t: &Tab, event: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(t.root.path().join(".itero/itero.log")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|l| l["event"] == event)
        .collect()
}
