//! The workflows (`itero ws wf list`, `GET /workflows`): each one, what
//! starts it, its runs and the last; and whether a daemon takes events.
use super::Fault;
use crate::engine::journal::Journal;
use crate::engine::steps;
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct List {
    pub workflows: Vec<Workflow>,
    pub daemon: Daemon,
    /// Events no daemon has handled yet.
    pub waiting: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Daemon {
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    /// Its file, from the workspace; none for a built-in one.
    pub path: Option<String>,
    pub internal: bool,
    /// Its `on`: the events and the schedules.
    pub on: Value,
    /// What starts it, as text: `on order.placed`, `at 0 * * * *`.
    pub starts: Vec<String>,
    pub runs: usize,
    pub last: Option<Last>,
    /// `off`, `not now (from/until)`, `internal`.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Last {
    pub status: String,
    pub at: String,
}

/// Each workflow, the workspace's own first.
pub fn list(ws: &Workspace) -> Result<List, Fault> {
    let j = Journal::open(ws)?;
    let now = chrono::Utc::now();
    let mut flows = ws.flows()?;
    flows.sort_by_key(|(_, f)| f.meta.internal);
    let files = ws.flow_files()?;
    let mut out = vec![];
    for (name, f) in &flows {
        let on = &f.on;
        let mut starts: Vec<String> = on
            .events
            .iter()
            .map(|e| match e.args.is_empty() {
                true => format!("on {}", e.kind),
                false => format!("on {} {}", e.kind, e.key()),
            })
            .collect();
        starts.extend(on.schedules.iter().map(|s| format!("at {s}")));
        starts.extend(on.paths.iter().map(|p| format!("path {p}")));
        let mut notes = vec![];
        if f.meta.off {
            notes.push("off".to_string());
        } else if !crate::flow::active(&f.meta, now) {
            notes.push("not now (from/until)".to_string());
        }
        if f.meta.internal {
            notes.push("internal".to_string());
        }
        let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
        let last = runs.first().map(|r| Last {
            status: steps::status(&r.runtime).to_string(),
            at: r.ended.clone().unwrap_or_else(|| r.started.clone()),
        });
        out.push(Workflow {
            name: name.clone(),
            path: files.get(name).map(|f| ws.shown(f)),
            internal: f.meta.internal,
            on: serde_json::json!({
                "events": on.events,
                "schedules": on.schedules,
            }),
            starts,
            runs: runs.len(),
            last,
            notes,
        });
    }
    Ok(List {
        workflows: out,
        daemon: Daemon {
            running: crate::engine::lock::held(ws).map_err(anyhow::Error::msg)?,
        },
        waiting: j.waiting_messages()?,
    })
}
