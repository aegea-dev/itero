//! Runs: the list (`itero ws wf runs`, `GET /runs`), one run's story
//! (`itero ws wf runs <run>`, `GET /runs/{id}`) and stopping one
//! (`itero ws wf stop`, `POST /runs/{id}/stop`).
use super::Fault;
use crate::engine::journal::{Journal, Recorded, Run, subject};
use crate::engine::{progress, steps};
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The statuses a run can have.
const STATUSES: [&str; 6] = ["running", "waiting", "done", "failed", "error", "stopped"];

/// Which runs: all of them unless a field narrows them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub workflow: Option<String>,
    /// One of `STATUSES`.
    pub status: Option<String>,
    /// The runs that broke at this action (`#` or not): status `error`.
    pub action: Option<String>,
    /// At most this many; all when none.
    pub limit: Option<usize>,
    /// The internal workflows' runs too.
    #[serde(default)]
    pub all: bool,
}

/// A run as a list shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub workflow: String,
    pub status: String,
    pub started: String,
    pub ended: Option<String>,
    /// The run whose step started it: a workflow used as a step.
    pub parent: Option<String>,
    /// That run's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// A run that broke: at which step, and why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// A running run: what its newest running step said last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Value>,
}

fn row(j: &Journal, ws: &Workspace, r: &Run) -> Result<Row, Fault> {
    let status = steps::status(&r.runtime).to_string();
    let broke = status == "error";
    let from = match &r.parent {
        Some(p) => Some(j.run(p)?.map_or_else(|| p.clone(), |p| p.name)),
        None => None,
    };
    let progress = match status.as_str() {
        "running" => progress::of_run(&ws.root, &r.id).into_iter().next(),
        _ => None,
    };
    Ok(Row {
        id: r.id.clone(),
        name: r.name.clone(),
        workflow: r.workflow.clone(),
        started: r.started.clone(),
        ended: r.ended.clone(),
        parent: r.parent.clone(),
        from,
        failed_step: r.runtime.failed_step.clone().filter(|_| broke),
        why: r.runtime.why.clone().filter(|_| broke),
        progress,
        status,
    })
}

/// The runs, newest first: those `q` asks for.
pub fn list(ws: &Workspace, q: &Query) -> Result<Vec<Row>, Fault> {
    let mut problems = vec![];
    if let Some(s) = &q.status
        && !STATUSES.contains(&s.as_str())
    {
        problems.push(format!("status is one of {}, not {s}", STATUSES.join(", ")));
    }
    if q.action.is_some() && q.status.as_deref().is_some_and(|s| s != "error") {
        problems.push("action is for the runs that broke: status error, or none".into());
    }
    if !problems.is_empty() {
        return Err(Fault::Bad(problems.join("; ")));
    }
    if let Some(w) = &q.workflow
        && !ws.flows()?.iter().any(|(n, _)| n == w)
    {
        return Err(Fault::Missing(format!(
            "no workflow {w}: itero ws wf list lists them"
        )));
    }
    let j = Journal::open(ws)?;
    // a workflow asked for by name is shown, internal or not
    let apart = if q.all || q.workflow.is_some() {
        vec![]
    } else {
        ws.internal()?
    };
    let step = q
        .action
        .as_ref()
        .map(|a| format!("#{}", a.trim_start_matches('#')));
    let status = match &step {
        Some(_) => Some("error"),
        None => q.status.as_deref(),
    };
    let limit = q.limit.unwrap_or(usize::MAX);
    let found = match status {
        // under way: few, and their status is not a column
        Some(st @ ("running" | "waiting")) => j
            .active_runs()?
            .into_iter()
            .rev()
            .filter(|r| steps::status(&r.runtime) == st)
            .filter(|r| q.workflow.as_ref().is_none_or(|w| *w == r.workflow))
            .filter(|r| !apart.contains(&r.workflow))
            .take(limit)
            .collect(),
        // an action's failures: every broken run, then those at its step
        ended => j.runs_page(
            q.workflow.as_deref(),
            ended,
            &apart,
            if step.is_some() { usize::MAX } else { limit },
        )?,
    };
    let mut rows = vec![];
    for r in &found {
        if step
            .as_ref()
            .is_some_and(|s| r.runtime.failed_step.as_ref() != Some(s))
        {
            continue;
        }
        rows.push(row(&j, ws, r)?);
        if rows.len() == limit {
            break;
        }
    }
    Ok(rows)
}

/// One run's story: how it started, each step and how it went, the
/// events it took, its input, state and journal events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    #[serde(flatten)]
    pub run: Row,
    /// Others with this name, when there are: this is the newest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_alike: Option<usize>,
    pub input: Value,
    pub state: Value,
    pub steps: Vec<Value>,
    pub events: Vec<Value>,
}

/// The run `run` names, by id or name: of two with one name, the newest.
pub fn one(ws: &Workspace, run: &str) -> Result<Story, Fault> {
    let j = Journal::open(ws)?;
    let found = j.run_by(run)?;
    let Some(r) = found.first() else {
        return Err(Fault::Missing(format!(
            "no run {run}: itero ws wf runs lists them"
        )));
    };
    let mut row = row(&j, ws, r)?;
    let mut steps = story(&j, r)?;
    // the steps running now, and what each said last
    if row.status == "running" {
        let now = progress::of_run(&ws.root, &r.id);
        for p in &now {
            steps.push(json!({
                "what": "running", "step": p["step"], "data": p["said"], "time": p["at"],
            }));
        }
        row.progress = Some(Value::Array(now));
    }
    let events = j
        .events(Some(&subject(&r.id)))?
        .iter()
        .map(|e| {
            json!({
                "seq": e.seq,
                "type": e.event.kind,
                "time": e.event.time,
                "handled": e.handled,
                "error": e.error,
                "data": e.event.data,
            })
        })
        .collect();
    Ok(Story {
        run: row,
        named_alike: (found.len() > 1).then_some(found.len()),
        input: r.input.clone(),
        state: Value::Object(r.runtime.state.clone()),
        steps,
        events,
    })
}

/// A run's steps as a story, oldest first.
fn story(j: &Journal, r: &Run) -> Result<Vec<Value>, Fault> {
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
    let mut out: Vec<Value> = vec![];
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
        let mut s = match e.event.kind.as_str() {
            "run.started" => json!({ "what": "started", "data": d }),
            "action.finished" => json!({ "what": "done", "step": step(), "data": d["output"] }),
            "action.waiting" => {
                json!({ "what": "waiting", "step": step(), "data": d["output"] })
            }
            "action.failed" => json!({ "what": "error", "step": step(), "data": d["error"] }),
            // a step whose group ended without it, as the workflow names it
            "step.stopped" => json!({ "what": "stopped", "step": d["step"] }),
            "run.stop" => json!({ "what": "stopped by command" }),
            // a workflow used as a step ended: how, what it returned, and
            // its run by name
            "workflow.ended" => {
                let child = d["run"].as_str().map(|id| j.run(id)).transpose()?.flatten();
                json!({
                    "what": d["status"],
                    "step": child.as_ref().map(|c| c.workflow.clone()),
                    "data": d["output"],
                    "run": child.map(|c| c.name),
                })
            }
            "timer.fired" => json!({ "what": "timed out" }),
            // an event is copied to every waiting run: show those it took
            "message" if r.runtime.taken.iter().any(|t| d["id"] == t.as_str()) => {
                json!({ "what": "event", "type": d["type"], "data": d["data"] })
            }
            _ => continue,
        };
        s["time"] = json!(e.event.time);
        out.push(s);
    }
    // in the journal's order when two have the same time
    out.sort_by(|a, b| a["time"].as_str().cmp(&b["time"].as_str()));
    Ok(out)
}

/// A stop asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stopped {
    /// The run's name.
    pub stopping: String,
    /// Whether a daemon runs the workspace: else the next to start stops
    /// it.
    pub daemon: bool,
}

/// Asks run `run` to stop: the daemon running the workspace stops it now,
/// or the next to start does.
pub fn stop(ws: &Workspace, run: &str) -> Result<Stopped, Fault> {
    use crate::engine::events::{StopAsked, stop_run};
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    match stop_run(ws, run, &now)? {
        StopAsked::Asked { name, daemon } => Ok(Stopped {
            stopping: name,
            daemon,
        }),
        StopAsked::Ended { name, status } => Err(Fault::Conflict(
            format!("{name} has ended: {status}"),
            serde_json::Map::new(),
        )),
        StopAsked::Missing => Err(Fault::Missing(format!(
            "no run {run}: itero ws wf runs lists them"
        ))),
    }
}
