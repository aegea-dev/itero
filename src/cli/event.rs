//! `itero ws ev`: the events for the workspace no daemon has handled yet,
//! and taking one back.
use super::out::{show, table, text};
use crate::engine::journal::{Cancelled, Journal};
use crate::workspace::Workspace;
use anyhow::{Result, bail};

/// Each event not yet handled, oldest first: its id, type, time and data.
pub fn list(ws: &Workspace, json: bool) -> Result<()> {
    let q = Journal::open(ws)?.queued()?;
    show(json, &serde_json::to_value(&q)?, |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let data = e["data"].to_string();
                let data = match data.char_indices().nth(60) {
                    Some((i, _)) => format!("{}...", &data[..i]),
                    None => data,
                };
                vec![text(&e["id"]), text(&e["type"]), text(&e["time"]), data]
            })
            .collect();
        table(&rows);
    })
}

/// Cancels an event no daemon has handled yet: it starts and reaches
/// nothing.
pub fn cancel(ws: &Workspace, id: &str, json: bool) -> Result<()> {
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    match Journal::open(ws)?.cancel(id, &now)? {
        Cancelled::Done => show(json, &serde_json::json!({ "cancelled": id }), |_| {
            println!("cancelled {id}")
        }),
        Cancelled::Handled { note, started } => {
            let what = match (started.is_empty(), note) {
                (false, _) => format!("it started {}", started.join(", ")),
                (true, Some(n)) => n,
                (true, None) => "it reached the runs waiting for it".into(),
            };
            bail!("{id} was already handled: {what}")
        }
        Cancelled::Reached { runs } => {
            bail!("{id} has reached runs waiting for it: {}", runs.join(", "))
        }
        Cancelled::Missing => {
            bail!("no event {id} in this workspace: itero ws ev list lists those not yet handled")
        }
    }
}

/// The queue's ids, for the shell.
pub fn ids(ws: &Workspace) -> Vec<String> {
    Journal::open(ws)
        .and_then(|j| j.queued())
        .map(|q| q.into_iter().map(|e| e.id).collect())
        .unwrap_or_default()
}
