//! `itero ws ev`: the events for the workspace no daemon has handled yet,
//! and taking one back.
use super::out::{show, table, text};
use crate::answer::Client;
use crate::engine::journal::Journal;
use crate::workspace::Workspace;
use anyhow::Result;

/// Each event not yet handled, oldest first: its id, type, time and data.
pub fn list(c: &Client, json: bool) -> Result<()> {
    let q = c.queued()?;
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
pub fn cancel(c: &Client, id: &str, json: bool) -> Result<()> {
    let done = c.cancel(id)?;
    show(json, &serde_json::to_value(&done)?, |_| {
        println!("cancelled {id}")
    })
}

/// The queue's ids, for the shell.
pub fn ids(ws: &Workspace) -> Vec<String> {
    Journal::open(ws)
        .and_then(|j| j.queued())
        .map(|q| q.into_iter().map(|e| e.id).collect())
        .unwrap_or_default()
}
