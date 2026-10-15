//! The daemon's work for one workspace, one pass at a time: fire due
//! timers, deliver every waiting event to its run's manager, save each
//! answer, and run the jobs it asks for in workers. The daemon decides
//! nothing: every change comes from a manager's answer.
use super::journal::{self, Envelope, Handled, Journal, Recorded};
use super::worker::{Failure, LOST_EXIT};
use super::{Answer, Event, Input, Interpreter, Job, Program};
use crate::workflow::{FAILED, actions};
use crate::workspace::Workspace;
use anyhow::{Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
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
    /// Jobs lost (killed, or their lease ran out), to run again.
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
/// before its action fails.
const MAX_RUNS: i64 = 3;

/// The built-ins a worker runs; any other action is a workspace action.
const WORKER_BUILT_INS: [&str; 4] = ["http.request@1", "command@1", "sh@1", "notify@1"];

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

/// How the daemon's loop is doing, for `/health`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Health {
    pub started: String,
    /// Passes so far.
    pub passes: u64,
    /// When the last pass ran.
    pub last_pass: Option<String>,
    /// What went wrong in the last pass, if anything.
    pub last_error: Option<String>,
}

impl Health {
    pub fn starting(at: &str) -> Health {
        Health {
            started: at.to_string(),
            passes: 0,
            last_pass: None,
            last_error: None,
        }
    }
}

/// Runs passes over the workspace until `wake`'s sender is gone: one every
/// `interval`, or at once when something arrives on `wake` (an event from
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

/// One pass over a workspace; `exe` is the itero binary, run as `itero
/// manager` and `itero worker`.
pub fn run_once(ws: &Workspace, now: DateTime<Utc>, exe: &Path) -> Result<Pass> {
    let mut j = Journal::open(ws)?;
    let mut pass = Pass::default();
    fire_schedules(ws, &mut j, now)?;
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
    // next events
    loop {
        while let Some(r) = j.next()? {
            if r.event.subject == journal::subject("app") {
                start_triggered(ws, &mut j, &r, now)?;
                pass.delivered += 1;
                continue;
            }
            if let Delivered::Deferred = deliver(&mut j, &r, now, exe)? {
                // events are delivered in order: the rest waits with it
                pass.deferred += 1;
                return Ok(pass);
            }
            pass.delivered += 1;
            if pass.delivered >= MAX_EVENTS {
                return Ok(pass);
            }
        }
        let batch = batch(j.jobs()?);
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
                starting.push((r, runs + 1));
            }
        }
        super::die_at("job-started");
        let beats = ws.root.join(".itero").join("beats");
        // the workers run side by side; each result is saved on its own
        let outcomes: Vec<Result<Value, Failure>> = std::thread::scope(|s| {
            let running: Vec<_> = starting
                .iter()
                .map(|(r, _)| {
                    let beat = beats.join(&r.event.id);
                    s.spawn(move || {
                        let outcome = job(ws, &r.event.data)
                            .map_err(Failure::Error)
                            .and_then(|job| spawn_worker(exe, &job, &beat));
                        let _ = std::fs::remove_file(&beat);
                        outcome
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
        for ((r, run), outcome) in starting.iter().zip(outcomes) {
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
    Ok(pass)
}

/// Starts a run of every workflow whose schedule is due. A schedule seen for
/// the first time only starts counting: it waits for its first slot.
fn fire_schedules(ws: &Workspace, j: &mut Journal, now: DateTime<Utc>) -> Result<()> {
    // a schedule's key names it, not its place among the triggers: merging
    // or reordering keeps its last slot; the same schedule twice is one
    let mut seen = BTreeSet::new();
    for (_, f) in ws.workflows()? {
        for t in &f.triggers {
            let Some(schedule) = &t.schedule else {
                continue;
            };
            let key = format!("{}#{schedule}", f.name);
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(last) = j.schedule_last(&key)? else {
                j.set_schedule_last(&key, &stamp(now))?;
                continue;
            };
            let last: DateTime<Utc> = last
                .parse()
                .map_err(|_| anyhow!("schedule {key}: its last slot is not a time: {last}"))?;
            if super::schedule::due(schedule, last, now).map_err(|e| anyhow!(e))? {
                j.start_run(&f, &t.args, &stamp(now), Some(&key))?;
            }
        }
    }
    Ok(())
}

/// An app's event: a run of every workflow with an `on` trigger for its
/// type, the trigger's args and the event's data as input.
fn start_triggered(
    ws: &Workspace,
    j: &mut Journal,
    r: &Recorded,
    now: DateTime<Utc>,
) -> Result<()> {
    let flows = ws.workflows()?;
    let starts: Vec<(&crate::workflow::File, &Map<String, Value>)> = flows
        .iter()
        .flat_map(|(_, f)| {
            f.triggers
                .iter()
                .filter(|t| t.on.as_deref() == Some(r.event.kind.as_str()))
                .map(move |t| (f, &t.args))
        })
        .collect();
    let ids = j.start_runs_for_event(r.seq, &starts, &r.event.data, &stamp(now))?;
    if ids.is_empty() {
        eprintln!("{}: no workflow starts on it", r.event.kind);
    }
    Ok(())
}

/// Why a job that kept being lost failed.
fn lost_too_often(why: &str) -> String {
    format!("lost {MAX_RUNS} times: {why}")
}

/// More workers than this at once wait for the next batch.
const MAX_WORKERS: usize = 8;

/// The jobs to run side by side: the oldest for each run (a run's jobs run
/// in order), at most `MAX_WORKERS`.
fn batch(jobs: Vec<Recorded>) -> Vec<Recorded> {
    let mut runs = BTreeSet::new();
    jobs.into_iter()
        .filter(|r| runs.insert(job_run(&r.event.data)))
        .take(MAX_WORKERS)
        .collect()
}

/// The run a job is for.
fn job_run(data: &Value) -> String {
    data.pointer("/run/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Asks the manager, and again when it was killed before it answered: it
/// is pure, so asking again gives the answer it would have given. Killed
/// `MAX_TRIES` times in a row, it is lost; the event is not.
fn ask_again(exe: &Path, input: &Input) -> Result<Answer, Failure> {
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
fn ask(exe: &Path, input: &Input) -> Result<Answer, Failure> {
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
        memory: None,
    })?;
    Ok(Delivered::Handled)
}

/// Delivers one event to its run's manager and saves the answer.
fn deliver(j: &mut Journal, r: &Recorded, now: DateTime<Utc>, exe: &Path) -> Result<Delivered> {
    let Some(id) = journal::run_of(&r.event.subject).map(str::to_string) else {
        return give_up(
            j,
            r.seq,
            &stamp(now),
            format!("no run in {}", r.event.subject),
        );
    };
    let Some(run) = j.run(&id)? else {
        return give_up(j, r.seq, &stamp(now), format!("no run {id}"));
    };
    let Some(workflow) = j.workflow(&run.pin)? else {
        return give_up(
            j,
            r.seq,
            &stamp(now),
            format!("workflow {} is not in the journal", run.pin),
        );
    };
    let mut run_map = Map::new();
    run_map.insert("id".into(), json!(run.id));
    run_map.insert("workflow".into(), json!(run.workflow));
    run_map.insert("started".into(), json!(run.started));
    let input = Input {
        now: stamp(now),
        workflow: workflow.clone(),
        run: run_map,
        args: run.args.clone(),
        memory: j.memory(&run.workflow)?,
        runtime: run.runtime.clone(),
        event: Event {
            id: r.event.id.clone(),
            kind: r.event.kind.clone(),
            data: r.event.data.clone(),
        },
    };
    let answer = match ask_again(exe, &input) {
        Ok(a) => a,
        Err(Failure::Error(why)) => return give_up(j, r.seq, &stamp(now), why),
        Err(Failure::Lost(why)) => {
            eprintln!(
                "{}: {id}: {why}; {} waits for the next pass",
                run.workflow, r.event.kind
            );
            return Ok(Delivered::Deferred);
        }
    };
    let events = answer
        .events
        .iter()
        .map(|e| {
            Envelope::new(
                &format!("manager/{id}"),
                &e.kind,
                &journal::subject(&e.target),
                &stamp(now),
                Some(r.event.id.clone()),
                e.data.clone(),
            )
        })
        .collect();
    for line in &answer.log {
        eprintln!("{}: {id}: {line}", run.workflow);
    }
    let state = &answer.runtime.state;
    let ended = answer.runtime.chain.is_none() && (workflow.end.contains(state) || state == FAILED);
    j.handle(Handled {
        seq: r.seq,
        at: &stamp(now),
        error: None,
        run: Some((&id, &answer.runtime, ended)),
        timers: answer.timers.as_deref(),
        events,
        memory: (!answer.memory.is_empty()).then_some((&run.workflow, &answer.memory)),
    })?;
    super::die_at("answer");
    Ok(Delivered::Handled)
}

/// Seconds a built-in may take.
const BUILT_IN_TIMEOUT: u64 = crate::workflow::actions::DEFAULT_TIMEOUT;

/// `${env.NAME}` in every string, filled in from the OS secret store, else
/// the daemon's environment.
fn fill_secrets(v: &Value) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) => {
            use super::eval::{Piece, pieces, secret_name};
            let mut out = String::new();
            for piece in pieces(s)? {
                match piece {
                    Piece::Text(t) => out += t,
                    Piece::Expr(e) => match secret_name(e) {
                        Some(name) => {
                            out += &super::secrets::get(name)
                                .ok_or_else(|| format!("secret {name} is not set"))?
                        }
                        // any other template was filled in by the manager
                        None => out += e,
                    },
                }
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(fill_secrets).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| Ok((k.clone(), fill_secrets(x)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// The job for an `action.requested` event: arguments with secrets, and
/// for a workspace action, the program that runs it.
fn job(ws: &Workspace, data: &Value) -> Result<Job, String> {
    let get = |k: &str| data.get(k).cloned().unwrap_or(Value::Null);
    let obj = |k: &str| get(k).as_object().cloned().unwrap_or_default();
    let action = get("action").as_str().unwrap_or("").to_string();
    let with = match fill_secrets(&get("with"))? {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    let mut job = Job {
        action: action.clone(),
        with,
        key: get("key").as_str().unwrap_or("").to_string(),
        input: get("input"),
        prev: get("prev"),
        run: obj("run"),
        memory: obj("memory"),
        args: obj("args"),
        root: ws.root.clone(),
        program: None,
        timeout: BUILT_IN_TIMEOUT,
        heartbeat: None,
    };
    if WORKER_BUILT_INS.contains(&action.as_str()) {
        return Ok(job);
    }
    let (name, target, version) =
        actions::split(&action).ok_or_else(|| format!("unknown action {action}"))?;
    let (folder, d) = ws.action(name).map_err(|e| e.to_string())?;
    if d.version.to_string() != version {
        return Err(format!(
            "action {name} is at version {} now, not {version}",
            d.version
        ));
    }
    let t = d
        .targets
        .get(target)
        .ok_or_else(|| format!("action {name} has no target {target}"))?;
    let secs = |d: &str| -> Result<u64, String> {
        Ok(crate::workflow::expr::duration(d)?.as_secs().max(1))
    };
    if let Some(l) = &d.limits {
        if let Some(t) = &l.timeout {
            job.timeout = secs(t)?;
        }
        job.heartbeat = l.heartbeat.as_deref().map(secs).transpose()?;
    }
    let folder = match &d.working_directory {
        Some(w) => folder.join(w),
        None => folder,
    };
    let (kind, version) = d.kind.split_once('@').unwrap_or((d.kind.as_str(), ""));
    let interpreter = match kind {
        "executable" => None,
        "python" => Some(Interpreter::Python {
            version: version.to_string(),
            home: d.home.clone().unwrap_or_else(|| ".venv".into()),
            requirements: d.requirements.clone(),
            module: t.module.clone().unwrap_or_default(),
            callable: t.callable.clone().unwrap_or_default(),
        }),
        "bun" => Some(Interpreter::Bun {
            file: t.file.clone().unwrap_or_default(),
            export: t.export.clone().unwrap_or_default(),
        }),
        other => return Err(format!("unknown action type {other}")),
    };
    let mut environment = std::collections::BTreeMap::new();
    for (k, v) in &d.environment {
        let filled = fill_secrets(&Value::String(v.clone()))?;
        environment.insert(k.clone(), filled.as_str().unwrap_or("").to_string());
    }
    job.program = Some(Program {
        folder,
        executable: t.executable.clone().unwrap_or_default(),
        arguments: t.arguments.clone(),
        environment,
        interpreter,
    });
    Ok(job)
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

/// How long since a job last touched its heartbeat file, or since it
/// started (`born`) when it has not.
fn quiet(beat: &Path, born: std::time::SystemTime) -> std::time::Duration {
    let last = std::fs::metadata(beat)
        .and_then(|m| m.modified())
        .map_or(born, |m| m.max(born));
    std::time::SystemTime::now()
        .duration_since(last)
        .unwrap_or_default()
}

/// Runs `itero worker` on a job in its own process group. The group is
/// killed at the job's time limit (an error), or when its lease runs out
/// without a heartbeat (lost: the job runs again). `beat` is the job's
/// heartbeat file.
fn spawn_worker(exe: &Path, job: &Job, beat: &Path) -> Result<Value, Failure> {
    let mut c = Command::new(exe);
    c.arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    if job.heartbeat.is_some() {
        if let Some(dir) = beat.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // a beat left by an earlier run must not renew this one
        let _ = std::fs::remove_file(beat);
        c.env("ITERO_HEARTBEAT", beat);
    }
    let born = std::time::SystemTime::now();
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start a worker: {e}"))?;
    let text = serde_json::to_string(job).map_err(|e| e.to_string())?;
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
    // waits for the worker's exit, the time limit or the end of its lease,
    // whichever comes first. When the group is killed, the readers end as
    // its pipes close; they are not waited for, in case something outside
    // the group holds them.
    let started = std::time::Instant::now();
    let cap = std::time::Duration::from_secs(job.timeout);
    let lease = job.heartbeat.map(std::time::Duration::from_secs);
    let status = loop {
        let left = cap.saturating_sub(started.elapsed());
        if left.is_zero() {
            stop(&mut child);
            return Err(Failure::Error(format!(
                "{}: timed out after {}s",
                job.action, job.timeout
            )));
        }
        let slice = match lease {
            Some(_) => left.min(std::time::Duration::from_millis(200)),
            None => left,
        };
        if let Some(status) = child.wait_timeout(slice).map_err(|e| e.to_string())? {
            break status;
        }
        if let Some(lease) = lease
            && quiet(beat, born) > lease
        {
            stop(&mut child);
            return Err(Failure::Lost(format!(
                "no heartbeat for {}s",
                lease.as_secs()
            )));
        }
    };
    let Some(code) = status.code() else {
        // killed from outside: whatever it started goes too
        stop(&mut child);
        return Err(Failure::Lost(format!("the worker was killed ({status})")));
    };
    let (out, err) = (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    );
    let why = String::from_utf8_lossy(&err).trim().to_string();
    let why = why.strip_prefix("error: ").unwrap_or(&why).to_string();
    match code {
        0 => {
            let v: Value =
                serde_json::from_slice(&out).map_err(|e| format!("the worker's answer: {e}"))?;
            Ok(v.get("output").cloned().unwrap_or(Value::Null))
        }
        LOST_EXIT => Err(Failure::Lost(why)),
        _ if why.is_empty() => Err(Failure::Error("the worker failed".into())),
        _ => Err(Failure::Error(why)),
    }
}

/// Sends a job's outcome to its run, in one transaction.
fn save_result(
    j: &mut Journal,
    r: &Recorded,
    outcome: Result<Value, String>,
    now: DateTime<Utc>,
) -> Result<()> {
    let data = &r.event.data;
    let (kind, mut result) = match outcome {
        Ok(output) => ("action.finished", json!({"output": output})),
        Err(why) => ("action.failed", json!({"error": why})),
    };
    result["chain"] = data.get("chain").cloned().unwrap_or(Value::Null);
    result["index"] = data.get("index").cloned().unwrap_or(Value::Null);
    let e = Envelope::new(
        &format!("worker/{}", r.event.id),
        kind,
        &journal::subject(&job_run(data)),
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
        memory: None,
    })
}
