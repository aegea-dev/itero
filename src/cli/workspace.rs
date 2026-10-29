//! `itero workspace`: the machine's list of workspaces, which one
//! `itero daemon --all` runs.
use super::out::{ago, show, table, text};
use crate::engine::journal::Journal;
use crate::machine::{Entry, List, list_path};
use crate::workspace::Workspace;
use anyhow::{Result, bail};
use serde_json::{Value, json};

/// `itero workspace add`: this workspace on the list, under its journal's
/// name; a name another listed workspace has is replaced in this one's
/// journal. A folder already listed by its journal id (it moved) is
/// updated.
pub fn add(ws: &Workspace) -> Result<()> {
    let path = list_path()?;
    let mut list = List::read(&path)?;
    let j = Journal::open(ws)?;
    let me = j.workspace()?;
    let mut name = me.name.clone();
    while list.by_name(&name).is_some_and(|e| e.id != me.id) {
        let new = crate::names::random();
        println!("the name {name} is taken on this machine: this workspace is now {new}");
        j.rename_workspace(&new)?;
        name = new;
    }
    list.upsert(Entry {
        id: me.id,
        name: name.clone(),
        folder: ws.root.clone(),
    });
    list.write(&path)?;
    println!("added {name} ({})", ws.root.display());
    if daemon_states().is_some() {
        println!("the machine's daemon starts it now");
    } else {
        println!(
            "no daemon runs the list now: itero service install starts one at login (or itero daemon --all, in a terminal)"
        );
    }
    Ok(())
}

/// `itero workspace remove [name]`: off the list; the folder and its
/// journal stay.
pub fn remove(name: Option<&str>) -> Result<()> {
    let path = list_path()?;
    let mut list = List::read(&path)?;
    let entry = match name {
        Some(n) => list.by_name(n).cloned(),
        None => {
            let ws = crate::workspace::find(&std::env::current_dir()?)?;
            let id = Journal::open(&ws)?.workspace()?.id;
            list.by_id(&id).cloned()
        }
    };
    let Some(e) = entry else {
        bail!(
            "{} is not on this machine's list: itero workspace list shows it",
            name.unwrap_or("this workspace")
        );
    };
    list.remove_id(&e.id);
    list.write(&path)?;
    println!(
        "removed {} ({}); its folder stays",
        e.name,
        e.folder.display()
    );
    Ok(())
}

/// A listed workspace as it is now: `missing` when its folder is gone or
/// holds another workspace, else `stopped` (no daemon runs it).
fn state(e: &Entry) -> (Value, Value) {
    let found = crate::workspace::find(&e.folder)
        .ok()
        .filter(|ws| ws.root == e.folder && ws.journal().is_file())
        .and_then(|ws| Journal::open(&ws).ok())
        .filter(|j| j.workspace().is_ok_and(|me| me.id == e.id));
    match found {
        None => (json!("missing"), Value::Null),
        Some(j) => {
            let last = j
                .runs_page(None, None, &[], 1)
                .ok()
                .and_then(|r| r.into_iter().next())
                .map(|r| json!(r.started));
            (json!("stopped"), last.unwrap_or(Value::Null))
        }
    }
}

/// The machine daemon's own view of its workspaces, when one answers.
fn daemon_states() -> Option<Vec<Value>> {
    let port = crate::engine::http::machine_port()?;
    crate::net::client(std::time::Duration::from_secs(2))
        .ok()?
        .get(format!("http://127.0.0.1:{port}/workspaces"))
        .send()
        .ok()?
        .json()
        .ok()
}

/// `itero workspace list`: each listed workspace's name, folder, state
/// (the daemon's, when one runs) and last run.
pub fn list(json_out: bool) -> Result<()> {
    let list = List::read(&list_path()?)?;
    let running = daemon_states().unwrap_or_default();
    let rows: Vec<Value> = list
        .workspaces
        .iter()
        .map(|e| {
            let (mut state, last) = state(e);
            let mut why = Value::Null;
            if let Some(d) = running.iter().find(|d| d["name"] == e.name.as_str()) {
                state = d["state"].clone();
                why = d["why"].clone();
            }
            json!({ "name": e.name, "folder": e.folder.display().to_string(),
                    "state": state, "why": why, "last_run": last })
        })
        .collect();
    let v = Value::Array(rows);
    show(json_out, &v, |v| {
        let now = chrono::Utc::now();
        let mut t = vec![vec![
            "NAME".into(),
            "FOLDER".into(),
            "STATE".into(),
            "LAST RUN".into(),
        ]];
        for r in v.as_array().into_iter().flatten() {
            let last = r["last_run"]
                .as_str()
                .map(|s| ago(s, now))
                .unwrap_or_default();
            t.push(vec![
                text(&r["name"]),
                text(&r["folder"]),
                text(&r["state"]),
                last,
            ]);
        }
        if t.len() == 1 {
            println!("no workspaces on this machine's list: itero workspace add, in one");
        } else {
            table(&t);
        }
    })
}
