//! The manager over steps (`docs/design-simple.md`): one decision for one
//! run and one event, JSON in, JSON out. Pure: no clock, no files, no
//! environment; the daemon gives it everything it needs and saves what it
//! answers.
//!
//! A run's position is a tree: the run's own workflow at the root, a node
//! per workflow used as a step, and one per entry of a list run side by
//! side, down to the actions running. Each job carries the path to its
//! node, starting with the phase, so a late answer from a branch already
//! left behind is recognised and ignored.
use super::{Event, Timer};
use crate::flow::{self, Flow, Name, On, Step};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// How often a waiting step runs again, to decide for itself.
const RETRY: chrono::Duration = chrono::Duration::hours(1);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Runtime {
    /// Where the run is; `None` before it starts and once it has ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    #[serde(default)]
    pub phase: Phase,
    #[serde(default)]
    pub state: Map<String, Value>,
    /// After a split: the args item this run is for, and whose it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Item>,
    /// Messages already taken, by id: one given twice counts once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taken: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
    /// The undos of the steps that finished, as written, oldest first: run
    /// newest first when a step breaks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The workflow's `steps`.
    #[default]
    Steps,
    /// Its `success` list, after the steps.
    Success,
    /// Its `failure` list: the steps ran, and the state says
    /// `"success": false`.
    Failure,
    /// Its `error` list: something broke.
    Error,
    /// Before the error list: the undos of the steps that finished, newest
    /// first.
    Undo,
}

impl Phase {
    /// The first number of a job's path.
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ended {
    Done,
    Failed,
    Error {
        why: String,
    },
    /// It went on as one run per args item.
    Split,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub action: String,
    pub value: Value,
}

/// A node of a run's position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum At {
    /// A workflow, at one of its steps.
    Flow {
        workflow: String,
        step: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner: Option<Box<At>>,
    },
    /// An action: its job is out, or waits for data.
    Action {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Map::is_empty")]
        data: Map<String, Value>,
        /// Its job was handed out (not yet, for a run just split).
        started: bool,
        #[serde(default)]
        waiting: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        waiting_since: Option<String>,
    },
    /// A list run side by side.
    Side { entries: Vec<Entry> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// `#check-payment` or `stock-check`.
    pub name: String,
    /// `None` once it is done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    /// The fields it printed, to keep per entry those more than one did.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub writes: Map<String, Value>,
}

/// Everything the manager is given for one decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepInput {
    /// The time, RFC 3339: the manager never reads a clock.
    pub now: String,
    /// The run's own workflow.
    pub workflow: String,
    /// The run's pinned workflows by name, as JSON: its own and those it
    /// uses as steps.
    pub flows: BTreeMap<String, Value>,
    /// The args files of the actions they use.
    #[serde(default)]
    pub args: BTreeMap<String, Value>,
    /// The `$$` names that are defined.
    #[serde(default)]
    pub defined: Vec<String>,
    pub runtime: Runtime,
    pub event: Event,
}

/// What the manager decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepAnswer {
    pub runtime: Runtime,
    /// Actions to run now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<StepJob>,
    /// New runs, one per args item, each going on from where this one
    /// split.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split: Vec<Runtime>,
    /// The run's timers from now on.
    pub timers: Vec<Timer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log: Vec<String>,
    /// Lines for the daemon's log, each with its `event`:
    /// `{"event": "step.skipped", "step": "#ship"}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Value>,
}

/// One action to run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepJob {
    /// Where its answer goes back to.
    pub path: Vec<usize>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The step's data in the workflow.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub data: Map<String, Value>,
    /// The args item, after a split.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    /// Since when it has been waiting, when it runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
}

/// Where a run is, or how it ended, in a word: `running`, `waiting`,
/// `done`, `failed`, `error` or `split`.
pub fn status(rt: &Runtime) -> &'static str {
    match &rt.ended {
        Some(Ended::Done) => "done",
        Some(Ended::Failed) => "failed",
        Some(Ended::Error { .. }) => "error",
        Some(Ended::Split) => "split",
        None if is_waiting(rt) => "waiting",
        None => "running",
    }
}

/// Whether a step of the run waits for data (exited 75): the runs messages
/// for the workspace are copied to.
pub fn is_waiting(rt: &Runtime) -> bool {
    let mut any = false;
    let mut at = rt.at.clone();
    if let Some(a) = at.as_mut() {
        leaves(a, &mut vec![], &mut |leaf, _| {
            any |= matches!(leaf, At::Action { waiting: true, .. });
        });
    }
    any
}

pub fn decide(input: &StepInput) -> Result<StepAnswer, String> {
    let now = DateTime::parse_from_rfc3339(&input.now)
        .map_err(|_| format!("now must be an RFC 3339 time, not `{}`", input.now))?;
    let mut flows = BTreeMap::new();
    for (n, v) in &input.flows {
        flows.insert(
            n.clone(),
            flow::from_value(v).map_err(|e| format!("{n}: {e}"))?,
        );
    }
    if !flows.contains_key(&input.workflow) {
        return Err(format!("no workflow {}", input.workflow));
    }
    let mut m = Machine {
        i: input,
        flows,
        now,
        rt: input.runtime.clone(),
        jobs: vec![],
        split: None,
        log: vec![],
        notes: vec![],
    };
    if m.rt.ended.is_some() {
        m.log
            .push(format!("{} ignored: the run has ended", input.event.kind));
    } else {
        m.handle()?;
    }
    let mut split = vec![];
    if let Some((action, items)) = m.split.take() {
        for value in items {
            let mut child = m.rt.clone();
            child.item = Some(Item {
                action: action.clone(),
                value,
            });
            split.push(child);
        }
        m.jobs.clear();
        m.end(Ended::Split);
    }
    let timers = m.timers();
    Ok(StepAnswer {
        runtime: m.rt,
        jobs: m.jobs,
        split,
        timers,
        log: m.log,
        notes: m.notes,
    })
}

struct Machine<'a> {
    i: &'a StepInput,
    flows: BTreeMap<String, Flow>,
    now: DateTime<FixedOffset>,
    rt: Runtime,
    jobs: Vec<StepJob>,
    /// An action with a list of args was reached: its name and the items.
    split: Option<(String, Vec<Value>)>,
    log: Vec<String>,
    notes: Vec<Value>,
}

impl Machine<'_> {
    fn handle(&mut self) -> Result<(), String> {
        let d = &self.i.event.data;
        match self.i.event.kind.as_str() {
            "run.started" if self.rt.at.is_none() => {
                self.rt.state = d.as_object().cloned().unwrap_or_default();
                self.rt.phase = Phase::Steps;
                self.begin()
            }
            // a run split from another: its position is there already
            "run.started" => {
                self.resume();
                Ok(())
            }
            "action.finished" => self.finished(&path(d)?, &output(d)),
            "action.waiting" => self.waiting(&path(d)?, &output(d)),
            "action.failed" => {
                let why = d
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("it failed");
                self.failed(&path(d)?, why)
            }
            "message" => self.message(d),
            "timer.fired" => self.timer(d),
            other => {
                self.log.push(format!("{other} ignored"));
                Ok(())
            }
        }
    }

    /// Starts the current phase's list from its first step.
    fn begin(&mut self) -> Result<(), String> {
        let root = self.i.workflow.clone();
        let mut p = vec![self.rt.phase.index()];
        self.rt.at = self.start_list(&root, true, 0, &mut p)?;
        if self.rt.at.is_none() {
            self.list_done()?;
        }
        Ok(())
    }

    /// The current phase's list is done: on to the next, or the end.
    fn list_done(&mut self) -> Result<(), String> {
        match self.rt.phase {
            Phase::Steps => {
                let failed = self.rt.state.get("success") == Some(&Value::Bool(false));
                self.rt.phase = if failed {
                    Phase::Failure
                } else {
                    Phase::Success
                };
                self.begin()
            }
            Phase::Success => {
                self.end(Ended::Done);
                Ok(())
            }
            Phase::Failure => {
                self.end(Ended::Failed);
                Ok(())
            }
            Phase::Error => {
                let why = self.why();
                self.end(Ended::Error { why });
                Ok(())
            }
            Phase::Undo => {
                self.rt.phase = Phase::Error;
                self.begin()
            }
        }
    }

    fn end(&mut self, e: Ended) {
        self.rt.at = None;
        self.rt.ended = Some(e);
    }

    fn why(&self) -> String {
        self.rt
            .state
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or("it failed")
            .to_string()
    }

    /// A step whose `on` does not hold: it is passed by.
    fn skipped(&mut self, step: &str) {
        self.notes.push(serde_json::json!({
            "event": "step.skipped",
            "step": step,
            "why": "its on does not hold",
        }));
    }

    /// Something broke at `step`: the undos, then the error list; or, in
    /// an end list, the end.
    fn fail(&mut self, why: &str, step: &str) -> Result<(), String> {
        self.rt.state.insert("why".into(), why.into());
        self.rt.state.insert("failed_step".into(), step.into());
        self.log.push(format!("{step}: {why}"));
        if self.rt.phase == Phase::Steps {
            self.rt.phase = if self.rt.undo.is_empty() {
                Phase::Error
            } else {
                Phase::Undo
            };
            self.begin()
        } else {
            // a step of an end list broke: no other list runs
            self.end(Ended::Error { why: why.into() });
            Ok(())
        }
    }

    /// An undo that broke: noted, and the next one runs all the same.
    fn undo_failed(&mut self, path: &[usize], step: &str, why: &str) -> Result<(), String> {
        self.notes
            .push(json!({"event": "undo.failed", "step": step, "why": why}));
        self.log.push(format!("undo {step}: {why}"));
        self.finished(path, &Map::new())
    }

    fn list(&self, workflow: &str, root: bool) -> Result<Vec<Step>, String> {
        let f = &self.flows[workflow];
        if !root {
            return Ok(f.steps.clone());
        }
        Ok(match self.rt.phase {
            Phase::Steps => f.steps.clone(),
            Phase::Success => f.success.clone(),
            Phase::Failure => f.failure.clone(),
            Phase::Error => f.error.clone(),
            Phase::Undo => self
                .rt
                .undo
                .iter()
                .rev()
                .map(|u| {
                    Ok(Step::One {
                        name: flow::name(u)?,
                        data: Map::new(),
                        on: None,
                        undo: None,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }

    fn holds(&self, on: &On) -> bool {
        let defined = |n: &str| self.i.defined.iter().any(|d| d == n);
        flow::holds(on, &self.rt.state, &defined)
    }

    /// Starts `workflow`'s list from step `from`, skipping the steps whose
    /// `on` does not hold: its node, or `None` when nothing is left to run.
    fn start_list(
        &mut self,
        workflow: &str,
        root: bool,
        from: usize,
        path: &mut Vec<usize>,
    ) -> Result<Option<At>, String> {
        let steps = self.list(workflow, root)?;
        for (idx, step) in steps.iter().enumerate().skip(from) {
            path.push(idx);
            let node = self.start_step(step, path)?;
            path.pop();
            if let Some(n) = node {
                return Ok(Some(At::Flow {
                    workflow: workflow.into(),
                    step: idx,
                    inner: Some(Box::new(n)),
                }));
            }
        }
        Ok(None)
    }

    fn start_step(&mut self, step: &Step, path: &mut Vec<usize>) -> Result<Option<At>, String> {
        match step {
            Step::One { name, data, on, .. } => {
                if let Some(on) = on
                    && !self.holds(on)
                {
                    self.skipped(&label(name));
                    return Ok(None);
                }
                self.start_name(name, data, path, false)
            }
            Step::Side(names) => {
                let mut entries = vec![];
                for (j, n) in names.iter().enumerate() {
                    path.push(j);
                    let at = self.start_name(n, &Map::new(), path, true)?;
                    path.pop();
                    entries.push(Entry {
                        name: label(n),
                        at,
                        writes: Map::new(),
                    });
                }
                Ok(entries
                    .iter()
                    .any(|e| e.at.is_some())
                    .then_some(At::Side { entries }))
            }
        }
    }

    fn start_name(
        &mut self,
        name: &Name,
        data: &Map<String, Value>,
        path: &mut Vec<usize>,
        side: bool,
    ) -> Result<Option<At>, String> {
        match name {
            Name::Action { name, version } => {
                let mut leaf = At::Action {
                    name: name.clone(),
                    version: version.clone(),
                    data: data.clone(),
                    started: false,
                    waiting: false,
                    waiting_since: None,
                };
                let mine = self.rt.item.as_ref().is_some_and(|it| it.action == *name);
                if let Some(Value::Array(items)) = self.i.args.get(name)
                    && !mine
                {
                    if side || self.split.is_some() {
                        return Err(format!(
                            "#{name}: a list of args splits a run, which a step side by side cannot"
                        ));
                    }
                    self.split = Some((name.clone(), items.clone()));
                    return Ok(Some(leaf));
                }
                self.jobs
                    .extend(job(&mut leaf, path, self.rt.item.as_ref()));
                Ok(Some(leaf))
            }
            Name::Workflow(w) => {
                let Some(f) = self.flows.get(w) else {
                    return Err(format!("no workflow {w}"));
                };
                if !self.holds(&f.on.clone()) {
                    self.skipped(w);
                    return Ok(None);
                }
                self.start_list(w, false, 0, path)
            }
        }
    }

    /// Hands out the jobs of a run split from another.
    fn resume(&mut self) {
        let item = self.rt.item.clone();
        let mut jobs = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if matches!(leaf, At::Action { started: false, .. }) {
                    jobs.extend(job(leaf, p, item.as_ref()));
                }
            });
        }
        self.jobs.extend(jobs);
    }

    /// The leaf at `path`, if it is one of this run's current actions,
    /// recording `out` in the lists side by side above it.
    fn leaf(&mut self, path: &[usize], out: &Map<String, Value>) -> Option<&mut At> {
        let (&phase, rest) = path.split_first()?;
        if phase != self.rt.phase.index() {
            return None;
        }
        let leaf = find(self.rt.at.as_mut()?, rest, out)?;
        matches!(leaf, At::Action { started: true, .. }).then_some(leaf)
    }

    fn finished(&mut self, path: &[usize], out: &Map<String, Value>) -> Result<(), String> {
        if self.leaf(path, out).is_none() {
            self.log
                .push(format!("an answer for {path:?} ignored: not a step now"));
            return Ok(());
        }
        merge(&mut self.rt.state, out);
        let at = self.rt.at.take().expect("found");
        self.rt.at = self.advance(at, &path[1..], vec![path[0]], 0)?;
        if self.rt.at.is_none() {
            self.list_done()?;
        }
        Ok(())
    }

    /// The node after the leaf at `path` (under `node`) is done: the same
    /// node going on, or `None` when it is done too.
    fn advance(
        &mut self,
        node: At,
        path: &[usize],
        prefix: Vec<usize>,
        depth: usize,
    ) -> Result<Option<At>, String> {
        match node {
            At::Action { .. } => Ok(None),
            At::Flow {
                workflow,
                step,
                inner,
            } => {
                let mut p = prefix.clone();
                p.push(step);
                let next = match inner {
                    Some(i) => self.advance(*i, &path[1..], p, depth + 1)?,
                    None => None,
                };
                match next {
                    Some(n) => Ok(Some(At::Flow {
                        workflow,
                        step,
                        inner: Some(Box::new(n)),
                    })),
                    None => {
                        // the step finished: what takes it back, if the run
                        // breaks later
                        if self.rt.phase == Phase::Steps
                            && let Some(Step::One { undo: Some(u), .. }) =
                                self.flows[&workflow].steps.get(step)
                        {
                            self.rt.undo.push(written(u));
                        }
                        let mut p = prefix;
                        self.start_list(&workflow, depth == 0, step + 1, &mut p)
                    }
                }
            }
            At::Side { mut entries } => {
                let j = path[0];
                let mut p = prefix;
                p.push(j);
                if let Some(at) = entries[j].at.take() {
                    entries[j].at = self.advance(at, &path[1..], p, depth + 1)?;
                }
                if entries.iter().any(|e| e.at.is_some()) {
                    return Ok(Some(At::Side { entries }));
                }
                self.keep_per_entry(&entries);
                Ok(None)
            }
        }
    }

    /// A field more than one entry printed is kept per entry.
    fn keep_per_entry(&mut self, entries: &[Entry]) {
        let mut by: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
        for e in entries {
            for (k, v) in &e.writes {
                by.entry(k.clone())
                    .or_default()
                    .insert(e.name.clone(), v.clone());
            }
        }
        for (k, values) in by {
            if values.len() > 1 {
                self.rt.state.insert(k, Value::Object(values));
            }
        }
    }

    fn waiting(&mut self, path: &[usize], out: &Map<String, Value>) -> Result<(), String> {
        let now = stamp(self.now);
        let Some(At::Action {
            waiting,
            waiting_since,
            ..
        }) = self.leaf(path, out)
        else {
            self.log
                .push(format!("a wait for {path:?} ignored: not a step now"));
            return Ok(());
        };
        *waiting = true;
        waiting_since.get_or_insert(now);
        merge(&mut self.rt.state, out);
        Ok(())
    }

    fn failed(&mut self, path: &[usize], why: &str) -> Result<(), String> {
        let Some(At::Action { name, .. }) = self.leaf(path, &Map::new()) else {
            self.log
                .push(format!("a failure for {path:?} ignored: not a step now"));
            return Ok(());
        };
        let step = format!("#{name}");
        if self.rt.phase == Phase::Undo {
            return self.undo_failed(path, &step, why);
        }
        self.fail(why, &step)
    }

    /// The waiting actions: their paths and names.
    fn waiting_leaves(&mut self) -> Vec<(Vec<usize>, String, Option<String>)> {
        let mut out = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if let At::Action {
                    name,
                    waiting: true,
                    waiting_since,
                    ..
                } = leaf
                {
                    out.push((p.to_vec(), name.clone(), waiting_since.clone()));
                }
            });
        }
        out
    }

    /// Runs the waiting actions again.
    fn wake(&mut self) {
        let item = self.rt.item.clone();
        let mut jobs = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if matches!(leaf, At::Action { waiting: true, .. }) {
                    jobs.extend(job(leaf, p, item.as_ref()));
                }
            });
        }
        self.jobs.extend(jobs);
    }

    fn message(&mut self, d: &Value) -> Result<(), String> {
        let id = d
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let data = d
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if self.rt.taken.contains(&id) {
            self.log
                .push(format!("message {id} ignored: taken already"));
            return Ok(());
        }
        if self.waiting_leaves().is_empty() {
            self.log
                .push(format!("message {id} ignored: no step waits"));
            return Ok(());
        }
        if !ours(&self.rt.state, &data) {
            self.log
                .push(format!("message {id} ignored: not this run's"));
            return Ok(());
        }
        merge(&mut self.rt.state, &data);
        self.rt.taken.push(id);
        self.wake();
        Ok(())
    }

    fn timer(&mut self, d: &Value) -> Result<(), String> {
        match d.get("kind").and_then(Value::as_str) {
            Some("retry") => {
                self.wake();
                Ok(())
            }
            Some("deadline") => {
                let limit = self.timeout();
                let late = self.waiting_leaves().into_iter().find(|(_, _, since)| {
                    since
                        .as_deref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|s| s + limit <= self.now)
                });
                match late {
                    Some((path, name, _)) => {
                        let wait = humantime::format_duration(limit.to_std().unwrap_or_default());
                        let (why, step) = (format!("waited {wait} for data"), format!("#{name}"));
                        if self.rt.phase == Phase::Undo {
                            return self.undo_failed(&path, &step, &why);
                        }
                        self.fail(&why, &step)
                    }
                    None => Ok(()),
                }
            }
            _ => {
                self.log.push("a timer of no kind ignored".into());
                Ok(())
            }
        }
    }

    fn timeout(&self) -> chrono::Duration {
        let t = self.flows[&self.i.workflow].timeout.unwrap_or(flow::WAIT);
        chrono::Duration::from_std(t).unwrap_or(chrono::Duration::days(1))
    }

    /// The run's timers: none unless a step waits; then the hourly retry
    /// and the deadline of the longest waiting.
    fn timers(&mut self) -> Vec<Timer> {
        if self.rt.ended.is_some() {
            return vec![];
        }
        let since = self
            .waiting_leaves()
            .into_iter()
            .filter_map(|(_, _, s)| s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()))
            .min();
        let Some(since) = since else {
            return vec![];
        };
        vec![
            Timer {
                at: stamp(self.now + RETRY),
                data: json!({"kind": "retry"}),
            },
            Timer {
                at: stamp(since + self.timeout()),
                data: json!({"kind": "deadline"}),
            },
        ]
    }
}

/// `#name` for an action, the name for a workflow.
fn label(n: &Name) -> String {
    match n {
        Name::Action { name, .. } => format!("#{name}"),
        Name::Workflow(w) => w.clone(),
    }
}

/// A name as a workflow writes it: `#name@0.2` for an action, the name for
/// a workflow.
fn written(n: &Name) -> String {
    match n {
        Name::Action {
            name,
            version: Some(v),
        } => format!("#{name}@{v}"),
        other => label(other),
    }
}

/// A leaf's job, handing it out.
fn job(leaf: &mut At, path: &[usize], item: Option<&Item>) -> Option<StepJob> {
    let At::Action {
        name,
        version,
        data,
        started,
        waiting,
        waiting_since,
    } = leaf
    else {
        return None;
    };
    *started = true;
    *waiting = false;
    Some(StepJob {
        path: path.to_vec(),
        action: name.clone(),
        version: version.clone(),
        data: data.clone(),
        item: item
            .filter(|it| it.action == *name)
            .map(|it| it.value.clone()),
        waiting_since: waiting_since.clone(),
    })
}

/// Calls `f` on every action under `node`, with its path.
fn leaves(node: &mut At, path: &mut Vec<usize>, f: &mut dyn FnMut(&mut At, &[usize])) {
    match node {
        At::Action { .. } => f(node, path),
        At::Flow { step, inner, .. } => {
            if let Some(i) = inner {
                path.push(*step);
                leaves(i, path, f);
                path.pop();
            }
        }
        At::Side { entries } => {
            for (j, e) in entries.iter_mut().enumerate() {
                if let Some(at) = e.at.as_mut() {
                    path.push(j);
                    leaves(at, path, f);
                    path.pop();
                }
            }
        }
    }
}

/// The action at `path` under `node`, recording `out` in each list side by
/// side on the way.
fn find<'n>(node: &'n mut At, path: &[usize], out: &Map<String, Value>) -> Option<&'n mut At> {
    if matches!(node, At::Action { .. }) {
        return path.is_empty().then_some(node);
    }
    let (&first, rest) = path.split_first()?;
    match node {
        At::Flow { step, inner, .. } => {
            if first != *step {
                return None;
            }
            find(inner.as_mut()?, rest, out)
        }
        At::Side { entries } => {
            let Entry { at, writes, .. } = entries.get_mut(first)?;
            let leaf = find(at.as_mut()?, rest, out)?;
            for (k, v) in out {
                if !v.is_null() {
                    writes.insert(k.clone(), v.clone());
                }
            }
            Some(leaf)
        }
        At::Action { .. } => None,
    }
}

/// Adds `out` to the state; a field set to null is removed.
fn merge(state: &mut Map<String, Value>, out: &Map<String, Value>) {
    for (k, v) in out {
        if v.is_null() {
            state.remove(k);
        } else {
            state.insert(k.clone(), v.clone());
        }
    }
}

/// Whether a message is this run's: it shares a field with the state, and
/// agrees with it on every field they share.
fn ours(state: &Map<String, Value>, data: &Map<String, Value>) -> bool {
    let mut shared = 0;
    for (k, v) in data {
        if let Some(s) = state.get(k) {
            if s != v {
                return false;
            }
            shared += 1;
        }
    }
    shared > 0
}

fn path(d: &Value) -> Result<Vec<usize>, String> {
    serde_json::from_value(d.get("path").cloned().unwrap_or(Value::Null))
        .map_err(|_| "an answer without its path".to_string())
}

fn output(d: &Value) -> Map<String, Value> {
    d.get("output")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn stamp(t: DateTime<FixedOffset>) -> String {
    t.with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
