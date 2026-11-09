//! `itero ws wf`: the workflows and their runs.
use super::out::{ago, show, table, text};
use crate::answer::{Client, runs};
use crate::workspace::Workspace;
use anyhow::Result;
use serde_json::Value;

/// This machine's workspace `ws`, as the commands ask it.
fn client(ws: &Workspace) -> Client {
    Client::Local(ws.clone())
}

/// Each workflow: its file, what starts it, its runs and the last, and
/// whether a daemon takes its events.
pub fn list(ws: &Workspace, json: bool) -> Result<()> {
    let now = chrono::Utc::now();
    let v = serde_json::to_value(client(ws).workflows()?)?;
    show(json, &v, |v| {
        let rows: Vec<Vec<String>> = v["workflows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|w| {
                let starts: Vec<String> = w["starts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text)
                    .collect();
                let last = match &w["last"] {
                    Value::Null => "last: none".to_string(),
                    l => format!("last: {} {}", text(&l["status"]), ago(&text(&l["at"]), now)),
                };
                let notes: Vec<String> = w["notes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text)
                    .collect();
                vec![
                    text(&w["name"]),
                    w["path"].as_str().unwrap_or("built in").to_string(),
                    if starts.is_empty() {
                        "a step only".into()
                    } else {
                        starts.join(", ")
                    },
                    format!("runs {}", w["runs"]),
                    last,
                    notes.join(", "),
                ]
            })
            .collect();
        table(&rows);
        let waiting = match v["waiting"].as_u64().unwrap_or(0) {
            0 => "no event waiting".to_string(),
            1 => "1 event waiting".to_string(),
            n => format!("{n} events waiting"),
        };
        let daemon = if v["daemon"]["running"] == true {
            "running"
        } else {
            "not running"
        };
        println!("daemon: {daemon}, {waiting}");
    })
}

/// Which runs `itero ws wf runs` shows.
pub struct Filter {
    pub errors: bool,
    pub workflow: Option<String>,
    pub action: Option<String>,
    pub limit: Option<usize>,
    pub all: bool,
}

/// The runs, newest first: all of them, or those `f` asks for.
pub fn runs(ws: &Workspace, f: &Filter, json: bool) -> Result<()> {
    let q = runs::Query {
        workflow: f.workflow.clone(),
        status: f.errors.then(|| "error".to_string()),
        action: f.action.clone(),
        limit: f.limit,
        all: f.all,
    };
    let rows = client(ws).runs(&q)?;
    show(json, &serde_json::to_value(rows)?, |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let mut cells = vec![
                    text(&r["id"]),
                    text(&r["name"]),
                    text(&r["workflow"]),
                    text(&r["status"]),
                    text(&r["started"]),
                    r["ended"].as_str().unwrap_or("-").to_string(),
                ];
                if let Some(name) = r["from"].as_str() {
                    cells.push(format!("from {name}"));
                }
                if r["status"] == "error" {
                    cells.push(format!("{}: {}", text(&r["failed_step"]), text(&r["why"])));
                }
                if r["progress"].is_object() {
                    let p = &r["progress"];
                    cells.push(format!("{}: {}", text(&p["step"]), text(&p["said"])));
                }
                cells
            })
            .collect();
        table(&rows);
    })
}

/// Asks a run to stop: the daemon running the workspace stops it now, or
/// the next to start does.
pub fn stop(ws: &Workspace, run: &str, json: bool) -> Result<()> {
    let stopped = client(ws).stop(run)?;
    show(json, &serde_json::to_value(&stopped)?, |_| {
        println!("stopping {}", stopped.stopping);
        if !stopped.daemon {
            eprintln!("no daemon is running: the next to start stops it");
        }
    })
}

/// One run, by id or name: how it started, each step and how it went, the
/// events it took, and its state.
pub fn run(ws: &Workspace, run: &str, json: bool) -> Result<()> {
    let story = client(ws).run(run)?;
    if let Some(n) = story.named_alike
        && !json
    {
        println!("{n} runs are named {run}: this is the newest; give an id for another");
    }
    let from = story.run.from.clone();
    let v = serde_json::to_value(&story)?;
    show(json, &v, |v| {
        println!(
            "{}  {}  {}  {}{}",
            text(&v["name"]),
            text(&v["id"]),
            text(&v["workflow"]),
            text(&v["status"]),
            from.as_ref()
                .map_or(String::new(), |n| format!("  from {n}"))
        );
        for s in v["steps"].as_array().into_iter().flatten() {
            let line = match s["what"].as_str().unwrap_or("") {
                "started" => format!("started  {}", s["data"]),
                "timed out" => "waited too long".to_string(),
                "stopped by command" => "stopped by command".to_string(),
                "stopped" => format!("{}  stopped", text(&s["step"])),
                "event" => format!("event {}  {}", text(&s["type"]), s["data"]),
                "running" => format!("{}  running  {}", text(&s["step"]), text(&s["data"])),
                what => format!("{}  {what}  {}", text(&s["step"]), s["data"]),
            };
            // a workflow used as a step: its run
            let line = match s["run"].as_str() {
                Some(run) => format!("{line}  run {run}"),
                None => line,
            };
            println!("  {}  {line}", text(&s["time"]));
        }
        if let Some(ended) = v["ended"].as_str() {
            println!("  {ended}  ended {}", text(&v["status"]));
        }
        println!("  state {}", v["state"]);
    })
}
