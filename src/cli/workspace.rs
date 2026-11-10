//! `itero workspace`: the machine's list of workspaces, which one
//! `itero daemon --all` runs.
use super::out::{ago, show, table, text};
use crate::engine::journal::Journal;
use crate::machine::{Entry, List, list_path};
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// The workspace a `ws` command is about: the one named on the machine's
/// list, else the folder's.
pub fn chosen(name: Option<&str>) -> Result<Workspace> {
    let Some(name) = name else {
        return crate::workspace::find(&std::env::current_dir()?);
    };
    Ok(crate::engine::machine::open(&listed(name)?)?.0)
}

/// Where a workspace command goes: this machine's workspace (`--ws` on
/// its list, else this folder's), or, with a context, its server's
/// workspace (`--ws`, else the context's; with neither, which one is asked,
/// never guessed).
pub fn target(
    ws: Option<&str>,
    context: Option<crate::context::Context>,
) -> Result<crate::answer::Client> {
    use crate::answer::{Client, remote};
    let Some(c) = context else {
        return Ok(Client::Local(Box::new(chosen(ws)?)));
    };
    let ws = match ws.map(String::from).or(c.ws.clone()) {
        Some(w) => w,
        None => {
            let listed: Vec<Value> =
                remote::server(&c.name, &c.url, c.token.as_deref(), &["workspaces"])?;
            let names: Vec<&str> = listed.iter().filter_map(|w| w["name"].as_str()).collect();
            bail!(
                "which workspace on {}: --ws one of {} (or itero context add {} {} --ws NAME)",
                c.name,
                names.join(", "),
                c.name,
                c.url
            );
        }
    };
    Ok(Client::Remote(remote::Remote {
        name: c.name,
        url: c.url,
        token: c.token,
        ws,
    }))
}

/// The entry on the machine's list with this name.
pub fn listed(name: &str) -> Result<Entry> {
    let list = List::read(&list_path()?)?;
    match list.by_name(name) {
        Some(e) => Ok(e.clone()),
        None => bail!("no workspace {name} on this machine's list: itero ws list lists them"),
    }
}

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
    if keep_env_out_of_git(&ws.root)? {
        println!(".env, where its secrets and values may be kept, is in .gitignore now");
    }
    if daemon_states().is_some() {
        // its state, once the daemon has read the list (in about 2 s)
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let said = loop {
            match machine_says(&ws.root) {
                Some(m) if m.state.is_some() || std::time::Instant::now() >= end => break Some(m),
                None => break None,
                Some(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        };
        match said {
            Some(m) if m.state.is_none() => {
                println!(
                    "the machine's daemon has not taken it up yet: itero ws list says when it has"
                )
            }
            Some(m) => {
                if m.runs() {
                    println!("the machine's daemon runs it");
                }
                for l in m.lines("it") {
                    println!("{l}");
                }
            }
            None => {
                println!("the machine's daemon no longer answers: itero service status says why")
            }
        }
    } else {
        println!(
            "no daemon runs the list now: itero service install starts one at login (or itero daemon --all, in a terminal)"
        );
    }
    Ok(())
}

/// Adds `.env` to the workspace's `.gitignore` (made when there is none),
/// unless a line there names it already: whether it added it.
fn keep_env_out_of_git(root: &std::path::Path) -> Result<bool> {
    let f = root.join(".gitignore");
    let text = match std::fs::read_to_string(&f) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", f.display())),
    };
    if text.lines().any(|l| matches!(l.trim(), ".env" | "/.env")) {
        return Ok(false);
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&f, format!("{text}{sep}.env\n"))
        .with_context(|| format!("cannot write {}", f.display()))?;
    Ok(true)
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
    match crate::engine::machine::open(e) {
        Err(_) => (json!("missing"), Value::Null),
        Ok((_, j)) => {
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
/// What the machine's daemon, when one answers, says of a workspace: its
/// name and state there and why (none when it is not on its list), and
/// which itero the daemon runs.
pub(crate) struct MachineSays {
    pub name: Option<String>,
    pub state: Option<(String, Option<String>)>,
    pub itero: Option<String>,
}

/// What the machine's daemon says of the workspace at `root`; `None` when
/// no machine's daemon answers.
pub(crate) fn machine_says(root: &std::path::Path) -> Option<MachineSays> {
    let states = daemon_states()?;
    let entry = states
        .iter()
        .find(|w| w["folder"].as_str().map(std::path::Path::new) == Some(root));
    let itero = crate::engine::http::machine_port()
        .and_then(super::daemon_health)
        .and_then(|h| h["daemon"]["itero"].as_str().map(str::to_string));
    Some(MachineSays {
        name: entry.and_then(|w| w["name"].as_str().map(str::to_string)),
        state: entry.map(|w| {
            (
                w["state"].as_str().unwrap_or("unknown").to_string(),
                w["why"].as_str().map(str::to_string),
            )
        }),
        itero,
    })
}

impl MachineSays {
    /// Whether it runs the workspace.
    pub fn runs(&self) -> bool {
        self.state.as_ref().is_some_and(|(s, _)| s == "running")
    }

    /// What to tell, `what` naming the workspace ("pala-su", or "it"): why
    /// it does not run it, and, when it runs another itero, how to put this
    /// one in place. Nothing when it runs it with this itero.
    pub fn lines(&self, what: &str) -> Vec<String> {
        let mut out = vec![];
        match &self.state {
            Some((s, _)) if s == "running" => {}
            Some((s, why)) => out.push(format!(
                "the machine's daemon does not run {what}: {}",
                why.as_deref().unwrap_or(s)
            )),
            None => out.push(
                "the machine's daemon does not run this workspace: itero ws add puts it on its list"
                    .into(),
            ),
        }
        let this = env!("CARGO_PKG_VERSION");
        if let Some(v) = &self.itero
            && v != this
        {
            out.push(format!(
                "the machine's daemon runs itero {v}, this is {this}: itero service restart puts this one in place"
            ));
        }
        out
    }
}

pub(crate) fn daemon_states() -> Option<Vec<Value>> {
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
