//! The manager over steps: one decision for one
//! run and one event, JSON in, JSON out. Pure: no clock, no files, no
//! environment; the daemon gives it everything it needs and saves what it
//! answers.
//!
//! A run's position is a tree: the run's own workflow at the root, and a
//! node per entry of a list run side by side, down to the actions running
//! and the workflows used as steps, each a run of its own. Each job (and
//! each run a step starts) carries the path to its node, starting with the
//! phase, so a late answer from a branch already left behind is recognised
//! and ignored.
use super::{Event, Timer};
use crate::flow::{self, Fail, Flow, Name, Need, On, Repeat, Step, Wait};
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
    /// A repeated workflow's run: which round it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<Round>,
    /// Messages already taken, by id: one given twice counts once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taken: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
    /// The undos of the steps that finished, oldest first: run newest
    /// first when a step breaks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo: Vec<Undo>,
    /// Why the run broke, at which step, and the undos that broke: the
    /// error list's first `$output`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undo_failed: Vec<String>,
}

/// What takes a finished step back, as written, with its args, and that
/// step's result: the undo's `$output` when it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "UndoForm")]
pub struct Undo {
    pub step: String,
    #[serde(default, skip_serializing_if = "is_empty_object")]
    pub args: Value,
    #[serde(default)]
    pub output: Value,
}

/// An undo as a journal holds it: a name alone, as runs before `$output`
/// kept it, or whole.
#[derive(Deserialize)]
#[serde(untagged)]
enum UndoForm {
    Name(String),
    Full {
        step: String,
        #[serde(default)]
        args: Value,
        #[serde(default)]
        output: Value,
    },
}

impl From<UndoForm> for Undo {
    fn from(f: UndoForm) -> Undo {
        match f {
            UndoForm::Name(step) => Undo {
                step,
                args: Value::Null,
                output: Value::Null,
            },
            UndoForm::Full { step, args, output } => Undo { step, args, output },
        }
    }
}

fn is_empty_object(v: &Value) -> bool {
    v.as_object().is_some_and(Map::is_empty) || v.is_null()
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
    Error { why: String },
}

/// A round of a repeat: its number, from 1, and how many there are when
/// `repeat` read them from a value (`"$output.count"`) as the first began.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<u64>,
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
    /// A repeated step: its round, and the step's node in this round; none
    /// while a cron repeat waits for its next time (`next`).
    Round {
        #[serde(flatten)]
        round: Round,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner: Option<Box<At>>,
    },
    /// A workflow used as a step: a run of its own, started with these
    /// args as its `$event`; the step waits until that run ends.
    Child {
        workflow: String,
        #[serde(default, skip_serializing_if = "Map::is_empty")]
        data: Map<String, Value>,
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
        /// What it printed when it waited: what a message is matched on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
        /// The message taken for it, its `$event` when it runs again.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<Value>,
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
    /// The run's workflow is on and within its `from` and `until`: a
    /// repeat goes on only while it is.
    #[serde(default = "yes")]
    pub active: bool,
    /// The `$$` names that are defined.
    #[serde(default)]
    pub defined: Vec<String>,
    /// The manifest, `$config`: read as it is now, never kept in the run.
    #[serde(default)]
    pub config: Map<String, Value>,
    /// The run is a workflow used as a step: another run's step started it.
    #[serde(default)]
    pub child: bool,
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
    /// Runs to start now: the workflows used as steps reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<StepChild>,
    /// A repeated workflow's next round: a new run, as seeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_round: Option<Runtime>,
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
    /// be ended, and their answers are ignored from now on. A workflow used
    /// as a step is never among them: its run is its own and goes on to its
    /// end, and what it returns reaches nothing.
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
    /// Since when it has been waiting, when it runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
    /// The message that came for it, when it runs again after waiting: its
    /// `$event`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
}

/// A workflow used as a step, to start as a run of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepChild {
    /// Where its end goes back to, as a job's.
    pub path: Vec<usize>,
    pub workflow: String,
    /// The step's args: its `$event`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub event: Map<String, Value>,
    /// Its first state beside `$event`: a repeated step's `$count`, and,
    /// after the first round, the round before's `$output` and `$steps`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub seed: Map<String, Value>,
}

fn yes() -> bool {
    true
}

/// What a run that ended returns, to the step that started it: its
/// `$output` (its end list's result, or else its last step's, or why it
/// broke), its `$steps`, and how it ended.
pub fn returned(rt: &Runtime) -> Value {
    json!({
        "output": rt.state.get("output").cloned().unwrap_or(Value::Null),
        "steps": rt.state.get("steps").cloned().unwrap_or_else(|| json!({})),
        "status": status(rt),
    })
}

/// A run's first state: the event that started it (`$event`) and the
/// manifest (`$config`).
pub fn first_state(event: Value, config: Map<String, Value>) -> Map<String, Value> {
    let mut s = Map::new();
    s.insert("event".into(), event);
    s.insert("config".into(), Value::Object(config));
    s
}

/// Where a run is, or how it ended, in a word: `running`, `waiting`,
/// `done`, `failed` or `error`.
pub fn status(rt: &Runtime) -> &'static str {
    match &rt.ended {
        Some(Ended::Done) => "done",
        Some(Ended::Failed) => "failed",
        Some(Ended::Error { .. }) => "error",
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
        children: vec![],
        kept: None,
        seed: Map::new(),
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
    // a repeated workflow's run that ended well: its next round
    let next_round = match (&input.runtime.ended, &m.rt.ended) {
        (None, Some(Ended::Done)) => m.next_round(),
        _ => None,
    };
    let timers = m.timers();
    Ok(StepAnswer {
        runtime: m.rt,
        jobs: m.jobs,
        children: m.children,
        next_round,
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
    children: Vec<StepChild>,
    /// What a workflow used as a step returned, kept by its name in place
    /// of its `$output` once its step is recorded.
    kept: Option<Value>,
    /// The first state of the next workflow used as a step, beside its
    /// `$event`: a repeated step's round.
    seed: Map<String, Value>,
    /// A step that could not start, and why: the run breaks there once the
    /// event is handled.
    broke: Option<(String, String)>,
    log: Vec<String>,
    notes: Vec<Value>,
    stopped: Vec<Vec<usize>>,
}

impl Machine<'_> {
    /// The state as a step reads it, with `$config`.
    fn view(&self) -> Map<String, Value> {
        let mut v = self.rt.state.clone();
        v.insert("config".into(), Value::Object(self.i.config.clone()));
        v
    }

    /// The step just done is kept by `key`, `$steps.<key>`: its `$output`,
    /// or, for a workflow used as a step, all it returned.
    fn record(&mut self, key: &str) {
        let out = self
            .kept
            .take()
            .unwrap_or_else(|| self.rt.state.get("output").cloned().unwrap_or(Value::Null));
        // the first of a name used more than once is read by the name
        // alone too: `$steps.ayna` is `$steps.ayna/1`
        if let Some(name) = key.strip_suffix("/1") {
            self.record_as(name, out.clone());
        }
        self.record_as(key, out);
    }

    fn record_as(&mut self, key: &str, out: Value) {
        let steps = self
            .rt
            .state
            .entry("steps")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(m) = steps {
            m.insert(key.into(), out);
        }
    }

    /// The error list starts with why the run ended as its `$output`.
    fn ended_output(&mut self) {
        self.rt.state.insert(
            "output".into(),
            json!({
                "why": self.rt.why,
                "failed_step": self.rt.failed_step,
                "undo_failed": self.rt.undo_failed,
            }),
        );
    }

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
                // its `$event`: the message's data, beside what it was
                // seeded with (a round's, a repeated step's)
                self.rt.state.insert("event".into(), d.clone());
                self.rt.phase = Phase::Steps;
                if let Err(why) = self.first_round() {
                    self.broke = Some((why, self.i.workflow.clone()));
                    return Ok(());
                }
                self.begin()
            }
            "run.started" => {
                self.log
                    .push("run.started ignored: the run has begun".into());
                Ok(())
            }
            "action.finished" => self.finished(&path(d)?, &output(d)),
            "timer.fired" if d.get("kind").and_then(Value::as_str) == Some("round") => {
                self.round_due()
            }
            "workflow.ended" => self.child_ended(&path(d)?, d),
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
        self.rt.at = self.start_list(&root, 0, &mut p)?;
        if self.rt.at.is_none() && self.broke.is_none() {
            self.list_done()?;
        }
        Ok(())
    }

    /// The current phase's list is done: on to the next, or the end.
    fn list_done(&mut self) -> Result<(), String> {
        match self.rt.phase {
            Phase::Steps => {
                let failed =
                    flow::lookup(&self.rt.state, "output.success") == Some(&Value::Bool(false));
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
                self.ended_output();
                self.begin()
            }
        }
    }

    fn end(&mut self, e: Ended) {
        self.rt.at = None;
        self.rt.ended = Some(e);
    }

    fn why(&self) -> String {
        self.rt.why.clone().unwrap_or_else(|| "it failed".into())
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
        self.rt.why = Some(why.into());
        self.rt.failed_step = Some(step.into());
        self.log.push(format!("{step}: {why}"));
        if self.rt.phase == Phase::Steps {
            self.rt.phase = if self.rt.undo.is_empty() {
                self.ended_output();
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
        self.rt.undo_failed.push(format!("{step}: {why}"));
        self.log.push(format!("undo {step}: {why}"));
        self.finished(path, &Value::String(String::new()))
    }

    fn list(&self, workflow: &str) -> Result<Vec<Step>, String> {
        let f = &self.flows[workflow];
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
                    let name = flow::name(&u.step)?;
                    Ok(Step::One {
                        label: name.label(),
                        name,
                        args: if u.args.is_null() {
                            Value::Object(Map::new())
                        } else {
                            u.args.clone()
                        },
                        on: None,
                        undo: None,
                        undo_args: Value::Object(Map::new()),
                        repeat: None,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }

    fn holds(&self, on: &On) -> bool {
        let defined = |n: &str| self.i.defined.iter().any(|d| d == n);
        flow::holds(on, &self.view(), &defined)
    }

    /// Starts `workflow`'s list from step `from`, skipping the steps whose
    /// `on` does not hold: its node, or `None` when nothing is left to run.
    fn start_list(
        &mut self,
        workflow: &str,
        from: usize,
        path: &mut Vec<usize>,
    ) -> Result<Option<At>, String> {
        let steps = self.list(workflow)?;
        for (idx, step) in steps.iter().enumerate().skip(from) {
            if self.rt.phase == Phase::Undo {
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
                repeat: Some(r),
                label,
                ..
            } => {
                // its first round: how many there are, when a value says
                let of = match r {
                    Repeat::TimesOf(p) => match self.count_of(p) {
                        Ok(0) => {
                            self.skipped(label);
                            return Ok(None);
                        }
                        Ok(n) => Some(n),
                        Err(why) => {
                            self.broke = Some((why, label.clone()));
                            return Ok(None);
                        }
                    },
                    _ => None,
                };
                self.round(step, Round { count: 1, of }, path)
            }
            Step::One {
                name,
                args,
                on,
                label,
                ..
            } => {
                if let Some(on) = on
                    && !self.holds(on)
                {
                    self.skipped(label);
                    return Ok(None);
                }
                let data = match self.args(args) {
                    Ok(data) => data,
                    Err(why) => {
                        // it cannot start: the run breaks here
                        self.broke = Some((why, label.clone()));
                        return Ok(None);
                    }
                };
                self.start_name(name, &data, path)
            }
            Step::Side(group) => {
                let mut entries: Vec<Entry> = vec![];
                for (j, m) in group.members.iter().enumerate() {
                    path.push(j);
                    let at = if m.on.as_ref().is_some_and(|on| !self.holds(on)) {
                        self.skipped(&m.label);
                        None
                    } else {
                        match self.args(&m.args) {
                            Ok(data) => self.start_name(&m.name, &data, path)?,
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
                        Adv::Done | Adv::RoundDone { .. } => None,
                        Adv::Broke { step, why } => {
                            self.broke = Some((why, step));
                            None
                        }
                    },
                )
            }
        }
    }

    /// A round of a repeated step: `$count` set, and, for a workflow used as
    /// a step, its run seeded with the round before's results; the step's
    /// node in it.
    fn round(
        &mut self,
        step: &Step,
        round: Round,
        path: &mut Vec<usize>,
    ) -> Result<Option<At>, String> {
        let Step::One { label, name, .. } = step else {
            return Ok(None);
        };
        self.rt.state.insert("count".into(), round.count.into());
        if matches!(name, Name::Workflow(_)) {
            let mut seed = Map::new();
            seed.insert("count".into(), round.count.into());
            if round.count > 1 {
                let back =
                    flow::lookup(&self.rt.state, &format!("steps.{}", flow::step_key(label)));
                seed.insert(
                    "output".into(),
                    self.rt.state.get("output").cloned().unwrap_or_default(),
                );
                seed.insert(
                    "steps".into(),
                    back.and_then(|b| b.get("steps"))
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                );
            }
            self.seed = seed;
        }
        let mut once = step.clone();
        if let Step::One { repeat, .. } = &mut once {
            *repeat = None;
        }
        let node = self.start_step(&once, path)?;
        self.seed = Map::new();
        Ok(match node {
            Some(n) => Some(At::Round {
                round,
                next: None,
                inner: Some(Box::new(n)),
            }),
            None => {
                self.rt.state.remove("count");
                None
            }
        })
    }

    /// A value read as a number of rounds (`repeat: "$output.count"`).
    fn count_of(&self, p: &str) -> Result<u64, String> {
        match flow::lookup(&self.view(), p) {
            Some(v) if v.as_u64().is_some() => Ok(v.as_u64().unwrap_or_default()),
            v => Err(format!(
                "repeat: ${p} is {}, not a number of rounds",
                v.map_or("not there".to_string(), Value::to_string)
            )),
        }
    }

    /// Whether a repeat goes on after round `round` ended well.
    fn goes_on(&self, r: &Repeat, round: Round) -> bool {
        let now = self.now.with_timezone(&Utc);
        self.i.active
            && match r {
                Repeat::Always | Repeat::Cron(_) => true,
                Repeat::Times(n) => round.count < *n,
                Repeat::TimesOf(_) => round.of.is_some_and(|n| round.count < n),
                Repeat::Until(t) => now < *t,
                Repeat::After(t) => now >= *t,
            }
    }

    /// A repeated workflow's run, as it starts: its round (the first unless
    /// it was seeded with one), `$count` set.
    fn first_round(&mut self) -> Result<(), String> {
        let Some(r) = self.flows[&self.i.workflow].repeat.clone() else {
            return Ok(());
        };
        let round = match self.rt.round {
            Some(round) => round,
            None => Round {
                count: 1,
                of: match &r {
                    Repeat::TimesOf(p) => Some(self.count_of(p)?),
                    _ => None,
                },
            },
        };
        self.rt.round = Some(round);
        self.rt.state.insert("count".into(), round.count.into());
        Ok(())
    }

    /// A repeated workflow's run that ended well: the next round, a new
    /// run seeded with this one's `$event`, `$output` and `$steps`, unless
    /// the repeat is over.
    fn next_round(&self) -> Option<Runtime> {
        let r = self.flows[&self.i.workflow].repeat.as_ref()?;
        let round = self.rt.round?;
        if !self.goes_on(r, round) {
            return None;
        }
        let mut state = Map::new();
        for key in ["event", "output", "steps"] {
            if let Some(v) = self.rt.state.get(key) {
                state.insert(key.into(), v.clone());
            }
        }
        state.insert("count".into(), (round.count + 1).into());
        Some(Runtime {
            state,
            round: Some(Round {
                count: round.count + 1,
                of: round.of,
            }),
            ..Runtime::default()
        })
    }

    /// The round of a repeated step that ended: the next one, now or at its
    /// cron line's next time, or the step is done.
    fn after_round(
        &mut self,
        workflow: &str,
        step: usize,
        prefix: Vec<usize>,
        round: Round,
        status: Option<Value>,
    ) -> Result<Adv, String> {
        let Some(s) = self.list(workflow)?.get(step).cloned() else {
            return self.step_done(workflow, step, prefix);
        };
        let Step::One {
            repeat: Some(r), ..
        } = &s
        else {
            return self.step_done(workflow, step, prefix);
        };
        let r = r.clone();
        // a round that failed (its result says "success": false, or a
        // workflow's ended failed or in error) stops the repeat
        let failed = flow::lookup(&self.rt.state, "output.success") == Some(&Value::Bool(false))
            || status.is_some_and(|st| st != "done");
        if failed || !self.goes_on(&r, round) {
            self.rt.state.remove("count");
            return self.step_done(workflow, step, prefix);
        }
        let next = Round {
            count: round.count + 1,
            of: round.of,
        };
        if let Repeat::Cron(line) = &r {
            let at = crate::engine::schedule::next(line, self.now.with_timezone(&Utc))?;
            return Ok(Adv::Going(At::Flow {
                workflow: workflow.into(),
                step,
                inner: Some(Box::new(At::Round {
                    round,
                    next: Some(stamp(at.fixed_offset())),
                    inner: None,
                })),
            }));
        }
        self.next_round_of(workflow, step, prefix, &s, next)
    }

    /// Starts round `round` of the repeated step `s`: the step's node, or
    /// the step done when it does not run (its `on` no longer holds).
    fn next_round_of(
        &mut self,
        workflow: &str,
        step: usize,
        prefix: Vec<usize>,
        s: &Step,
        round: Round,
    ) -> Result<Adv, String> {
        let mut p = prefix.clone();
        p.push(step);
        let node = self.round(s, round, &mut p)?;
        if let Some((why, step)) = self.broke.take() {
            return Ok(Adv::Broke { step, why });
        }
        match node {
            Some(n) => Ok(Adv::Going(At::Flow {
                workflow: workflow.into(),
                step,
                inner: Some(Box::new(n)),
            })),
            None => self.step_done(workflow, step, prefix),
        }
    }

    /// A timer: the next round of a cron repeat is due.
    fn round_due(&mut self) -> Result<(), String> {
        let Some(At::Flow {
            workflow,
            step,
            inner: Some(i),
        }) = self.rt.at.clone()
        else {
            return Ok(());
        };
        let At::Round {
            round,
            next: Some(next),
            inner: None,
        } = *i
        else {
            self.log
                .push("a round timer ignored: no round waits".into());
            return Ok(());
        };
        let due = DateTime::parse_from_rfc3339(&next).map_err(|e| e.to_string())?;
        if due > self.now {
            return Ok(());
        }
        let Some(s) = self.list(&workflow)?.get(step).cloned() else {
            return Ok(());
        };
        let prefix = vec![self.rt.phase.index()];
        let next = Round {
            count: round.count + 1,
            of: round.of,
        };
        match self.next_round_of(&workflow, step, prefix, &s, next)? {
            Adv::Going(at) => {
                self.rt.at = Some(at);
                Ok(())
            }
            Adv::Done | Adv::RoundDone { .. } => self.list_done(),
            Adv::Broke { step, why } => self.fail(&why, &step),
        }
    }

    fn start_name(
        &mut self,
        name: &Name,
        data: &Map<String, Value>,
        path: &[usize],
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
                    event: None,
                };
                self.jobs.extend(job(&mut leaf, path));
                Ok(Some(leaf))
            }
            Name::Workflow(w) => {
                let Some(f) = self.flows.get(w) else {
                    return Err(format!("no workflow {w}"));
                };
                // its `on` holds over its `$event`, the args it is given
                let first = first_state(Value::Object(data.clone()), self.i.config.clone());
                let defined = |n: &str| self.i.defined.iter().any(|d| d == n);
                if !flow::holds(&f.on, &first, &defined) {
                    self.skipped(w);
                    return Ok(None);
                }
                self.children.push(StepChild {
                    path: path.to_vec(),
                    workflow: w.clone(),
                    event: data.clone(),
                    seed: std::mem::take(&mut self.seed),
                });
                Ok(Some(At::Child {
                    workflow: w.clone(),
                    data: data.clone(),
                }))
            }
        }
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

    /// A step's args filled from the state, an object: its action's, and
    /// nothing else's.
    fn args(&mut self, args: &Value) -> Result<Map<String, Value>, String> {
        flow::fill_args(args, &self.view())
    }

    /// The run a step started ended: the step is done, its `$output` what
    /// the run returned. A run that broke or failed is no error for this
    /// one: it reads what came back and goes on.
    fn child_ended(&mut self, path: &[usize], d: &Value) -> Result<(), String> {
        let here = path.split_first().is_some_and(|(&phase, rest)| {
            phase == self.rt.phase.index()
                && self
                    .rt
                    .at
                    .as_mut()
                    .and_then(|at| find(at, rest))
                    .is_some_and(|leaf| matches!(leaf, At::Child { .. }))
        });
        if !here {
            self.log.push(format!(
                "the end of a run for {path:?} ignored: not a step now"
            ));
            return Ok(());
        }
        let out = d.get("output").cloned().unwrap_or(Value::Null);
        self.kept = Some(json!({
            "output": out,
            "steps": d.get("steps").cloned().unwrap_or_else(|| json!({})),
            "status": d.get("status").cloned().unwrap_or(Value::Null),
        }));
        let r = self.settle_leaf(path, Leaf::Done(out));
        self.kept = None;
        r
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
        match self.advance(at, &path[1..], vec![path[0]], &leaf)? {
            Adv::Going(at) => {
                self.rt.at = Some(at);
                Ok(())
            }
            Adv::Done | Adv::RoundDone { .. } => self.list_done(),
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
        leaf: &Leaf,
    ) -> Result<Adv, String> {
        match node {
            At::Action { .. } | At::Child { .. } => Ok(match leaf {
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
            } => self.advance_flow(workflow, step, inner, path, prefix, leaf),
            // a round of a repeated step: the step's node under it
            At::Round {
                round,
                next,
                inner: Some(i),
            } => Ok(match self.advance(*i, path, prefix, leaf)? {
                Adv::Going(n) => Adv::Going(At::Round {
                    round,
                    next,
                    inner: Some(Box::new(n)),
                }),
                Adv::Done => Adv::RoundDone { round },
                other => other,
            }),
            waiting @ At::Round { inner: None, .. } => Ok(Adv::Going(waiting)),
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
                    match self.advance(at, &path[1..], p, leaf)? {
                        Adv::Going(at) => entries[j].at = Some(at),
                        Adv::Done => {
                            entries[j].outcome = Some(Outcome::Done);
                            entries[j].output = self.rt.state.get("output").cloned();
                            entries[j].order = next_order(&entries);
                            self.record(&flow::step_key(&entries[j].name));
                        }
                        Adv::Broke { step, why } => {
                            entries[j].outcome = Some(Outcome::Broke { step, why });
                            entries[j].order = next_order(&entries);
                        }
                        // a step side by side is never repeated
                        Adv::RoundDone { .. } => unreachable!("a repeated step side by side"),
                    }
                }
                Ok(self.settle(wait, fail, entries, &prefix))
            }
        }
    }

    /// A workflow's node at step `step` after the leaf under it ended: its
    /// step goes on, or it finished (kept by its name, `$steps.<name>`, and
    /// the next step starts), or it broke.
    fn advance_flow(
        &mut self,
        workflow: String,
        step: usize,
        inner: Option<Box<At>>,
        path: &[usize],
        prefix: Vec<usize>,
        leaf: &Leaf,
    ) -> Result<Adv, String> {
        let mut p = prefix.clone();
        p.push(step);
        let next = match inner {
            Some(i) => self.advance(*i, &path[1..], p, leaf)?,
            None => Adv::Done,
        };
        match next {
            Adv::Going(n) => Ok(Adv::Going(At::Flow {
                workflow,
                step,
                inner: Some(Box::new(n)),
            })),
            broke @ Adv::Broke { .. } => Ok(broke),
            Adv::RoundDone { round } => {
                // each round's result is kept by the step's name
                let status = self.kept.as_ref().and_then(|k| k.get("status").cloned());
                if let Some(Step::One { label, .. }) = self.list(&workflow)?.get(step).cloned() {
                    self.record(&flow::step_key(&label));
                }
                self.after_round(&workflow, step, prefix, round, status)
            }
            Adv::Done => {
                if self.rt.phase != Phase::Undo
                    && let Some(Step::One { label, .. }) = self.list(&workflow)?.get(step).cloned()
                {
                    self.record(&flow::step_key(&label));
                }
                self.step_done(&workflow, step, prefix)
            }
        }
    }

    /// The step at `step` finished (and was kept by its name): what takes
    /// it back, if the run breaks later, and on to the next step.
    fn step_done(
        &mut self,
        workflow: &str,
        step: usize,
        prefix: Vec<usize>,
    ) -> Result<Adv, String> {
        {
            {
                if self.rt.phase == Phase::Steps
                    && let Some(Step::One {
                        undo: Some(u),
                        undo_args,
                        ..
                    }) = self.flows[workflow].steps.get(step)
                {
                    self.rt.undo.push(Undo {
                        step: written(u),
                        args: undo_args.clone(),
                        output: self.rt.state.get("output").cloned().unwrap_or_default(),
                    });
                }
                let mut p = prefix;
                let next = self.start_list(workflow, step + 1, &mut p)?;
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
            let given_now: Vec<Vec<usize>> = self
                .jobs
                .iter()
                .map(|job| job.path.clone())
                .chain(self.children.iter().map(|c| c.path.clone()))
                .collect();
            // an action running, or a workflow used as a step (a run of its
            // own), started before this decision
            let runs = |leaf: &At, at: &[usize]| {
                matches!(
                    leaf,
                    At::Action {
                        started: true,
                        waiting: false,
                        ..
                    } | At::Child { .. }
                ) && !given_now.iter().any(|g| g == at)
            };
            e.at = prune(node, &mut p, &mut |leaf, at| !runs(leaf, at), &mut gone);
            self.jobs.retain(|job| !gone.contains(&job.path));
            // a workflow used as a step not yet started never starts
            let unstarted: Vec<Vec<usize>> = self
                .children
                .iter()
                .filter(|c| gone.contains(&c.path))
                .map(|c| c.path.clone())
                .collect();
            self.children.retain(|c| !unstarted.contains(&c.path));
            self.stopped
                .extend(gone.into_iter().filter(|g| !unstarted.contains(g)));
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
            let (mut gone, mut runs) = (vec![], vec![]);
            let mut p = at.to_vec();
            p.push(j);
            leaves(&mut node, &mut p, &mut |leaf, path| {
                if matches!(leaf, At::Child { .. }) {
                    runs.push(path.to_vec());
                } else {
                    gone.push(path.to_vec());
                }
            });
            self.jobs.retain(|job| !gone.contains(&job.path));
            // a workflow used as a step is a run of its own: one running goes
            // on to its end, its answer reaching nothing; one not yet started
            // never starts
            self.children.retain(|c| !runs.contains(&c.path));
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
        // printing nothing when it runs again keeps what it is matched on
        let nothing = matches!(out, Value::Null)
            || out.as_str() == Some("")
            || out.as_object().is_some_and(Map::is_empty);
        if !nothing || output.is_none() {
            *output = Some(out.clone());
        }
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
        let mut jobs = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if matches!(leaf, At::Action { waiting: true, .. }) {
                    jobs.extend(job(leaf, p));
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
        let mut jobs = vec![];
        if let Some(at) = self.rt.at.as_mut() {
            leaves(at, &mut vec![self.rt.phase.index()], &mut |leaf, p| {
                if let At::Action {
                    waiting: true,
                    output,
                    event,
                    ..
                } = leaf
                    && output
                        .as_ref()
                        .and_then(Value::as_object)
                        .is_some_and(|o| ours(o, &data))
                {
                    *event = Some(Value::Object(data.clone()));
                    jobs.extend(job(leaf, p));
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
            // a step whose time is up gives up, though the retry came first
            // (a timeout of whole hours brings both at once); the rest run
            // again
            Some("retry") => {
                self.give_up_late()?;
                self.wake();
                Ok(())
            }
            Some("deadline") => self.give_up_late(),
            _ => {
                self.log.push("a timer of no kind ignored".into());
                Ok(())
            }
        }
    }

    /// The waiting steps that have waited their `timeout` break.
    fn give_up_late(&mut self) -> Result<(), String> {
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
            // one that a group stopped, or a run that went on, is not a step
            // now
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

    fn timeout(&self) -> chrono::Duration {
        let t = self.flows[&self.i.workflow].timeout.unwrap_or(flow::WAIT);
        chrono::Duration::from_std(t).unwrap_or(chrono::Duration::days(1))
    }

    /// The run's timers: a cron repeat's next round, when one waits; and,
    /// while a step waits for data, the hourly retry and the deadline of
    /// the longest waiting.
    fn timers(&mut self) -> Vec<Timer> {
        if self.rt.ended.is_some() {
            return vec![];
        }
        if let Some(At::Flow { inner: Some(i), .. }) = &self.rt.at
            && let At::Round {
                next: Some(next),
                inner: None,
                ..
            } = i.as_ref()
        {
            return vec![Timer {
                at: next.clone(),
                data: json!({"kind": "round"}),
            }];
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

/// A node of the position after a leaf ended. It is moved once per step,
/// returned up the tree: boxing it would cost an allocation each time for
/// no gain.
#[allow(clippy::large_enum_variant)]
enum Adv {
    Going(At),
    Done,
    /// A round of a repeated step ended well.
    RoundDone {
        round: Round,
    },
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
        fields.insert("winner".into(), flow::step_key(&w.name).into());
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
                failed.push(Value::String(flow::step_key(&e.name)));
                why.insert(flow::step_key(&e.name), w.clone().into());
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
fn job(leaf: &mut At, path: &[usize]) -> Option<StepJob> {
    let At::Action {
        name,
        version,
        data,
        started,
        waiting,
        waiting_since,
        event,
        ..
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
        waiting_since: waiting_since.clone(),
        event: event.clone(),
    })
}

/// Calls `f` on every action and workflow used as a step under `node`,
/// with its path.
fn leaves(node: &mut At, path: &mut Vec<usize>, f: &mut dyn FnMut(&mut At, &[usize])) {
    match node {
        At::Action { .. } | At::Child { .. } => f(node, path),
        At::Round { inner, .. } => {
            if let Some(i) = inner {
                leaves(i, path, f);
            }
        }
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

/// `node` without the actions (and workflows used as steps) `drop` picks, given each with its path (from
/// `path`, the node's own): theirs go to `gone`. `None` once no action is
/// left under it.
fn prune(
    node: At,
    path: &mut Vec<usize>,
    drop: &mut dyn FnMut(&At, &[usize]) -> bool,
    gone: &mut Vec<Vec<usize>>,
) -> Option<At> {
    match node {
        At::Action { .. } | At::Child { .. } => {
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
        At::Round { round, next, inner } => {
            inner
                .and_then(|i| prune(*i, path, drop, gone))
                .map(|i| At::Round {
                    round,
                    next,
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
        At::Action { .. } | At::Child { .. } => false,
        At::Round { inner, .. } => inner.as_deref().is_some_and(|i| breaking(i, path)),
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

/// The action (or workflow used as a step) at `path` under `node`.
fn find<'n>(node: &'n mut At, path: &[usize]) -> Option<&'n mut At> {
    if matches!(node, At::Action { .. } | At::Child { .. }) {
        return path.is_empty().then_some(node);
    }
    // a round of a repeated step takes no place in a path
    if let At::Round { inner, .. } = node {
        return find(inner.as_mut()?, path);
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
        At::Action { .. } | At::Child { .. } | At::Round { .. } => None,
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
                        .insert(flow::step_key(&e.name), v.clone());
                }
            }
            None | Some(Value::Null) => {}
            Some(Value::String(t)) if t.is_empty() => {}
            Some(v) => {
                out.insert(flow::step_key(&e.name), v.clone());
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
