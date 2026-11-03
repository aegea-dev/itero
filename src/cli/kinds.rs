//! `itero ws ac` and `itero ws cn`: what there is, their repeat and
//! args, and which actions break.
use super::out::{ago, show, table, text};
use crate::engine::journal::Journal;
use crate::engine::worker::BUILT_IN;
use crate::workspace::Workspace;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Each action: its name and its files (one per version); the built-in
/// ones too.
pub fn actions(ws: &Workspace, json: bool) -> Result<()> {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (file, path) in ws.action_files()? {
        let name = file.split('@').next().unwrap_or(&file).to_string();
        by_name.entry(name).or_default().push(ws.shown(&path));
    }
    let mut out: Vec<Value> = by_name
        .into_iter()
        .map(|(name, files)| json!({ "name": name, "files": files, "built_in": false }))
        .collect();
    for b in BUILT_IN {
        if !out.iter().any(|a| a["name"] == b) {
            out.push(json!({ "name": b, "files": [], "built_in": true }));
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
                ]
            })
            .collect();
        table(&rows);
    })
}

/// Each connector, once for each set of args the workflows give it: the
/// type of the events it sends, its file, and those args.
pub fn connectors(ws: &Workspace, json: bool) -> Result<()> {
    let out: Vec<Value> = crate::connectors::copies(ws)?
        .into_iter()
        .map(|c| {
            let mut v = json!({ "type": c.listen.kind, "file": ws.shown(&c.file) });
            if !c.listen.args.is_empty() {
                v["args"] = Value::Object(c.listen.args.clone());
            }
            v
        })
        .collect();
    show(json, &Value::Array(out), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let args = match &c["args"] {
                    Value::Object(a) => crate::flow::Listen {
                        kind: String::new(),
                        args: a.clone(),
                    }
                    .key(),
                    _ => String::new(),
                };
                vec![text(&c["type"]), text(&c["file"]), args]
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
            let rt = &r.runtime;
            let step = rt.failed_step.as_deref()?.strip_prefix('#')?.to_string();
            let at = r.ended.clone().unwrap_or(r.started.clone());
            Some((
                step,
                json!({
                    "when": at,
                    "workflow": r.workflow,
                    "run": r.name,
                    "id": r.id,
                    "why": rt.why,
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
