//! The machine's list of workspaces as one answer: `itero ws list` and
//! the machine's `GET /workspaces`.
use super::Fault;
use crate::machine::{List, list_path};
use serde::{Deserialize, Serialize};

/// A workspace on the machine's list, as it is now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub name: String,
    pub folder: String,
    /// What the machine's daemon says of it (`running`, `stopped`,
    /// `failed`, `missing`); `pending` while a running daemon has not read
    /// it yet; `stopped` when no daemon runs the list.
    pub state: String,
    pub why: Option<String>,
    /// When its newest run started.
    pub last_run: Option<String>,
}

/// What the machine's daemon says of a workspace it has.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub name: String,
    pub state: String,
    pub why: Option<String>,
}

/// The state of a listed workspace a running daemon has not read yet.
pub const PENDING: &str = "pending";

/// Each workspace on the machine's list: `missing` when its folder is
/// gone or holds another workspace; else what `seen`, the machine's
/// daemon (the CLI asks it, the route is it), says, or `pending` when it
/// has not read it yet; with no daemon (`None`), `stopped`.
pub fn workspaces(seen: Option<&[Seen]>) -> Result<Vec<Listed>, Fault> {
    let list = List::read(&list_path()?)?;
    Ok(list
        .workspaces
        .iter()
        .map(|e| {
            let (mut state, mut why, last_run) = match crate::engine::machine::open(e) {
                Err(_) => ("missing".to_string(), None, None),
                Ok((_, j)) => {
                    let last = j
                        .runs_page(None, None, &[], 1)
                        .ok()
                        .and_then(|r| r.into_iter().next())
                        .map(|r| r.started);
                    ("stopped".to_string(), None, last)
                }
            };
            match seen.map(|all| all.iter().find(|s| s.name == e.name)) {
                Some(Some(s)) => {
                    state = s.state.clone();
                    why = s.why.clone();
                }
                Some(None) if state != "missing" => {
                    state = PENDING.into();
                    why =
                        Some("the machine's daemon takes it up when it next reads the list".into());
                }
                _ => {}
            }
            Listed {
                name: e.name.clone(),
                folder: e.folder.display().to_string(),
                state,
                why,
                last_run,
            }
        })
        .collect())
}
