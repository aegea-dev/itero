//! `itero ws ac` and `itero ws cn`: what there is, their repeat and
//! args, and which actions break.
use super::client;
use super::out::{ago, show, table, text};
use crate::workspace::Workspace;
use anyhow::Result;
use serde_json::Value;

/// Each action: its name and its files (one per version); the built-in
/// ones too.
pub fn actions(ws: &Workspace, json: bool) -> Result<()> {
    let out = serde_json::to_value(client(ws).actions()?)?;
    show(json, &out, |v| {
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
    let out = serde_json::to_value(client(ws).connectors()?)?;
    show(json, &out, |v| {
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
    let now = chrono::Utc::now();
    if let Some(name) = action {
        let mine = serde_json::to_value(client(ws).failures_of(name, limit)?)?;
        let name = name.trim_start_matches('#');
        return show(json, &mine, |v| {
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
    let out = serde_json::to_value(client(ws).errors_by_action(limit)?)?;
    show(json, &out, |v| {
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
