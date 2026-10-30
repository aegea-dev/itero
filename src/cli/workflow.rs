//! `itero workflow` (`itero wf`): the workflows and their runs.
use super::out::{ago, show, table, text};
use crate::engine::journal::{Journal, Recorded, Run, subject};
use crate::engine::{progress, steps};
use crate::workspace::Workspace;
use anyhow::{Result, bail};
use serde_json::{Value, json};

/// Each workflow: its file, what starts it, its runs and the last, and
/// whether a daemon takes its events.
pub fn list(ws: &Workspace, json: bool) -> Result<()> {
    let j = Journal::open(ws)?;
    let now = chrono::Utc::now();
    let mut flows = ws.flows()?;
    flows.sort_by_key(|(_, f)| f.meta.internal);
    let files = ws.flow_files()?;
    let mut out = vec![];
    for (name, f) in &flows {
        let on = &f.on;
        let mut starts: Vec<String> = on.events.iter().map(|e| format!("on {e}")).collect();
        starts.extend(on.schedules.iter().map(|s| format!("at {s}")));
        starts.extend(on.paths.iter().map(|p| format!("path {p}")));
        let mut notes = vec![];
        if f.meta.off {
            notes.push("off");
        } else if !crate::flow::active(&f.meta, now) {
            notes.push("not now (from/until)");
        }
        if f.meta.internal {
            notes.push("internal");
        }
        let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
        let last = runs.first().map(|r| {
            json!({
                "status": steps::status(&r.runtime),
                "at": r.ended.as_deref().unwrap_or(&r.started),
            })
        });
        out.push(json!({
            "name": name,
            "file": files.get(name).map(|f| ws.shown(f)),
            "starts": starts,
            "runs": runs.len(),
            "last": last,
            "notes": notes,
        }));
    }
    let v = json!({
        "workflows": out,
        "daemon": { "running": super::daemon_running(ws, &j)? },
        "waiting": j.waiting_messages()?,
    });
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
                    w["file"].as_str().unwrap_or("built in").to_string(),
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

/// Which runs `itero workflow runs` shows.
pub struct Filter {
    pub errors: bool,
    pub workflow: Option<String>,
    pub action: Option<String>,
    pub limit: Option<usize>,
    pub all: bool,
}

/// A run as a row: what `runs` shows, and why when it broke.
fn row(r: &Run) -> Value {
    let status = steps::status(&r.runtime);
    let mut v = json!({
        "id": r.id,
        "name": r.name,
        "workflow": r.workflow,
        "status": status,
        "started": r.started,
        "ended": r.ended,
    });
    if status == "error" {
        v["failed_step"] = json!(r.runtime.failed_step);
        v["why"] = json!(r.runtime.why);
    }
    v
}

/// The runs, newest first: all of them, or those `f` asks for.
pub fn runs(ws: &Workspace, f: &Filter, json: bool) -> Result<()> {
    let j = Journal::open(ws)?;
    if let Some(w) = &f.workflow
        && !ws.flows()?.iter().any(|(n, _)| n == w)
    {
        bail!("no workflow {w}: itero workflow list lists them");
    }
    let apart = if f.all || f.workflow.is_some() {
        vec![]
    } else {
        ws.internal()?
    };
    // an action's failures are runs that broke at it
    let errors = f.errors || f.action.is_some();
    let step = f
        .action
        .as_ref()
        .map(|a| format!("#{}", a.trim_start_matches('#')));
    let found = j.runs_page(
        f.workflow.as_deref(),
        errors.then_some("error"),
        &apart,
        if step.is_some() {
            usize::MAX
        } else {
            f.limit.unwrap_or(usize::MAX)
        },
    )?;
    // a running run shows what its newest running step said last
    let rows: Vec<Value> = found
        .iter()
        .map(|r| {
            let mut v = row(r);
            if v["status"] == "running"
                && let Some(p) = progress::of_run(&ws.root, &r.id).into_iter().next()
            {
                v["progress"] = p;
            }
            v
        })
        .filter(|r| step.as_ref().is_none_or(|s| r["failed_step"] == s.as_str()))
        .take(f.limit.unwrap_or(usize::MAX))
        .collect();
    show(json, &Value::Array(rows), |v| {
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

/// One run, by id or name: how it started, each step and how it went, the
/// events it took, and its state.
pub fn run(ws: &Workspace, run: &str, json: bool) -> Result<()> {
    let j = Journal::open(ws)?;
    let found = j.run_by(run)?;
    let Some(r) = found.first() else {
        bail!("no run {run}: itero workflow runs lists them");
    };
    if found.len() > 1 && !json {
        println!(
            "{} runs are named {run}: this is the newest; give an id for another",
            found.len()
        );
    }
    let mut v = story(&j, r)?;
    // the steps running now, and what each said last
    if v["status"] == "running" {
        for p in progress::of_run(&ws.root, &r.id) {
            v["steps"].as_array_mut().expect("a list").push(json!({
                "what": "running", "step": p["step"], "data": p["said"], "time": p["at"],
            }));
        }
    }
    show(json, &v, |v| {
        println!(
            "{}  {}  {}  {}",
            text(&v["name"]),
            text(&v["id"]),
            text(&v["workflow"]),
            text(&v["status"])
        );
        for s in v["steps"].as_array().into_iter().flatten() {
            let line = match s["what"].as_str().unwrap_or("") {
                "started" => format!("started  {}", s["data"]),
                "timed out" => "waited too long".to_string(),
                "stopped" => format!("{}  stopped", text(&s["step"])),
                "event" => format!("event {}  {}", text(&s["type"]), s["data"]),
                "running" => format!("{}  running  {}", text(&s["step"]), text(&s["data"])),
                what => format!("{}  {what}  {}", text(&s["step"]), s["data"]),
            };
            println!("  {}  {line}", text(&s["time"]));
        }
        if let Some(ended) = v["ended"].as_str() {
            println!("  {ended}  ended {}", text(&v["status"]));
        }
        println!("  state {}", v["state"]);
    })
}

/// A run's story, as one value.
fn story(j: &Journal, r: &Run) -> Result<Value> {
    // each result's cause is its job, which names the action
    let mine: Vec<_> = j
        .events(Some(&subject("worker")))?
        .into_iter()
        .filter(|e| e.event.data["run"] == r.id.as_str())
        .collect();
    let action =
        |e: &Recorded| format!("#{}", e.event.data["job"]["action"].as_str().unwrap_or("?"));
    let jobs: std::collections::BTreeMap<String, String> = mine
        .iter()
        .map(|e| (e.event.id.clone(), action(e)))
        .collect();
    let mut steps_: Vec<Value> = vec![];
    for e in j.events(Some(&subject(&r.id)))? {
        let d = &e.event.data;
        let step = || {
            let name = e
                .event
                .cause
                .as_ref()
                .and_then(|c| jobs.get(c))
                .map_or("a step", String::as_str);
            let undo = steps::Phase::Undo.index() as u64;
            match d["path"][0].as_u64() {
                Some(p) if p == undo => format!("{name} (undo)"),
                _ => name.to_string(),
            }
        };
        let s = match e.event.kind.as_str() {
            "run.started" => json!({ "what": "started", "data": d }),
            "action.finished" => json!({ "what": "done", "step": step(), "data": d["output"] }),
            "action.waiting" => {
                json!({ "what": "waiting", "step": step(), "data": d["output"] })
            }
            "action.failed" => json!({ "what": "error", "step": step(), "data": d["error"] }),
            // a step whose group ended without it, as the workflow names it
            "step.stopped" => json!({ "what": "stopped", "step": d["step"] }),
            "timer.fired" => json!({ "what": "timed out" }),
            // an event is copied to every waiting run: show those it took
            "message" if r.runtime.taken.iter().any(|t| d["id"] == t.as_str()) => {
                json!({ "what": "event", "type": d["type"], "data": d["data"] })
            }
            _ => continue,
        };
        let mut s = s;
        s["time"] = json!(e.event.time);
        steps_.push(s);
    }
    // in the journal's order when two have the same time
    steps_.sort_by(|a, b| a["time"].as_str().cmp(&b["time"].as_str()));
    Ok(json!({
        "name": r.name,
        "id": r.id,
        "workflow": r.workflow,
        "status": steps::status(&r.runtime),
        "started": r.started,
        "ended": r.ended,
        "steps": steps_,
        "state": Value::Object(r.runtime.state.clone()),
    }))
}
