//! `itero action` and `itero connector`: what there is, their args, and
//! which actions break.
use super::out::{ago, given, plain_name, show, table, text};
use crate::engine::journal::Journal;
use crate::engine::worker::BUILT_IN;
use crate::workspace::Workspace;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Each action: its name, its files (one per version), and whether it has
/// args; the built-in ones too.
pub fn actions(ws: &Workspace, json: bool) -> Result<()> {
    let entries = ws.entries("action")?;
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, path) in ws.action_files()? {
        let name = file.split('@').next().unwrap_or(&file).to_string();
        by_name.entry(name).or_default().push(ws.shown(&path));
    }
    let mut out: Vec<Value> = by_name
        .into_iter()
        .map(|(name, files)| {
            let args = entries.get(&name).is_some_and(|e| e.get("args").is_some());
            json!({ "name": name, "files": files, "built_in": false, "args": args })
        })
        .collect();
    for b in BUILT_IN {
        if !out.iter().any(|a| a["name"] == b) {
            out.push(json!({ "name": b, "files": [], "built_in": true, "args": false }));
        }
    }
    show(json, &Value::Array(out), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let files: Vec<String> = a["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text)
                    .collect();
                vec![
                    format!("#{}", text(&a["name"])),
                    if a["built_in"] == true {
                        "built in".into()
                    } else {
                        files.join(", ")
                    },
                    if a["args"] == true { "args" } else { "" }.into(),
                ]
            })
            .collect();
        table(&rows);
    })
}

/// Each connector: the type of the events it sends, its file, and whether
/// it has args.
pub fn connectors(ws: &Workspace, json: bool) -> Result<()> {
    let entries = ws.entries("connector")?;
    let out: Vec<Value> = ws
        .connector_files()?
        .into_iter()
        .map(|(kind, f)| {
            let args = entries.get(&kind).is_some_and(|e| e.get("args").is_some());
            json!({ "type": kind, "file": ws.shown(&f), "args": args })
        })
        .collect();
    show(json, &Value::Array(out), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                vec![
                    text(&c["type"]),
                    text(&c["file"]),
                    if c["args"] == true { "args" } else { "" }.into(),
                ]
            })
            .collect();
        table(&rows);
    })
}

/// Which actions break: without a name, each action runs failed at, how
/// often, when and why last; with one, its failures, newest first. From
/// the runs that ended in error: the step they failed at, and why.
pub fn errors(ws: &Workspace, action: Option<&str>, limit: usize, json: bool) -> Result<()> {
    let j = Journal::open(ws)?;
    let now = chrono::Utc::now();
    let failed: Vec<(String, Value)> = j
        .runs_page(None, Some("error"), &[], usize::MAX)?
        .into_iter()
        .filter_map(|r| {
            let st = &r.runtime.state;
            let step = st
                .get("failed_step")?
                .as_str()?
                .strip_prefix('#')?
                .to_string();
            let at = r.ended.clone().unwrap_or(r.started.clone());
            Some((
                step,
                json!({
                    "when": at,
                    "workflow": r.workflow,
                    "run": r.name,
                    "id": r.id,
                    "why": st.get("why").cloned().unwrap_or(Value::Null),
                }),
            ))
        })
        .collect();
    if let Some(name) = action {
        let name = name.trim_start_matches('#');
        let mine: Vec<Value> = failed
            .into_iter()
            .filter(|(a, _)| a == name)
            .map(|(_, f)| f)
            .take(limit)
            .collect();
        return show(json, &Value::Array(mine), |v| {
            let rows: Vec<Vec<String>> = v
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| {
                    vec![
                        ago(&text(&f["when"]), now),
                        text(&f["workflow"]),
                        text(&f["run"]),
                        text(&f["why"]),
                    ]
                })
                .collect();
            if rows.is_empty() {
                println!("#{name}: no run ended in error at it");
            }
            table(&rows);
        });
    }
    // newest first already: the first of each is its last
    let mut by: Vec<(String, usize, Value)> = vec![];
    for (a, f) in failed {
        match by.iter_mut().find(|(n, _, _)| *n == a) {
            Some((_, count, _)) => *count += 1,
            None => by.push((a, 1, f)),
        }
    }
    let out: Vec<Value> = by
        .into_iter()
        .take(limit)
        .map(|(a, count, last)| {
            json!({ "action": a, "errors": count, "last": last["when"], "why": last["why"], "run": last["run"] })
        })
        .collect();
    show(json, &Value::Array(out), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                vec![
                    format!("#{}", text(&e["action"])),
                    format!("{} errors", e["errors"]),
                    format!("last {}", ago(&text(&e["last"]), now)),
                    text(&e["why"]),
                ]
            })
            .collect();
        if rows.is_empty() {
            println!("no action has broken a run");
        }
        table(&rows);
    })
}

/// Whose args these are.
#[derive(Clone, Copy)]
pub enum Kind {
    Action,
    Connector,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Action => "action",
            Kind::Connector => "connector",
        }
    }

    /// Whether the workspace has one of this name.
    fn has(self, ws: &Workspace, name: &str) -> Result<bool> {
        Ok(match self {
            Kind::Action => ws.action_file(name, None).is_ok() || BUILT_IN.contains(&name),
            Kind::Connector => ws.connector_files()?.iter().any(|(n, _)| n == name),
        })
    }

    fn known(self, ws: &Workspace, name: &str) -> Result<()> {
        let k = self.key();
        if !plain_name(name) {
            bail!("{name} is not an {k}'s name");
        }
        if !self.has(ws, name)? {
            bail!("no {k} {name}: itero {k} list lists them");
        }
        Ok(())
    }
}

/// An action's or a connector's args, as `itero.config.json` has them.
pub fn args_get(ws: &Workspace, kind: Kind, name: &str, json: bool) -> Result<()> {
    entry_get(ws, kind, name, "args", json)
}

/// One key of an action's or a connector's entry (`args`, `repeat`), as
/// `itero.config.json` has it.
pub fn entry_get(ws: &Workspace, kind: Kind, name: &str, key: &str, json: bool) -> Result<()> {
    let name = name.trim_start_matches('#');
    kind.known(ws, name)?;
    let v = ws
        .entries(kind.key())?
        .get(name)
        .and_then(|e| e.get(key))
        .cloned()
        .unwrap_or(Value::Null);
    show(json, &v, |a| match a {
        Value::Null => eprintln!("{name} has no {key} in {}", crate::workspace::MANIFEST),
        a => println!("{}", serde_json::to_string_pretty(a).unwrap_or_default()),
    })
}

/// Sets an action's repeat: a list of one or more objects, its step run
/// once per item.
pub fn repeat_set(
    ws: &Workspace,
    name: &str,
    data: Option<String>,
    file: Option<std::path::PathBuf>,
) -> Result<()> {
    let name = name.trim_start_matches('#');
    Kind::Action.known(ws, name)?;
    let wrong = || {
        anyhow::anyhow!(
            "repeat is a list of one or more objects, as JSON: one run of the step for each"
        )
    };
    let v: Value = serde_json::from_str(&given(data, file)?).map_err(|_| wrong())?;
    match &v {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {}
        _ => return Err(wrong()),
    }
    let f = ws.write_entry(Kind::Action.key(), name, "repeat", v)?;
    println!("{}", f.display());
    Ok(())
}

/// Sets an action's args (an object, or a list of objects read in order,
/// as a step's) or a connector's (an object: a connector runs once).
pub fn args_set(
    ws: &Workspace,
    kind: Kind,
    name: &str,
    data: Option<String>,
    file: Option<std::path::PathBuf>,
) -> Result<()> {
    let name = name.trim_start_matches('#');
    kind.known(ws, name)?;
    let v: Value = serde_json::from_str(&given(data, file)?).map_err(|_| match kind {
        Kind::Action => anyhow::anyhow!("args are an object, or a list of objects, as JSON"),
        Kind::Connector => anyhow::anyhow!("a connector's args are an object, as JSON"),
    })?;
    match (kind, &v) {
        (_, Value::Object(_)) => {}
        (Kind::Action, Value::Array(items)) if items.iter().all(Value::is_object) => {
            crate::flow::read_args(&v).map_err(|why| anyhow::anyhow!("{why}"))?;
        }
        (Kind::Action, _) => bail!("args are an object, or a list of objects, as JSON"),
        (Kind::Connector, _) => {
            bail!("a connector's args are an object, as JSON: a connector runs once")
        }
    }
    let f = ws.write_entry(kind.key(), name, "args", v)?;
    println!("{}", f.display());
    Ok(())
}
