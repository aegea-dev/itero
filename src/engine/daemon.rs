//! The daemon's work for one workspace, one pass at a time: start the runs
//! schedules and messages call for, fire due timers, deliver every waiting
//! event to its run's manager, save each answer, and run the jobs it asks
//! for in workers. The daemon decides nothing about a run: every change to
//! one comes from a manager's answer.
use super::Event;
use super::journal::{self, Envelope, Handled, Journal, Recorded, Split, Start};
use super::steps::{self, StepAnswer, StepInput, StepJob};
use super::worker::{Failure, LOST_EXIT, NOTIFY, Outcome, StepRun};
use crate::flow::{self, Flow, Name};
use crate::workspace::Workspace;
use anyhow::{Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use wait_timeout::ChildExt;

/// What one pass did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pass {
    /// Events answered.
    pub delivered: usize,
    /// Jobs run in workers, their results saved.
    pub worked: usize,
    /// Jobs lost (killed, or past their time), to run again.
    pub lost: usize,
    /// Events left for the next pass: their manager kept being killed.
    pub deferred: usize,
}

/// More events than this in one pass wait for the next.
const MAX_EVENTS: usize = 10_000;

/// How many times a manager killed before it answered is asked again,
/// before the event waits for the next pass.
const MAX_TRIES: usize = 3;

/// How many times a job runs, the first run and those after it was lost,
/// before its step is an error.
const MAX_RUNS: i64 = 3;

/// Seconds a step may take: past that it is stopped and tried again.
const STEP_TIMEOUT: u64 = 600;

/// What became of an event given to `deliver`.
enum Delivered {
    /// Answered (or given up on with an error) and saved.
    Handled,
    /// Left unhandled: its manager kept being killed.
    Deferred,
}

pub fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// One life of the daemon: from its start to its end.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Daemon {
    pub id: String,
    /// Two random words.
    pub name: String,
    pub started: String,
    pub pid: u32,
    pub host: String,
    pub itero: String,
}

impl Daemon {
    pub fn starting(at: &str) -> Daemon {
        Daemon {
            id: uuid::Uuid::now_v7().to_string(),
            name: crate::names::random(),
            started: at.into(),
            pid: std::process::id(),
            host: host(),
            itero: journal::ITERO.into(),
        }
    }
}

/// This machine's name.
fn host() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer and its length match; the name ends at its NUL
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    if ok {
        String::from_utf8_lossy(&buf[..end]).into_owned()
    } else {
        String::new()
    }
}

/// How the daemon's loop is doing, for `/health`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Health {
    pub daemon: Daemon,
    pub started: String,
    /// Passes so far.
    pub passes: u64,
    /// When the last pass ran.
    pub last_pass: Option<String>,
    /// What went wrong in the last pass, if anything.
    pub last_error: Option<String>,
    /// How each source is doing, by the type it sends.
    pub sources: BTreeMap<String, SourceHealth>,
}

/// How one source is doing.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SourceHealth {
    /// `running`, `waiting` (to start again) or `gave up`.
    pub state: String,
    /// Starts after the first, within the supervisor's window.
    pub restarts: usize,
    /// How it ended last.
    pub last_exit: Option<String>,
}

impl Health {
    pub fn starting(at: &str) -> Health {
        Health {
            daemon: Daemon::starting(at),
            started: at.to_string(),
            passes: 0,
            last_pass: None,
            last_error: None,
            sources: Default::default(),
        }
    }
}

/// Runs passes over the workspace until `wake`'s sender is gone: one every
/// `interval`, or at once when something arrives on `wake` (a message from
/// outside). Each pass is recorded in `health`, and `report` hears of it; a
/// pass that fails never stops the next.
pub fn run_loop(
    ws: &Workspace,
    interval: std::time::Duration,
    exe: &Path,
    wake: &Receiver<()>,
    health: &Mutex<Health>,
    report: impl Fn(&Result<Pass>),
) {
    loop {
        let now = Utc::now();
        let pass = run_once(ws, now, exe);
        {
            let mut h = health.lock().unwrap_or_else(PoisonError::into_inner);
            h.passes += 1;
            h.last_pass = Some(stamp(now));
            h.last_error = pass.as_ref().err().map(|e| format!("{e:#}"));
        }
        report(&pass);
        match wake.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // several wake-ups while a pass ran need only one more pass
        while wake.try_recv().is_ok() {}
    }
}

/// The workspace's workflows as their files say, and read.
struct Flows {
    values: BTreeMap<String, Value>,
    read: BTreeMap<String, Flow>,
}

impl Flows {
    fn load(ws: &Workspace) -> Result<Flows> {
        let values = ws.flow_values()?;
        let mut read = BTreeMap::new();
        for (n, v) in &values {
            read.insert(
                n.clone(),
                flow::from_value(v).map_err(|e| anyhow!("{n}: {e}"))?,
            );
        }
        Ok(Flows { values, read })
    }

    /// `root` and every workflow it uses as a step, there or deeper, as
    /// their files say: what a run of it is pinned to.
    fn pinned(&self, root: &str) -> Result<BTreeMap<String, Value>, String> {
        let mut out = BTreeMap::new();
        let mut todo = vec![root.to_string()];
        while let Some(name) = todo.pop() {
            if out.contains_key(&name) {
                continue;
            }
            let (Some(v), Some(f)) = (self.values.get(&name), self.read.get(&name)) else {
                return Err(format!("{root} uses workflow {name}, which is not there"));
            };
            out.insert(name.clone(), v.clone());
            for n in f.uses() {
                if let Name::Workflow(w) = n {
                    todo.push(w);
                }
            }
        }
        Ok(out)
    }
}

/// Whether an environment variable or a secret is defined.
fn defined(name: &str) -> bool {
    super::secrets::get(name).is_some()
}

/// One pass over a workspace; `exe` is the itero binary, run as `itero
/// manager` and `itero worker`.
pub fn run_once(ws: &Workspace, now: DateTime<Utc>, exe: &Path) -> Result<Pass> {
    let mut j = Journal::open(ws)?;
    let mut pass = Pass::default();
    let flows = Flows::load(ws)?;
    j.see_workflows(flows.read.keys(), &stamp(now))?;
    fire_schedules(&flows, &mut j, now)?;
    for (id, run, data) in j.due_timers(&stamp(now))? {
        let e = Envelope::new(
            "daemon",
            "timer.fired",
            &journal::subject(&run),
            &stamp(now),
            None,
            data,
        );
        j.fire_timer(id, &e)?;
    }
    // runs' events first; then the waiting jobs, whose results are the
    // next events. Messages that would start a run while `runs` are under
    // way are held; they are taken again once a run has ended.
    let mut held = vec![];
    loop {
        while let Some(r) = j.next(&held)? {
            if r.event.subject == journal::subject("app") {
                if route(ws, &flows, &mut j, &r, now)? {
                    pass.delivered += 1;
                } else {
                    held.push(r.seq);
                }
                continue;
            }
            if let Delivered::Deferred = deliver(ws, &mut j, &r, now, exe)? {
                // events are delivered in order: the rest waits with it
                pass.deferred += 1;
                return Ok(pass);
            }
            pass.delivered += 1;
            if !held.is_empty() && j.active_count()? < ws.limits.runs {
                held.clear();
            }
            if pass.delivered >= MAX_EVENTS {
                return Ok(pass);
            }
        }
        let batch: Vec<Recorded> = j.jobs()?.into_iter().take(ws.limits.steps).collect();
        if batch.is_empty() {
            break;
        }
        // each run of a job is counted before it starts, so one the daemon
        // stopped during counts too
        let mut starting = vec![];
        for r in batch {
            let (runs, lost) = j.job_runs(r.seq)?;
            if runs >= MAX_RUNS {
                let why = lost.unwrap_or_default();
                save_result(&mut j, &r, Err(lost_too_often(&why)), now)?;
                pass.worked += 1;
            } else {
                j.start_job(r.seq)?;
                let step = step_run(ws, &j, &r.event.data);
                starting.push((r, runs + 1, step));
            }
        }
        super::die_at("job-started");
        // the workers run side by side; each result is saved on its own
        let outcomes: Vec<Result<Outcome, Failure>> = std::thread::scope(|s| {
            let running: Vec<_> = starting
                .iter()
                .map(|(_, _, step)| {
                    s.spawn(move || match step {
                        Ok(step) => spawn_worker(exe, step),
                        Err(why) => Err(Failure::Error(why.clone())),
                    })
                })
                .collect();
            running
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        Err(Failure::Error("the worker's runner panicked".into()))
                    })
                })
                .collect()
        });
        super::die_at("job-done");
        for ((r, run, _), outcome) in starting.iter().zip(outcomes) {
            match outcome {
                Err(Failure::Lost(why)) if *run < MAX_RUNS => {
                    j.lose_job(r.seq, &why)?;
                    pass.lost += 1;
                }
                Err(Failure::Lost(why)) => {
                    save_result(&mut j, r, Err(lost_too_often(&why)), now)?;
                    pass.worked += 1;
                }
                other => {
                    save_result(&mut j, r, other.map_err(|f| f.to_string()), now)?;
                    pass.worked += 1;
                }
            }
            super::die_at("result");
        }
    }
    let keep = chrono::Duration::from_std(ws.limits.keep).unwrap_or(chrono::Duration::MAX);
    j.prune(&stamp(now - keep))?;
    Ok(pass)
}

/// Starts a run of every workflow whose schedule is due, when its
/// conditions hold. A schedule seen for the first time only starts
/// counting: it waits for its first slot.
fn fire_schedules(flows: &Flows, j: &mut Journal, now: DateTime<Utc>) -> Result<()> {
    for (name, f) in &flows.read {
        for cron in &f.on.schedules {
            let key = format!("{name}#{cron}");
            let Some(last) = j.schedule_last(&key)? else {
                j.set_schedule_last(&key, &stamp(now))?;
                continue;
            };
            // off or outside its dates, its slots pass without a run
            if !flow::active(&f.meta, now) {
                j.set_schedule_last(&key, &stamp(now))?;
                continue;
            }
            let last: DateTime<Utc> = last
                .parse()
                .map_err(|_| anyhow!("schedule {key}: its last slot is not a time: {last}"))?;
            if !super::schedule::due(cron, last, now).map_err(|e| anyhow!(e))? {
                continue;
            }
            let pinned = flows.pinned(name);
            match pinned {
                Ok(pinned) if flow::holds(&f.on, &Map::new(), &defined) => {
                    let start = Start {
                        workflow: name.clone(),
                        flows: pinned,
                        input: json!({}),
                        cause: None,
                    };
                    j.start_run(&start, &stamp(now), Some(&key))?;
                }
                Ok(_) => {
                    eprintln!("{name}: schedule {cron}: its on does not hold");
                    j.set_schedule_last(&key, &stamp(now))?;
                }
                Err(why) => {
                    eprintln!("{name}: schedule {cron}: {why}");
                    j.set_schedule_last(&key, &stamp(now))?;
                }
            }
        }
    }
    Ok(())
}

/// A message for the workspace: a run of every active workflow whose `on`
/// lists its type and holds over its data, and a copy for every run
/// waiting for data. False, and nothing done, when it would start a run
/// while `runs` are under way: it waits its turn.
fn route(
    ws: &Workspace,
    flows: &Flows,
    j: &mut Journal,
    r: &Recorded,
    now: DateTime<Utc>,
) -> Result<bool> {
    let data = r.event.data.as_object().cloned().unwrap_or_default();
    let mut starts = vec![];
    for (name, f) in &flows.read {
        if !f.on.events.contains(&r.event.kind)
            || !flow::active(&f.meta, now)
            || !flow::holds(&f.on, &data, &defined)
        {
            continue;
        }
        match flows.pinned(name) {
            Ok(pinned) => starts.push(Start {
                workflow: name.clone(),
                flows: pinned,
                input: Value::Object(data.clone()),
                cause: Some(r.event.id.clone()),
            }),
            Err(why) => eprintln!("{}: {name}: {why}", r.event.kind),
        }
    }
    if !starts.is_empty() && j.active_count()? >= ws.limits.runs {
        return Ok(false);
    }
    let waiting: Vec<String> = j
        .active_runs()?
        .into_iter()
        .filter(|run| steps::is_waiting(&run.runtime))
        .map(|run| run.id)
        .collect();
    if starts.is_empty() && waiting.is_empty() {
        eprintln!(
            "{}: no workflow starts on it, and no run waits for it",
            r.event.kind
        );
    }
    j.route(r, &starts, &waiting, &stamp(now))?;
    Ok(true)
}

/// Why a job that kept being lost is an error.
fn lost_too_often(why: &str) -> String {
    format!("lost {MAX_RUNS} times: {why}")
}

/// Asks the manager, and again when it was killed before it answered: it
/// is pure, so asking again gives the answer it would have given. Killed
/// `MAX_TRIES` times in a row, it is lost; the event is not.
fn ask_again(exe: &Path, input: &StepInput) -> Result<StepAnswer, Failure> {
    let mut why = String::new();
    for _ in 0..MAX_TRIES {
        match ask(exe, input) {
            Err(Failure::Lost(w)) => why = w,
            other => return other,
        }
    }
    Err(Failure::Lost(format!(
        "the manager was killed {MAX_TRIES} times ({why})"
    )))
}

/// Runs `itero manager` on one input.
fn ask(exe: &Path, input: &StepInput) -> Result<StepAnswer, Failure> {
    let mut c = Command::new(exe)
        .arg("manager")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start the manager: {e}"))?;
    let text = serde_json::to_string(input).map_err(|e| e.to_string())?;
    // a manager killed before it read its input closes the pipe: its exit
    // below says so, not this write
    let written = c.stdin.take().expect("piped").write_all(text.as_bytes());
    let out = c.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.code().is_none() {
        return Err(Failure::Lost(format!(
            "the manager was killed ({})",
            out.status
        )));
    }
    written.map_err(|e| format!("cannot give the manager its input: {e}"))?;
    if !out.status.success() {
        return Err(Failure::Error(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(serde_json::from_slice(&out.stdout).map_err(|e| format!("the manager's answer: {e}"))?)
}

/// Marks an event handled with the reason it could not be delivered.
fn give_up(j: &mut Journal, seq: i64, at: &str, why: String) -> Result<Delivered> {
    j.handle(Handled {
        seq,
        at,
        error: Some(why),
        run: None,
        timers: None,
        events: vec![],
        splits: vec![],
    })?;
    Ok(Delivered::Handled)
}

/// Delivers one event to its run's manager and saves the answer.
fn deliver(
    ws: &Workspace,
    j: &mut Journal,
    r: &Recorded,
    now: DateTime<Utc>,
    exe: &Path,
) -> Result<Delivered> {
    let at = stamp(now);
    let Some(id) = journal::run_of(&r.event.subject).map(str::to_string) else {
        return give_up(j, r.seq, &at, format!("no run in {}", r.event.subject));
    };
    let Some(run) = j.run(&id)? else {
        return give_up(j, r.seq, &at, format!("no run {id}"));
    };
    let Some((root, values)) = j.pinned(&run.pin)? else {
        return give_up(
            j,
            r.seq,
            &at,
            format!("version {} is not in the journal", run.pin),
        );
    };
    // what the manager needs beyond the run: the args of the actions its
    // workflows name, and which of their $$ names are defined
    let mut args = BTreeMap::new();
    let mut names = vec![];
    for (n, v) in &values {
        let f = match flow::from_value(v) {
            Ok(f) => f,
            Err(e) => return give_up(j, r.seq, &at, format!("{n}: {e}")),
        };
        for u in f.uses() {
            if let Name::Action { name, .. } = u
                && !args.contains_key(&name)
            {
                match ws.args(&name) {
                    Ok(Some(a)) => {
                        args.insert(name, a);
                    }
                    Ok(None) => {}
                    Err(e) => return give_up(j, r.seq, &at, format!("{e:#}")),
                }
            }
        }
        names.extend(f.defined_names());
    }
    names.sort();
    names.dedup();
    let input = StepInput {
        now: at.clone(),
        workflow: root,
        flows: values,
        args,
        defined: names.into_iter().filter(|n| defined(n)).collect(),
        runtime: run.runtime.clone(),
        event: Event {
            id: r.event.id.clone(),
            kind: r.event.kind.clone(),
            data: r.event.data.clone(),
        },
    };
    let answer = match ask_again(exe, &input) {
        Ok(a) => a,
        Err(Failure::Error(why)) => return give_up(j, r.seq, &at, why),
        Err(Failure::Lost(why)) => {
            eprintln!(
                "{}: {id}: {why}; {} waits for the next pass",
                run.workflow, r.event.kind
            );
            return Ok(Delivered::Deferred);
        }
    };
    for line in &answer.log {
        eprintln!("{}: {id}: {line}", run.workflow);
    }
    if let Some(steps::Ended::Error { why }) = &answer.runtime.ended {
        eprintln!("{}: {id}: error: {why}", run.workflow);
    }
    let mut events: Vec<Envelope> = answer
        .jobs
        .iter()
        .map(|job| {
            Envelope::new(
                &format!("manager/{id}"),
                "action.requested",
                &journal::subject("worker"),
                &at,
                Some(r.event.id.clone()),
                json!({"run": id, "workflow": run.workflow, "job": job}),
            )
        })
        .collect();
    // a run starting to wait gets the messages it may have missed
    if steps::is_waiting(&answer.runtime) && !steps::is_waiting(&run.runtime) {
        let first = j.started_by(&id)?;
        for m in j.messages_since(&run.started, first.as_deref())? {
            events.push(journal::copy(&m.event, &id, &at));
        }
    }
    let splits = answer
        .split
        .iter()
        .map(|rt| Split {
            workflow: &run.workflow,
            pin: &run.pin,
            runtime: rt,
            input: &run.input,
        })
        .collect();
    j.handle(Handled {
        seq: r.seq,
        at: &at,
        error: None,
        run: Some((&id, &answer.runtime, answer.runtime.ended.is_some())),
        timers: Some(&answer.timers),
        events,
        splits,
    })?;
    super::die_at("answer");
    Ok(Delivered::Handled)
}

/// Whether a name can be an environment variable: a letter or `_`, then
/// letters, digits or `_`.
fn env_name(n: &str) -> bool {
    let mut cs = n.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A value of a step's data or args: `"$name"` is the state's, `"$$NAME"`
/// the secret's (or the environment variable's); anything else itself.
fn resolve(v: &Value, state: &Map<String, Value>) -> Result<Value, String> {
    if let Some(n) = flow::secret_ref(v) {
        return super::secrets::get(n)
            .map(Value::String)
            .ok_or_else(|| format!("{n} is not set: no secret, no environment variable"));
    }
    if let Some(n) = flow::state_ref(v) {
        return Ok(state.get(n).cloned().unwrap_or(Value::Null));
    }
    Ok(v.clone())
}

/// An action's environment: each value found first in the step's data,
/// then its args (its item, after a split), then the run's state. Names
/// that cannot be environment variables are on stdin only.
pub fn bind(
    state: &Map<String, Value>,
    args: Option<&Value>,
    job: &StepJob,
) -> Result<BTreeMap<String, String>, String> {
    let mut values = state.clone();
    let args = match (&job.item, args) {
        (Some(item), _) => item.as_object().cloned(),
        (None, Some(Value::Object(o))) => Some(o.clone()),
        _ => None,
    };
    for layer in [args.unwrap_or_default(), job.data.clone()] {
        for (k, v) in layer {
            values.insert(k, resolve(&v, state)?);
        }
    }
    let mut env = BTreeMap::new();
    for (k, v) in values {
        if !env_name(&k) {
            continue;
        }
        match v {
            Value::Null => {}
            Value::String(s) => {
                env.insert(k, s);
            }
            other => {
                env.insert(k, other.to_string());
            }
        }
    }
    Ok(env)
}

/// What a worker is given for an `action.requested` event: the action's
/// file, its values bound, the run's state.
fn step_run(ws: &Workspace, j: &Journal, data: &Value) -> Result<StepRun, String> {
    let run_id = data["run"].as_str().unwrap_or("");
    let workflow = data["workflow"].as_str().unwrap_or("");
    let job: StepJob =
        serde_json::from_value(data["job"].clone()).map_err(|e| format!("the job: {e}"))?;
    let run = j
        .run(run_id)
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| format!("no run {run_id}"))?;
    let file = match ws.action_file(&job.action, job.version.as_deref()) {
        Ok(f) => f,
        Err(_) if job.action == NOTIFY && job.version.is_none() => PathBuf::from(NOTIFY),
        Err(e) => return Err(format!("#{}: {e:#}", job.action)),
    };
    let args = ws.args(&job.action).map_err(|e| format!("{e:#}"))?;
    Ok(StepRun {
        file,
        root: ws.root.clone(),
        env: bind(&run.runtime.state, args.as_ref(), &job)?,
        state: run.runtime.state.clone(),
        memory: ws.root.join(".itero").join("memory").join(workflow),
        waiting_since: job.waiting_since.clone(),
        timeout: STEP_TIMEOUT,
    })
}

/// Kills a worker's whole process group, then reaps the worker.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: killpg only sends a signal to the worker's own process group,
    // which it leads (process_group(0) in `spawn_worker`). A worker already
    // reaped leaves its group's id taken while anything in it still runs.
    unsafe {
        libc::killpg(child.id() as i32, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `itero worker` on a step in its own process group, with the
/// daemon's own folder first on its PATH. Past its time the group is
/// killed and the job is lost: it runs again.
fn spawn_worker(exe: &Path, step: &StepRun) -> Result<Outcome, Failure> {
    let mut c = Command::new(exe);
    c.arg("worker")
        .env("PATH", crate::sources::supervise::path_with(exe))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start a worker: {e}"))?;
    let text = serde_json::to_string(step).map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let read = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = vec![];
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    );
    // when the group is killed, the readers end as its pipes close; they
    // are not waited for, in case something outside the group holds them
    let limit = std::time::Duration::from_secs(step.timeout);
    let Some(status) = child.wait_timeout(limit).map_err(|e| e.to_string())? else {
        // still running, not yet reaped: its group's id is still its own
        stop(&mut child);
        return Err(Failure::Lost(format!(
            "took longer than {}",
            humantime::format_duration(limit)
        )));
    };
    let Some(code) = status.code() else {
        // killed from outside, and already reaped: its group is not killed,
        // since its id may be another process's by now
        return Err(Failure::Lost(format!("the worker was killed ({status})")));
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    let why = String::from_utf8_lossy(&err).trim().to_string();
    let why = why.strip_prefix("error: ").unwrap_or(&why).to_string();
    match code {
        0 => Ok(serde_json::from_slice(&out).map_err(|e| format!("the worker's answer: {e}"))?),
        LOST_EXIT => Err(Failure::Lost(why)),
        _ if why.is_empty() => Err(Failure::Error("the worker failed".into())),
        _ => Err(Failure::Error(why)),
    }
}

/// Sends a job's outcome to its run, in one transaction.
fn save_result(
    j: &mut Journal,
    r: &Recorded,
    outcome: Result<Outcome, String>,
    now: DateTime<Utc>,
) -> Result<()> {
    let data = &r.event.data;
    let path = data.pointer("/job/path").cloned().unwrap_or(Value::Null);
    let (kind, result) = match outcome {
        Ok(Outcome::Done(m)) => ("action.finished", json!({"path": path, "output": m})),
        Ok(Outcome::NotYet(m)) => ("action.waiting", json!({"path": path, "output": m})),
        Err(why) => ("action.failed", json!({"path": path, "error": why})),
    };
    let run = data["run"].as_str().unwrap_or("");
    let e = Envelope::new(
        &format!("worker/{}", r.event.id),
        kind,
        &journal::subject(run),
        &stamp(now),
        Some(r.event.id.clone()),
        result,
    );
    j.handle(Handled {
        seq: r.seq,
        at: &stamp(now),
        error: None,
        run: None,
        timers: None,
        events: vec![e],
        splits: vec![],
    })
}
