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
use crate::flow::{self, Fail, Flow, Name, Need, On, Step, Wait};
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
    /// The undos of the steps that finished, oldest first: run newest
    /// first when a step breaks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo: Vec<Undo>,
}

/// What takes a finished step back, as written, and that step's
/// `$output`, which it is given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "UndoForm")]
pub struct Undo {
    pub step: String,
    #[serde(default)]
    pub output: Value,
}

/// An undo as a journal holds it: a name alone, as runs before `$output`
/// kept it, or with its step's output.
#[derive(Deserialize)]
#[serde(untagged)]
enum UndoForm {
    Name(String),
    Full {
        step: String,
        #[serde(default)]
        output: Value,
    },
}

impl From<UndoForm> for Undo {
    fn from(f: UndoForm) -> Undo {
        match f {
            UndoForm::Name(step) => Undo {
                step,
                output: Value::Null,
            },
            UndoForm::Full { step, output } => Undo { step, output },
        }
    }
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
        /// Its own `$output` when it runs again: what it printed waiting,
        /// or the message taken for it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
    },
    /// A list run side by side, and how the next step waits for it.
    Side {
        #[serde(default)]
        wait: Wait,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fail: Option<Fail>,
        entries: Vec<Entry>,
        /// The step it broke at, while it waits for the steps still running
        /// to end (`all`, `settled`): it breaks once none runs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        breaking: Option<Broken>,
    },
}

/// A step that broke, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Broken {
    pub step: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// `#check-payment` or `stock-check`.
    pub name: String,
    /// `None` once it has ended: done, broke or stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    /// Its `$output` once it is done, for the list's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    /// How it ended; `None` while it runs (and, in a run written before
    /// groups said how to wait, once it is done).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    /// Which end it was in the group, counting from 1: how the group tells
    /// the first from the rest. 0 while it runs.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub order: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// How an entry of a group ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Done,
    /// A step of it broke: that step's name, and why.
    Broke {
        step: String,
        why: String,
    },
    /// The group ended without it: it was running or waiting.
    Stopped,
    /// It did not run (a workflow whose `on` does not hold, or none of
    /// whose steps ran): out of the group, as if not written.
    Skipped,
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
    /// The paths (as in `StepJob::path`) of the actions a group stopped in
    /// this decision, running, waiting or just started: their jobs are to
    /// be ended, and their answers are ignored from now on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped: Vec<Vec<usize>>,
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
    /// Its own `$output` when it runs again: what it printed waiting, or
    /// the message that came for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
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
        broke: None,
        log: vec![],
        notes: vec![],
        stopped: vec![],
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
        stopped: m.stopped,
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
    /// A step that could not start, and why: the run breaks there once the
    /// event is handled.
    broke: Option<(String, String)>,
    log: Vec<String>,
    notes: Vec<Value>,
    stopped: Vec<Vec<usize>>,
}

impl Machine<'_> {
    fn handle(&mut self) -> Result<(), String> {
        self.event()?;
        while let Some((why, step)) = self.broke.take() {
            self.jobs.clear();
            self.fail(&why, &step)?;
        }
        Ok(())
    }

    fn event(&mut self) -> Result<(), String> {
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
        if self.rt.at.is_none() && self.broke.is_none() {
            self.list_done()?;
        }
        Ok(())
    }

    /// The current phase's list is done: on to the next, or the end.
    fn list_done(&mut self) -> Result<(), String> {
        match self.rt.phase {
            Phase::Steps => {
                let no = Some(&Value::Bool(false));
                let failed = self.rt.state.get("success") == no
                    || flow::lookup(&self.rt.state, "output.success") == no;
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

    /// An undo that broke: noted, in the log and in `undo_failed` for the
    /// error list (what it made may be left), and the next one runs all
    /// the same.
    fn undo_failed(&mut self, path: &[usize], step: &str, why: &str) -> Result<(), String> {
        self.notes
            .push(json!({"event": "undo.failed", "step": step, "why": why}));
        let line = Value::String(format!("{step}: {why}"));
        match self.rt.state.get_mut("undo_failed") {
            Some(Value::Array(lines)) => lines.push(line),
            _ => {
                self.rt
                    .state
                    .insert("undo_failed".into(), Value::Array(vec![line]));
            }
        }
        self.log.push(format!("undo {step}: {why}"));
        self.finished(path, &Value::String(String::new()))
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
                        name: flow::name(&u.step)?,
                        args: Value::Object(Map::new()),
                        on: None,
                        undo: None,
                        strict: false,
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
            if root && self.rt.phase == Phase::Undo {
                // an undo is given the result of the step it takes back
                let u = &self.rt.undo[self.rt.undo.len() - 1 - idx];
                self.rt.state.insert("output".into(), u.output.clone());
            }
            path.push(idx);
            let node = self.start_step(step, path)?;
            path.pop();
            if self.broke.is_some() {
                return Ok(None);
            }
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
            Step::One {
                name,
                args,
                on,
                strict,
                ..
            } => {
                if let Some(on) = on
                    && !self.holds(on)
                {
                    self.skipped(&label(name));
                    return Ok(None);
                }
                let data = match self.args(args, *strict) {
                    Ok(data) => data,
                    Err(why) => {
                        // it cannot start: the run breaks here
                        self.broke = Some((why, label(name)));
                        return Ok(None);
                    }
                };
                self.start_name(name, &data, path, false)
            }
            Step::Side(group) => {
                let mut entries: Vec<Entry> = vec![];
                for (j, m) in group.members.iter().enumerate() {
                    path.push(j);
                    let at = if m.on.as_ref().is_some_and(|on| !self.holds(on)) {
                        self.skipped(&m.label);
                        None
                    } else {
                        match self.args(&m.args, m.strict) {
                            Ok(data) => self.start_name(&m.name, &data, path, true)?,
                            Err(why) => {
                                // it cannot start: that step of the group breaks
                                self.broke = Some((why, m.label.clone()));
                                None
                            }
                        }
                    };
                    path.pop();
                    let mut e = Entry {
                        name: m.label.clone(),
                        at,
                        output: None,
                        outcome: None,
                        order: 0,
                    };
                    if let Some((why, step)) = self.broke.take() {
                        // it cannot start: that step of the group broke
                        e.outcome = Some(Outcome::Broke { step, why });
                        e.order = next_order(&entries);
                    } else if e.at.is_none() {
                        // it did not run: out of the group
                        e.outcome = Some(Outcome::Skipped);
                    }
                    entries.push(e);
                }
                if entries.iter().all(|e| e.outcome == Some(Outcome::Skipped)) {
                    // nothing ran
                    return Ok(None);
                }
                Ok(
                    match self.settle(group.wait.clone(), group.fail.clone(), entries, path) {
                        Adv::Going(at) => Some(at),
                        Adv::Done => None,
                        Adv::Broke { step, why } => {
                            self.broke = Some((why, step));
                            None
                        }
                    },
                )
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
                    output: None,
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

    /// The leaf at `path`, if it is one of this run's current actions.
    fn leaf(&mut self, path: &[usize]) -> Option<&mut At> {
        let (&phase, rest) = path.split_first()?;
        if phase != self.rt.phase.index() {
            return None;
        }
        let leaf = find(self.rt.at.as_mut()?, rest)?;
        matches!(leaf, At::Action { started: true, .. }).then_some(leaf)
    }

    /// A step's args filled from the state, an object; unless the step is
    /// strict, they join the state too.
    fn args(&mut self, args: &Value, strict: bool) -> Result<Map<String, Value>, String> {
        let data = flow::fill_args(args, &self.rt.state)?;
        if data.contains_key("output") {
            return Err("args: output is given by itero, the step before's result".into());
        }
        if !strict {
            for (k, v) in &data {
                self.rt.state.insert(k.clone(), v.clone());
            }
        }
        Ok(data)
    }

    fn finished(&mut self, path: &[usize], out: &Value) -> Result<(), String> {
        if self.leaf(path).is_none() {
            self.log
                .push(format!("an answer for {path:?} ignored: not a step now"));
            return Ok(());
        }
        self.settle_leaf(path, Leaf::Done(out.clone()))
    }

    /// The action at `path` ended, well or not: its place in the position
    /// is settled, and the run goes on, or breaks.
    fn settle_leaf(&mut self, path: &[usize], leaf: Leaf) -> Result<(), String> {
        let at = self.rt.at.take().expect("found");
        match self.advance(at, &path[1..], vec![path[0]], 0, &leaf)? {
            Adv::Going(at) => {
                self.rt.at = Some(at);
                Ok(())
            }
            Adv::Done => self.list_done(),
            Adv::Broke { step, why } => self.fail(&why, &step),
        }
    }

    /// The node after the leaf at `path` (under `node`) ended is settled:
    /// the same node going on, or done, or broken.
    fn advance(
        &mut self,
        node: At,
        path: &[usize],
        prefix: Vec<usize>,
        depth: usize,
        leaf: &Leaf,
    ) -> Result<Adv, String> {
        match node {
            At::Action { .. } => Ok(match leaf {
                Leaf::Done(out) => {
                    self.rt.state.insert("output".into(), out.clone());
                    Adv::Done
                }
                Leaf::Broke { step, why } => Adv::Broke {
                    step: step.clone(),
                    why: why.clone(),
                },
                // only a group that is breaking hears of a wait
                Leaf::Waits => Adv::Going(node),
            }),
            At::Flow {
                workflow,
                step,
                inner,
            } => {
                let mut p = prefix.clone();
                p.push(step);
                let next = match inner {
                    Some(i) => self.advance(*i, &path[1..], p, depth + 1, leaf)?,
                    None => Adv::Done,
                };
                match next {
                    Adv::Going(n) => Ok(Adv::Going(At::Flow {
                        workflow,
                        step,
                        inner: Some(Box::new(n)),
                    })),
                    broke @ Adv::Broke { .. } => Ok(broke),
                    Adv::Done => {
                        // the step finished: what takes it back, if the run
                        // breaks later
                        if self.rt.phase == Phase::Steps
                            && let Some(Step::One { undo: Some(u), .. }) =
                                self.flows[&workflow].steps.get(step)
                        {
                            self.rt.undo.push(Undo {
                                step: written(u),
                                output: self.rt.state.get("output").cloned().unwrap_or_default(),
                            });
                        }
                        let mut p = prefix;
                        let next = self.start_list(&workflow, depth == 0, step + 1, &mut p)?;
                        if let Some((why, step)) = self.broke.take() {
                            return Ok(Adv::Broke { step, why });
                        }
                        Ok(match next {
                            Some(n) => Adv::Going(n),
                            None => Adv::Done,
                        })
                    }
                }
            }
            At::Side {
                wait,
                fail,
                mut entries,
                breaking: Some(b),
            } => {
                // what a step still running gives is not kept, and it goes
                // no further: once none runs, the group breaks
                let j = path[0];
                let target: Vec<usize> = prefix.iter().chain(path).copied().collect();
                let mut p = prefix;
                p.push(j);
                if let Some(at) = entries[j].at.take() {
                    entries[j].at = prune(at, &mut p, &mut |_, at| at == target, &mut vec![]);
                    if entries[j].at.is_none() {
                        entries[j].outcome = Some(match leaf {
                            Leaf::Done(_) => Outcome::Done,
                            Leaf::Broke { step, why } => Outcome::Broke {
                                step: step.clone(),
                                why: why.clone(),
                            },
                            Leaf::Waits => {
                                self.stopped_note(&entries[j].name);
                                Outcome::Stopped
                            }
                        });
                    }
                }
                Ok(if entries.iter().any(|e| e.at.is_some()) {
                    Adv::Going(At::Side {
                        wait,
                        fail,
                        entries,
                        breaking: Some(b),
                    })
                } else {
                    Adv::Broke {
                        step: b.step,
                        why: b.why,
                    }
                })
            }
            At::Side {
                wait,
                fail,
                mut entries,
                breaking: None,
            } => {
                let j = path[0];
                let mut p = prefix.clone();
                p.push(j);
                if let Some(at) = entries[j].at.take() {
                    match self.advance(at, &path[1..], p, depth + 1, leaf)? {
                        Adv::Going(at) => entries[j].at = Some(at),
                        Adv::Done => {
                            entries[j].outcome = Some(Outcome::Done);
                            entries[j].output = self.rt.state.get("output").cloned();
                            entries[j].order = next_order(&entries);
                        }
                        Adv::Broke { step, why } => {
                            entries[j].outcome = Some(Outcome::Broke { step, why });
                            entries[j].order = next_order(&entries);
                        }
                    }
                }
                Ok(self.settle(wait, fail, entries, &prefix))
            }
        }
    }

    /// A group after a change: it goes on, or it ends (the steps still
    /// running are stopped, and `$output` is its result), or it broke (the
    /// steps still running are stopped, but in `all` and `settled`, which
    /// wait for them to end first). `at` is the group's own path.
    fn settle(
        &mut self,
        wait: Wait,
        fail: Option<Fail>,
        mut entries: Vec<Entry>,
        at: &[usize],
    ) -> Adv {
        match judge(&wait, &fail, &entries) {
            Verdict::Going => Adv::Going(At::Side {
                wait,
                fail,
                entries,
                breaking: None,
            }),
            Verdict::Ended => {
                self.stop(&mut entries, at);
                let out = group_output(&wait, &fail, &entries);
                self.rt.state.insert("output".into(), out);
                Adv::Done
            }
            // all and settled wait for every step, even once one broke them
            Verdict::Broke { step, why } if matches!(wait, Wait::All | Wait::Settled) => {
                self.drain(&mut entries, at);
                if entries.iter().all(|e| e.at.is_none()) {
                    return Adv::Broke { step, why };
                }
                Adv::Going(At::Side {
                    wait,
                    fail,
                    entries,
                    breaking: Some(Broken { step, why }),
                })
            }
            Verdict::Broke { step, why } => {
                self.stop(&mut entries, at);
                Adv::Broke { step, why }
            }
        }
    }

    /// A group that broke, waiting for its steps still running: they run
    /// to their end; the rest are stopped (a waiting one waits no more, a
    /// job not yet handed out is dropped). `at` is the group's own path.
    fn drain(&mut self, entries: &mut [Entry], at: &[usize]) {
        for (j, e) in entries.iter_mut().enumerate() {
            let Some(node) = e.at.take() else {
                continue;
            };
            let mut p = at.to_vec();
            p.push(j);
            let mut gone = vec![];
            let given_now: Vec<Vec<usize>> = self.jobs.iter().map(|job| job.path.clone()).collect();
            let runs = |leaf: &At, at: &[usize]| {
                matches!(
                    leaf,
                    At::Action {
                        started: true,
                        waiting: false,
                        ..
                    }
                ) && !given_now.iter().any(|g| g == at)
            };
            e.at = prune(node, &mut p, &mut |leaf, at| !runs(leaf, at), &mut gone);
            self.jobs.retain(|job| !gone.contains(&job.path));
            self.stopped.extend(gone);
            if e.at.is_none() {
                e.outcome = Some(Outcome::Stopped);
                self.stopped_note(&e.name);
            }
        }
    }

    /// Notes that the group's entry `name` was stopped.
    fn stopped_note(&mut self, name: &str) {
        self.notes
            .push(json!({"event": "step.stopped", "step": name}));
    }

    /// The entries still running or waiting are stopped: their actions
    /// leave the position (so nothing they answer reaches the run), their
    /// jobs not yet handed out are dropped, and the daemon is told which
    /// to end. `at` is the group's own path.
    fn stop(&mut self, entries: &mut [Entry], at: &[usize]) {
        for (j, e) in entries.iter_mut().enumerate() {
            let Some(mut node) = e.at.take() else {
                continue;
            };
            let mut gone = vec![];
            let mut p = at.to_vec();
            p.push(j);
            leaves(&mut node, &mut p, &mut |_, leaf| gone.push(leaf.to_vec()));
            self.jobs.retain(|job| !gone.contains(&job.path));
            self.stopped.extend(gone);
            e.outcome = Some(Outcome::Stopped);
            self.stopped_note(&e.name);
        }
    }

    fn waiting(&mut self, path: &[usize], out: &Value) -> Result<(), String> {
        let now = stamp(self.now);
        // a step that starts to wait in a group that broke runs no more
        if self.leaf(path).is_some()
            && let Some(at) = &self.rt.at
            && breaking(at, &path[1..])
        {
            return self.settle_leaf(path, Leaf::Waits);
        }
        let Some(At::Action {
            waiting,
            waiting_since,
            output,
            ..
        }) = self.leaf(path)
        else {
            self.log
                .push(format!("a wait for {path:?} ignored: not a step now"));
            return Ok(());
        };
        *waiting = true;
        waiting_since.get_or_insert(now);
        *output = Some(out.clone());
        Ok(())
    }

    fn failed(&mut self, path: &[usize], why: &str) -> Result<(), String> {
        let Some(At::Action { name, .. }) = self.leaf(path) else {
            self.log
                .push(format!("a failure for {path:?} ignored: not a step now"));
            return Ok(());
        };
        let step = format!("#{name}");
        if self.rt.phase == Phase::Undo {
            return self.undo_failed(path, &step, why);
        }
        self.settle_leaf(
            path,
            Leaf::Broke {
                step,
                why: why.into(),
            },
        )
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
        // taken by each waiting step whose output it agrees with: it is
        // that step's output, and the step runs again
        let item = self.rt.item.clone();
        let mut jobs = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if let At::Action {
                    waiting: true,
                    output,
                    ..
                } = leaf
                    && output
                        .as_ref()
                        .and_then(Value::as_object)
                        .is_some_and(|o| ours(o, &data))
                {
                    *output = Some(Value::Object(data.clone()));
                    jobs.extend(job(leaf, p, item.as_ref()));
                }
            });
        }
        if jobs.is_empty() {
            self.log
                .push(format!("message {id} ignored: not this run's"));
            return Ok(());
        }
        self.rt.taken.push(id);
        self.jobs.extend(jobs);
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
                let late: Vec<_> = self
                    .waiting_leaves()
                    .into_iter()
                    .filter(|(_, _, since)| {
                        since
                            .as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .is_some_and(|s| s + limit <= self.now)
                    })
                    .collect();
                for (path, name, _) in late {
                    // one that a group stopped, or a run that went on, is not
                    // a step now
                    if self.leaf(&path).is_none() {
                        continue;
                    }
                    let wait = humantime::format_duration(limit.to_std().unwrap_or_default());
                    let (why, step) = (format!("waited {wait} for data"), format!("#{name}"));
                    if self.rt.phase == Phase::Undo {
                        self.undo_failed(&path, &step, &why)?;
                    } else {
                        self.settle_leaf(&path, Leaf::Broke { step, why })?;
                    }
                }
                Ok(())
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

/// How an action ended, for the position to settle.
enum Leaf {
    /// Its result, its `$output`.
    Done(Value),
    Broke {
        step: String,
        why: String,
    },
    /// It waits for data, in a group that broke.
    Waits,
}

/// A node of the position after a leaf ended.
enum Adv {
    Going(At),
    Done,
    /// A step broke: its name, and why.
    Broke {
        step: String,
        why: String,
    },
}

/// What a group decides after a change.
enum Verdict {
    Going,
    Ended,
    Broke { step: String, why: String },
}

impl Entry {
    /// It ended well (a run written before groups said how to wait kept an
    /// entry that was done with no outcome).
    fn is_ok(&self) -> bool {
        match &self.outcome {
            Some(o) => *o == Outcome::Done,
            None => self.at.is_none(),
        }
    }

    fn broke(&self) -> bool {
        matches!(self.outcome, Some(Outcome::Broke { .. }))
    }

    fn ended(&self) -> bool {
        self.is_ok() || self.broke()
    }
}

/// The order number of the next entry to end.
fn next_order(entries: &[Entry]) -> usize {
    entries.iter().map(|e| e.order).max().unwrap_or(0) + 1
}

/// Whether a group goes on, ended, or broke, over its entries: its wait's
/// rule, or `fail`'s in its place for when it breaks.
fn judge(wait: &Wait, fail: &Option<Fail>, entries: &[Entry]) -> Verdict {
    // a step that did not run is out of the group
    let n = entries
        .iter()
        .filter(|e| e.outcome != Some(Outcome::Skipped))
        .count();
    let good = entries.iter().filter(|e| e.is_ok()).count();
    let bad = entries.iter().filter(|e| e.broke()).count();
    let running = entries.iter().filter(|e| e.at.is_some()).count();
    let first = |keep: &dyn Fn(&Entry) -> bool| {
        entries
            .iter()
            .filter(|e| e.broke() && keep(e))
            .min_by_key(|e| e.order)
    };
    let any = |_: &Entry| true;
    let named = |names: &[String]| {
        let names = names.to_vec();
        move |e: &Entry| names.contains(&e.name)
    };
    let culprit = match (fail, wait) {
        (Some(Fail::Any), _) | (None, Wait::All) => first(&any),
        (Some(Fail::All), _) | (None, Wait::Any) => first(&any).filter(|_| bad == n),
        (Some(Fail::Never), _) | (None, Wait::Settled) => None,
        (Some(Fail::Steps(names)), _) | (None, Wait::Some(Need::Names(names))) => {
            first(&named(names))
        }
        (None, Wait::Race) => entries
            .iter()
            .filter(|e| e.ended())
            .min_by_key(|e| e.order)
            .filter(|e| e.broke()),
        (None, Wait::Some(Need::Count(c))) => first(&any).filter(|_| bad > n.saturating_sub(*c)),
    };
    if let Some(Entry {
        outcome: Some(Outcome::Broke { step, why }),
        ..
    }) = culprit
    {
        return Verdict::Broke {
            step: step.clone(),
            why: why.clone(),
        };
    }
    let satisfied = match wait {
        Wait::All => good == n,
        Wait::Race => good + bad > 0,
        Wait::Any => good > 0,
        Wait::Some(Need::Count(c)) => good >= *c,
        Wait::Some(Need::Names(names)) => names
            .iter()
            .all(|nm| entries.iter().any(|e| e.is_ok() && e.name == *nm)),
        Wait::Settled => running == 0,
    };
    // with `fail` replacing the rule, a group whose wait cannot be met waits
    // for the rest to end, as `settled` does
    if satisfied || running == 0 {
        Verdict::Ended
    } else {
        Verdict::Going
    }
}

/// The `$output` of a group that ended: the results of its steps, and
/// itero's own keys (they win over a step's field of the same name).
fn group_output(wait: &Wait, fail: &Option<Fail>, entries: &[Entry]) -> Value {
    let winner = match wait {
        Wait::Race => entries
            .iter()
            .filter(|e| e.ended())
            .min_by_key(|e| e.order)
            .filter(|e| e.is_ok()),
        Wait::Any => entries.iter().filter(|e| e.is_ok()).min_by_key(|e| e.order),
        _ => None,
    };
    let mut out = match (wait, winner) {
        (Wait::Race | Wait::Any, Some(w)) => side_output(std::slice::from_ref(w)),
        (Wait::Race | Wait::Any, None) => Value::Object(Map::new()),
        _ => side_output(entries),
    };
    let Value::Object(fields) = &mut out else {
        unreachable!("side_output is an object");
    };
    if let Some(w) = winner {
        fields.insert("winner".into(), w.name.clone().into());
    }
    // which steps broke, when one did; always for settled and fail never
    if matches!(wait, Wait::Settled)
        || matches!(fail, Some(Fail::Never))
        || entries.iter().any(Entry::broke)
    {
        let mut failed = vec![];
        let mut why = Map::new();
        for e in entries {
            if let Some(Outcome::Broke { why: w, .. }) = &e.outcome {
                failed.push(Value::String(e.name.clone()));
                why.insert(e.name.clone(), w.clone().into());
            }
        }
        fields.insert("failed".into(), Value::Array(failed));
        fields.insert("why".into(), Value::Object(why));
    }
    out
}

/// `#name` for an action, the name for a workflow.
fn label(n: &Name) -> String {
    n.label()
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
        output,
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
        output: output.clone(),
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
        At::Side { entries, .. } => {
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

/// `node` without the actions `drop` picks, given each with its path (from
/// `path`, the node's own): theirs go to `gone`. `None` once no action is
/// left under it.
fn prune(
    node: At,
    path: &mut Vec<usize>,
    drop: &mut dyn FnMut(&At, &[usize]) -> bool,
    gone: &mut Vec<Vec<usize>>,
) -> Option<At> {
    match node {
        At::Action { .. } => {
            if drop(&node, path) {
                gone.push(path.clone());
                return None;
            }
            Some(node)
        }
        At::Flow {
            workflow,
            step,
            inner,
        } => {
            path.push(step);
            let inner = inner.and_then(|i| prune(*i, path, drop, gone));
            path.pop();
            inner.map(|i| At::Flow {
                workflow,
                step,
                inner: Some(Box::new(i)),
            })
        }
        At::Side {
            wait,
            fail,
            mut entries,
            breaking,
        } => {
            for (j, e) in entries.iter_mut().enumerate() {
                if let Some(at) = e.at.take() {
                    path.push(j);
                    e.at = prune(at, path, drop, gone);
                    path.pop();
                }
            }
            entries.iter().any(|e| e.at.is_some()).then_some(At::Side {
                wait,
                fail,
                entries,
                breaking,
            })
        }
    }
}

/// Whether the action at `path` under `node` is in a group that broke and
/// waits for its running steps to end.
fn breaking(node: &At, path: &[usize]) -> bool {
    let Some((&first, rest)) = path.split_first() else {
        return false;
    };
    match node {
        At::Action { .. } => false,
        At::Flow { step, inner, .. } => {
            first == *step && inner.as_deref().is_some_and(|i| breaking(i, rest))
        }
        At::Side {
            entries,
            breaking: b,
            ..
        } => {
            b.is_some()
                || entries
                    .get(first)
                    .and_then(|e| e.at.as_ref())
                    .is_some_and(|a| breaking(a, rest))
        }
    }
}

/// The action at `path` under `node`.
fn find<'n>(node: &'n mut At, path: &[usize]) -> Option<&'n mut At> {
    if matches!(node, At::Action { .. }) {
        return path.is_empty().then_some(node);
    }
    let (&first, rest) = path.split_first()?;
    match node {
        At::Flow { step, inner, .. } => {
            if first != *step {
                return None;
            }
            find(inner.as_mut()?, rest)
        }
        At::Side { entries, .. } => find(entries.get_mut(first)?.at.as_mut()?, rest),
        At::Action { .. } => None,
    }
}

/// The `$output` of a list side by side: the objects its entries printed,
/// merged; a field more than one printed kept by entry name, and a value
/// that is no object under its entry's name. One that printed nothing adds
/// nothing.
fn side_output(entries: &[Entry]) -> Value {
    let mut by: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut out = Map::new();
    for e in entries {
        match &e.output {
            Some(Value::Object(o)) => {
                for (k, v) in o {
                    by.entry(k.clone())
                        .or_default()
                        .insert(e.name.clone(), v.clone());
                }
            }
            None | Some(Value::Null) => {}
            Some(Value::String(t)) if t.is_empty() => {}
            Some(v) => {
                out.insert(e.name.clone(), v.clone());
            }
        }
    }
    for (k, values) in by {
        let v = match values.len() {
            1 => values
                .into_iter()
                .next()
                .map(|(_, v)| v)
                .unwrap_or_default(),
            _ => Value::Object(values),
        };
        out.insert(k, v);
    }
    Value::Object(out)
}

/// Whether a message is a waiting step's: it shares a field with what the
/// step printed, and agrees with it on every field they share.
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

fn output(d: &Value) -> Value {
    d.get("output")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
}

fn stamp(t: DateTime<FixedOffset>) -> String {
    t.with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
