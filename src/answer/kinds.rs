//! Actions and connectors: what there is (`itero ws ac list`, `GET
//! /actions`; `itero ws cn list`, `GET /connectors`) and which actions
//! break runs (`itero ws ac errors`, `GET /actions/errors`).
use super::Fault;
use crate::engine::journal::Journal;
use crate::engine::worker::BUILT_IN;
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// An action: its name and its files (one per version).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub name: String,
    pub files: Vec<String>,
    pub built_in: bool,
}

/// Each action, the built-in ones too.
pub fn actions(ws: &Workspace) -> Result<Vec<Action>, Fault> {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, path) in ws.action_files()? {
        let name = file.split('@').next().unwrap_or(&file).to_string();
        by_name.entry(name).or_default().push(ws.shown(&path));
    }
    let mut out: Vec<Action> = by_name
        .into_iter()
        .map(|(name, files)| Action {
            name,
            files,
            built_in: false,
        })
        .collect();
    for b in BUILT_IN {
        if !out.iter().any(|a| a.name == b) {
            out.push(Action {
                name: b.to_string(),
                files: vec![],
                built_in: true,
            });
        }
    }
    Ok(out)
}

/// A connector, once for each set of args the workflows give it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    /// The type of the events it sends.
    #[serde(rename = "type")]
    pub kind: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Map<String, Value>>,
}

pub fn connectors(ws: &Workspace) -> Result<Vec<Connector>, Fault> {
    Ok(crate::connectors::copies(ws)?
        .into_iter()
        .map(|c| Connector {
            kind: c.listen.kind,
            file: ws.shown(&c.file),
            args: (!c.listen.args.is_empty()).then_some(c.listen.args),
        })
        .collect())
}

/// An action and how it breaks runs: how often, when and why last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Breaks {
    pub action: String,
    pub errors: usize,
    pub last: String,
    pub why: Option<String>,
    /// The last run it broke, by name.
    pub run: String,
}

/// A run an action broke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Broke {
    pub when: String,
    pub workflow: String,
    pub run: String,
    pub id: String,
    pub why: Option<String>,
}

/// The runs that ended in error, newest first, each with the action it
/// broke at (without `#`).
fn broken(ws: &Workspace) -> Result<Vec<(String, Broke)>, Fault> {
    let j = Journal::open(ws)?;
    Ok(j.runs_page(None, Some("error"), &[], usize::MAX)?
        .into_iter()
        .filter_map(|r| {
            let rt = &r.runtime;
            let step = rt.failed_step.as_deref()?.strip_prefix('#')?.to_string();
            Some((
                step,
                Broke {
                    when: r.ended.clone().unwrap_or_else(|| r.started.clone()),
                    workflow: r.workflow.clone(),
                    run: r.name.clone(),
                    id: r.id.clone(),
                    why: rt.why.clone(),
                },
            ))
        })
        .collect())
}

/// Each action runs broke at, the most recent first: at most `limit`.
pub fn breaks(ws: &Workspace, limit: usize) -> Result<Vec<Breaks>, Fault> {
    // newest first already: the first of each is its last
    let mut by: Vec<Breaks> = vec![];
    for (action, b) in broken(ws)? {
        match by.iter_mut().find(|x| x.action == action) {
            Some(x) => x.errors += 1,
            None => by.push(Breaks {
                action,
                errors: 1,
                last: b.when,
                why: b.why,
                run: b.run,
            }),
        }
    }
    by.truncate(limit);
    Ok(by)
}

/// The runs `action` (`#` or not) broke, newest first: at most `limit`.
/// An action that never broke one has none: not a mistake, as the action
/// may be gone while its failures stay.
pub fn broke(ws: &Workspace, action: &str, limit: usize) -> Result<Vec<Broke>, Fault> {
    let name = action.trim_start_matches('#');
    Ok(broken(ws)?
        .into_iter()
        .filter(|(a, _)| a == name)
        .map(|(_, b)| b)
        .take(limit)
        .collect())
}
