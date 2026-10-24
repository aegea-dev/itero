//! The daemon's work for one workspace, one pass at a time: start the runs
//! schedules and messages call for, fire due timers, deliver every waiting
//! event to its run's manager, save each answer, and run the jobs it asks
//! for in workers. The daemon decides nothing about a run: every change to
//! one comes from a manager's answer.
use super::Event;
use super::events;
use super::journal::{self, Envelope, Handled, Journal, Recorded, Split, Start};
use super::log::Log;
use super::steps::{self, StepAnswer, StepInput, StepJob};
use super::worker::{self, Failure, LOST_EXIT, Outcome, StepRun};
use crate::flow::{self, Flow, Name};
use crate::workspace::Workspace;
use anyhow::{Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
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
    /// It stopped at its cap of events with more waiting.
    pub more: bool,
}

/// More events than this in one pass wait for the next.
const MAX_EVENTS: usize = 10_000;

/// How many times a manager killed before it answered is asked again,
/// before the event waits for the next pass.
const MAX_TRIES: usize = 3;

/// How many times a job runs, the first run and those after it was lost,
/// before its step is an error.
const MAX_RUNS: i64 = 3;

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
    /// This process's life, made the first time it is asked for.
    pub fn current() -> &'static Daemon {
        static LIFE: std::sync::OnceLock<Daemon> = std::sync::OnceLock::new();
        LIFE.get_or_init(|| Daemon {
            id: uuid::Uuid::now_v7().to_string(),
            name: crate::names::random(),
            started: stamp(Utc::now()),
            pid: std::process::id(),
            host: host(),
            itero: journal::ITERO.into(),
        })
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
    /// When the pass under way began; none while the daemon waits.
    pub busy_since: Option<String>,
    /// How each connector is doing, by the type it sends.
    pub connectors: BTreeMap<String, ConnectorHealth>,
}

/// How one connector is doing.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ConnectorHealth {
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
            daemon: Daemon::current().clone(),
            started: at.to_string(),
            passes: 0,
            last_pass: None,
            last_error: None,
            busy_since: None,
            connectors: Default::default(),
        }
    }
}

/// A pass that fails, or cannot tell when it is next due, is tried again
/// after this, whatever else wakes the daemon sooner.
const AFTER_FAILURE: chrono::Duration = chrono::Duration::minutes(1);

/// Runs a pass each time something arrives on `woken` (a message, a line
/// from a connector, a watched path or an edit, the alarm), until its
/// senders are gone. After a pass the alarm is set to the next due time,
/// or to none; between passes the daemon waits, with no timeout. Each pass
/// is recorded in `health`, and `report` hears of it; a pass that fails
/// never stops the next.
pub fn run_loop(
    ws: &Workspace,
    exe: &Path,
    alarm: &super::alarm::Alarm,
    woken: &Receiver<()>,
    health: &Mutex<Health>,
    report: impl Fn(&Result<Pass>),
) {
    let lock = || health.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        let now = Utc::now();
        lock().busy_since = Some(stamp(now));
        let pass = run_once(ws, now, exe);
        {
            let mut h = lock();
            h.passes += 1;
            h.last_pass = Some(stamp(now));
            h.last_error = pass.as_ref().err().map(|e| format!("{e:#}"));
            h.busy_since = None;
        }
        report(&pass);
        let next = match &pass {
            Ok(p) if p.more => Ok(Some(now)),
            // a lost manager is asked again, as before, a second on
            Ok(p) if p.deferred > 0 => Ok(Some(now + chrono::Duration::seconds(1))),
            Ok(_) => next_due(ws, Utc::now()),
            Err(_) => Ok(Some(now + AFTER_FAILURE)),
        };
        let next = next.unwrap_or_else(|e| {
            report(&Err(e.context("cannot tell when the next pass is due")));
            Some(Utc::now() + AFTER_FAILURE)
        });
        if let Err(e) = alarm.set(next) {
            report(&Err(anyhow!("cannot set the alarm for the next pass: {e}")));
        }
        if woken.recv().is_err() {
            return;
        }
        // several wake-ups while a pass ran need only one more pass
        while woken.try_recv().is_ok() {}
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
        let entries = ws.entries("workflow")?;
        let mut read = BTreeMap::new();
        for (n, v) in &values {
            read.insert(n.clone(), ws.read_flow(n, v, &entries)?);
        }
        Ok(Flows { values, read })
    }

    /// Whether a workflow looks after the workspace (`meta.internal`).
    fn internal(&self, name: &str) -> bool {
        self.read.get(name).is_some_and(|f| f.meta.internal)
    }

    /// The internal workflows' names.
    fn internals(&self) -> Vec<String> {
        self.read
            .iter()
            .filter(|(_, f)| f.meta.internal)
            .map(|(n, _)| n.clone())
            .collect()
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

/// A `path` entry as a path: `~/` is the home folder, `$$NAME` an
/// environment variable (never a secret: the path is in runs and the
/// log), and a relative path is under the workspace.
pub fn expand(root: &Path, path: &str) -> Result<PathBuf, String> {
    let (mut out, mut rest) = match path.strip_prefix("~/") {
        Some(r) => (
            std::env::var("HOME").map_err(|_| format!("{path}: HOME is not set"))? + "/",
            r,
        ),
        None => (String::new(), path),
    };
    while let Some(i) = rest.find("$$") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let n = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..n];
        let v = std::env::var(name)
            .ok()
            .filter(|_| env_name(name))
            .ok_or_else(|| format!("{path}: no environment variable {name}"))?;
        out.push_str(&v);
        rest = &after[n..];
    }
    out.push_str(rest);
    Ok(root.join(out))
}

/// Every path the workflows' `on` names, filled in, and what could not
/// be.
fn wanted_paths(ws: &Workspace) -> (Vec<PathBuf>, Vec<String>) {
    let (mut paths, mut errors) = (vec![], vec![]);
    match ws.flows() {
        Ok(flows) => {
            for (name, f) in flows {
                for p in &f.on.paths {
                    match expand(&ws.root, p) {
                        Ok(p) if !paths.contains(&p) => paths.push(p),
                        Ok(_) => {}
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
            }
        }
        Err(e) => errors.push(format!("{e:#}")),
    }
    paths.sort();
    (paths, errors)
}

/// Watches every path a workflow's `on` names and the workspace's own
/// files (`Workspace::own_files`), all in this one thread. A watched path,
/// once quiet for half a second, sends `itero.path.changed`; an edit to the
/// workspace's own files wakes the daemon, which reads them again, and
/// moves the watches. A path whose folder is not there yet is watched for
/// from the nearest folder that is. `Workspace::wake_file`, touched by
/// other processes once they saved a message, wakes the daemon too. It
/// waits in the kernel, with no timeout while nothing is pending. Runs for
/// good.
pub fn watch_paths(ws: &Workspace, wake: &std::sync::mpsc::Sender<()>, log: &Log) {
    use crate::watch::{Debouncer, Kind};
    use std::time::{Duration, Instant};
    let mut backend = match crate::watch::open_many(&[]) {
        Ok(b) => b,
        Err(e) => {
            log.write(
                "error",
                json!({ "why": format!("cannot watch files: {e}") }),
            );
            return;
        }
    };
    let mut d = Debouncer::new(Duration::from_millis(500), Duration::from_secs(5));
    let mut watched: Option<Vec<PathBuf>> = None;
    let mut said = vec![];
    let mut look = true;
    let (mut paths, mut own, mut lookouts) = (vec![], vec![], vec![]);
    let poke = ws.wake_file();
    loop {
        if look {
            let mut errors;
            (paths, errors) = wanted_paths(ws);
            own = ws.own_files().unwrap_or_else(|e| {
                errors.push(format!("{e:#}"));
                vec![]
            });
            // the nearest folder there is above one that is not there yet
            lookouts = paths
                .iter()
                .filter(|p| p.parent().is_some_and(|d| !d.is_dir()))
                .filter_map(|p| p.ancestors().skip(1).find(|a| a.is_dir()))
                .map(Path::to_path_buf)
                .collect();
            let mut all: Vec<PathBuf> = paths
                .iter()
                .chain(&own)
                .chain(&lookouts)
                .chain([&poke])
                .cloned()
                .collect();
            all.sort();
            all.dedup();
            if watched.as_ref() != Some(&all) {
                match backend.set(&all) {
                    Ok(()) => {
                        log.write("paths.watched", json!({ "paths": paths }));
                        watched = Some(all);
                    }
                    // a folder may come: looked for again at the next edit
                    Err(e) => errors.push(e.to_string()),
                }
            }
            if errors != said {
                for why in &errors {
                    log.write("error", json!({ "why": why }));
                }
                said = errors;
            }
            look = false;
        }
        let timeout = d
            .due()
            .map(|at| at.saturating_duration_since(Instant::now()));
        match backend.wait(timeout) {
            Ok(changes) => d.add(changes, Instant::now()),
            Err(e) => {
                log.write("error", json!({ "why": format!("watching files: {e}") }));
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        let mut woke = false;
        for c in d.take(Instant::now()) {
            if own.contains(&c.path) {
                look = true;
                woke = true;
            }
            if lookouts.contains(&c.path) {
                look = true;
            }
            if c.path == poke {
                woke = true;
            }
            if !paths.contains(&c.path) {
                continue;
            }
            let kind = match c.kind {
                Kind::Created => "created",
                Kind::Removed | Kind::Renamed => "removed",
                Kind::Changed | Kind::Rescan => "changed",
            };
            let m = events::own(
                events::PATH_CHANGED,
                json!({ "path": c.path.display().to_string(), "kind": kind }),
                &stamp(Utc::now()),
            );
            match Journal::open(ws).and_then(|j| j.publish(&m)) {
                Ok(()) => woke = true,
                Err(e) => log.write("error", json!({ "why": format!("{e:#}") })),
            }
        }
        if woke {
            let _ = wake.send(());
        }
    }
}

/// Whether an environment variable or a secret is defined.
fn defined(name: &str) -> bool {
    super::secrets::get(name).is_some()
}

/// One pass over a workspace; `exe` is the itero binary, run as `itero
/// manager` and `itero worker`. Ends by posting the callbacks due.
pub fn run_once(ws: &Workspace, now: DateTime<Utc>, exe: &Path) -> Result<Pass> {
    runs_pass(ws, now, exe)
}

fn runs_pass(ws: &Workspace, now: DateTime<Utc>, exe: &Path) -> Result<Pass> {
    let mut j = Journal::open(ws)?;
    let log = Log::new(ws, &j.workspace()?.id);
    let mut pass = Pass::default();
    let flows = Flows::load(ws)?;
    j.see_workflows(flows.read.keys(), &stamp(now))?;
    let no_room = watch_limits(ws, &j, now, &log)?;
    fire_schedules(&flows, &mut j, now, &log)?;
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
                if route(ws, &flows, &mut j, &r, now, &log, no_room)? {
                    pass.delivered += 1;
                } else {
                    held.push(r.seq);
                }
                continue;
            }
            if let Delivered::Deferred = deliver(ws, &mut j, &r, now, exe, &log)? {
                // events are delivered in order: the rest waits with it
                pass.deferred += 1;
                return Ok(pass);
            }
            pass.delivered += 1;
            if !held.is_empty() && j.active_count(&flows.internals())? < ws.limits.runs {
                held.clear();
            }
            if pass.delivered >= MAX_EVENTS {
                pass.more = true;
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
                let step = step_run(ws, &j, &r.event.data, &stamp(now));
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
    Ok(pass)
}

/// Sends `itero.space.full` when `.itero/` grows past its `space`, and
/// `itero.log.full` when the log grows past its `log` limit: each once,
/// until there is room again or the log is rolled. Returns whether
/// `.itero/` is past its space: then no new run starts but internal ones.
fn watch_limits(ws: &Workspace, j: &Journal, now: DateTime<Utc>, log: &Log) -> Result<bool> {
    let at = stamp(now);
    let (used, space) = (ws.used(), ws.limits.space);
    let no_room = used > space;
    if no_room && !j.flag(journal::SPACE_FULL)? {
        j.publish(&events::own(
            "itero.space.full",
            json!({ "used": used, "limit": space }),
            &at,
        ))?;
        j.set_flag(journal::SPACE_FULL, true)?;
        log.write(
            "error",
            json!({ "why": format!(".itero/ takes {used} bytes, past its space ({space}): no new runs until there is room") }),
        );
    } else if !no_room && j.flag(journal::SPACE_FULL)? {
        j.set_flag(journal::SPACE_FULL, false)?;
    }
    let path = ws.root.join(".itero").join("itero.log");
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    if size > ws.limits.log && !j.flag(journal::LOG_FULL)? {
        j.publish(&events::own(
            "itero.log.full",
            json!({ "path": path.display().to_string(), "size": size, "limit": ws.limits.log }),
            &at,
        ))?;
        j.set_flag(journal::LOG_FULL, true)?;
    }
    Ok(no_room)
}

/// Starts a run of every workflow whose schedule is due, when its
/// conditions hold. A schedule seen for the first time only starts
/// counting: it waits for its first slot.
/// A schedule's last slot, none before the daemon first saw it.
fn last_slot(j: &Journal, key: &str) -> Result<Option<DateTime<Utc>>> {
    j.schedule_last(key)?
        .map(|last| {
            last.parse()
                .map_err(|_| anyhow!("schedule {key}: its last slot is not a time: {last}"))
        })
        .transpose()
}

/// When the daemon next has something to do that no message brings: the
/// earliest of each schedule's next slot and a waiting run's timer; none
/// when there is nothing. A schedule not seen yet
/// is due `now`, as the next pass starts counting its slots. A workflow
/// that is off, or past its `until`, has no slots (an edit that turns it on
/// wakes the daemon); one before its `from` has its first slot from then.
pub fn next_due(ws: &Workspace, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
    let j = Journal::open(ws)?;
    let flows = Flows::load(ws)?;
    let mut due = vec![];
    for (name, f) in &flows.read {
        if f.meta.off || f.meta.until.is_some_and(|u| now >= u) {
            continue;
        }
        for cron in &f.on.schedules {
            let Some(last) = last_slot(&j, &format!("{name}#{cron}"))? else {
                due.push(now);
                continue;
            };
            // the slot at `from` itself counts
            let since = f
                .meta
                .from
                .map_or(last, |from| last.max(from - chrono::Duration::seconds(1)));
            due.push(super::schedule::next(cron, since).map_err(|e| anyhow!(e))?);
        }
    }
    if let Some(t) = j.next_timer()? {
        due.push(
            t.parse()
                .map_err(|_| anyhow!("a timer's time is not a time: {t}"))?,
        );
    }
    Ok(due.into_iter().min())
}

fn fire_schedules(flows: &Flows, j: &mut Journal, now: DateTime<Utc>, log: &Log) -> Result<()> {
    for (name, f) in &flows.read {
        for cron in &f.on.schedules {
            let key = format!("{name}#{cron}");
            let Some(last) = last_slot(j, &key)? else {
                j.set_schedule_last(&key, &stamp(now))?;
                continue;
            };
            // off or outside its dates, its slots pass without a run
            if !flow::active(&f.meta, now) {
                j.set_schedule_last(&key, &stamp(now))?;
                continue;
            }
            if !super::schedule::due(cron, last, now).map_err(|e| anyhow!(e))? {
                continue;
            }
            let pinned = flows.pinned(name);
            match pinned {
                Ok(pinned) if flow::holds(&f.on, &f.settings, &defined) => {
                    let start = Start {
                        workflow: name.clone(),
                        flows: pinned,
                        input: Value::Object(f.settings.clone()),
                        cause: None,
                    };
                    j.start_run(&start, &stamp(now), Some(&key))?;
                }
                Ok(_) => {
                    log.write(
                        "run.skipped",
                        json!({ "workflow": name, "schedule": cron, "why": "its on does not hold" }),
                    );
                    j.set_schedule_last(&key, &stamp(now))?;
                }
                Err(why) => {
                    log.write(
                        "error",
                        json!({ "workflow": name, "schedule": cron, "why": why }),
                    );
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
/// (not an internal one) while `runs` are under way, or while `.itero/`
/// has no room: it waits its turn. A message about a run never starts its
/// own workflow.
fn route(
    ws: &Workspace,
    flows: &Flows,
    j: &mut Journal,
    r: &Recorded,
    now: DateTime<Utc>,
    log: &Log,
    no_room: bool,
) -> Result<bool> {
    let data = r.event.data.as_object().cloned().unwrap_or_default();
    let on_a_run = r.event.kind.starts_with("itero.run.");
    // a watched path's change starts the workflows that name the path
    let changed = (r.event.kind == events::PATH_CHANGED)
        .then(|| data.get("path").and_then(Value::as_str))
        .flatten()
        .map(PathBuf::from);
    let names_it = |f: &Flow| {
        changed.as_ref().is_some_and(|c| {
            f.on.paths
                .iter()
                .any(|p| expand(&ws.root, p).is_ok_and(|p| p == *c))
        })
    };
    let mut starts = vec![];
    for (name, f) in &flows.read {
        // a run's state starts with its workflow's settings, the message's
        // data on top
        let mut first = f.settings.clone();
        first.extend(data.clone());
        if !(f.on.events.contains(&r.event.kind) || names_it(f))
            || !flow::active(&f.meta, now)
            || !flow::holds(&f.on, &first, &defined)
            || (on_a_run && data.get("workflow") == Some(&json!(name)))
        {
            continue;
        }
        match flows.pinned(name) {
            Ok(pinned) => starts.push(Start {
                workflow: name.clone(),
                flows: pinned,
                input: Value::Object(first),
                cause: Some(r.event.id.clone()),
            }),
            Err(why) => log.write(
                "error",
                json!({ "workflow": name, "message": r.event.id, "why": why }),
            ),
        }
    }
    let counted = starts.iter().any(|s| !flows.internal(&s.workflow));
    if counted && (no_room || j.active_count(&flows.internals())? >= ws.limits.runs) {
        return Ok(false);
    }
    // the daemon's own messages start workflows; no run waits for them,
    // but for a click on a notification, which a #notify step waits for
    let reaches =
        !r.event.kind.starts_with(events::OWN) || r.event.kind == events::NOTIFICATION_CLICKED;
    let waiting: Vec<String> = j
        .active_runs()?
        .into_iter()
        .filter(|run| reaches && steps::is_waiting(&run.runtime))
        .map(|run| run.id)
        .collect();
    let started = j.route(r, &starts, &waiting, &stamp(now))?;
    let mut line = json!({ "message": r.event.id, "type": r.event.kind, "data": r.event.data });
    if started.is_empty() && waiting.is_empty() {
        line["nothing"] = json!(true);
    } else {
        line["started"] = json!(started);
        line["reached"] = json!(waiting);
    }
    log.write("message", line);
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
fn give_up(log: &Log, j: &mut Journal, seq: i64, at: &str, why: String) -> Result<Delivered> {
    log.write("error", json!({ "why": why }));
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
    log: &Log,
) -> Result<Delivered> {
    let at = stamp(now);
    let Some(id) = journal::run_of(&r.event.subject).map(str::to_string) else {
        return give_up(log, j, r.seq, &at, format!("no run in {}", r.event.subject));
    };
    let Some(run) = j.run(&id)? else {
        return give_up(log, j, r.seq, &at, format!("no run {id}"));
    };
    let Some((root, values)) = j.pinned(&run.pin)? else {
        return give_up(
            log,
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
    let mut who = Who {
        run: &run,
        internal: false,
    };
    // whether it is internal is its entry's (or a built-in's own meta)
    let entries = ws.entries("workflow")?;
    for (n, v) in &values {
        let f = match ws.read_flow(n, v, &entries) {
            Ok(f) => f,
            Err(e) => return give_up(log, j, r.seq, &at, format!("{e:#}")),
        };
        if *n == root {
            who.internal = f.meta.internal;
        }
        for u in f.uses() {
            if let Name::Action { name, .. } = u
                && !args.contains_key(&name)
            {
                match ws.args(&name) {
                    Ok(Some(a)) => {
                        args.insert(name, a);
                    }
                    Ok(None) => {}
                    Err(e) => return give_up(log, j, r.seq, &at, format!("{e:#}")),
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
        Err(Failure::Error(why)) => return give_up(log, j, r.seq, &at, why),
        Err(Failure::Lost(why)) => {
            log.write(
                "error",
                who.line(
                    json!({ "why": format!("{why}; {} waits for the next pass", r.event.kind) }),
                ),
            );
            return Ok(Delivered::Deferred);
        }
    };
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
    // a run ending in error or failed: the daemon's own message says so
    if run.runtime.ended.is_none() {
        let why = answer
            .runtime
            .state
            .get("why")
            .cloned()
            .unwrap_or(Value::Null);
        let own = match &answer.runtime.ended {
            Some(steps::Ended::Error { .. }) => Some((
                "itero.run.error",
                json!({
                    "workflow": run.workflow, "run": id, "why": why,
                    "failed_step": answer.runtime.state.get("failed_step"),
                }),
            )),
            Some(steps::Ended::Failed) => Some((
                "itero.run.failed",
                json!({ "workflow": run.workflow, "run": id, "why": why }),
            )),
            _ => None,
        };
        if let Some((kind, data)) = own {
            events.push(events::own(kind, data, &at));
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
            message: run.message.as_deref(),
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
    tell(log, &who, r, &answer);
    super::die_at("answer");
    Ok(Delivered::Handled)
}

/// The run a log line is about, and whether its workflow is internal.
struct Who<'a> {
    run: &'a journal::Run,
    internal: bool,
}

impl Who<'_> {
    /// A line's `fields` and which run it is about.
    fn line(&self, mut fields: Value) -> Value {
        fields["workflow"] = json!(self.run.workflow);
        fields["run"] = json!(self.run.id);
        fields["run_name"] = json!(self.run.name);
        if self.internal {
            fields["internal"] = json!(true);
        }
        fields
    }
}

/// Logs what an answer, saved, did to a run: the event it answered, the
/// steps it starts, what the manager noted, and the run's end.
fn tell(log: &Log, who: &Who, r: &Recorded, answer: &StepAnswer) {
    let d = &r.event.data;
    match r.event.kind.as_str() {
        "run.started" => log.write(
            "run.started",
            who.line(match &r.event.cause {
                Some(m) => json!({ "message": m }),
                None => json!({ "schedule": true }),
            }),
        ),
        "action.finished" | "action.waiting" => log.write(
            if r.event.kind == "action.finished" {
                "step.done"
            } else {
                "step.waiting"
            },
            who.line(json!({ "step": d["step"], "output": d["output"] })),
        ),
        "action.failed" => log.write(
            "step.failed",
            who.line(json!({ "step": d["step"], "why": d["error"] })),
        ),
        _ => {}
    }
    for note in &answer.notes {
        let event = note["event"].as_str().unwrap_or("run.note");
        log.write(event, who.line(note.clone()));
    }
    for line in &answer.log {
        log.write("run.note", who.line(json!({ "note": line })));
    }
    for job in &answer.jobs {
        log.write(
            "step.started",
            who.line(json!({ "step": format!("#{}", job.action) })),
        );
    }
    let run = who.run;
    if run.runtime.ended.is_none()
        && let Some(ended) = &answer.runtime.ended
    {
        let why = match ended {
            steps::Ended::Error { why } => json!(why),
            _ => answer
                .runtime
                .state
                .get("why")
                .cloned()
                .unwrap_or(Value::Null),
        };
        log.write(
            "run.ended",
            who.line(json!({ "status": steps::status(&answer.runtime), "why": why })),
        );
    }
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
/// that cannot be environment variables, and state values past `ENV_MAX`,
/// are on stdin only; a bigger value from the data or args is an error.
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
    to_env(values, state)
}

/// A connector's args (`connector.<name>.args`), each one an environment
/// variable: a name no environment variable can have is an error, and so
/// is a `"$name"`, as a connector has no run state.
pub fn connector_args(ws: &Workspace, connector: &str) -> Result<Map<String, Value>, String> {
    let args = ws
        .connector_args(connector)
        .map_err(|e| format!("{e:#}"))?
        .unwrap_or_default();
    for (k, v) in &args {
        let at = format!("connector.{connector}.args.{k}");
        if !env_name(k) {
            return Err(format!("{at}: not a name an environment variable can have"));
        }
        if flow::state_ref(v).is_some() {
            return Err(format!("{at}: a connector has no run state for {v}"));
        }
    }
    Ok(args)
}

/// A connector's environment: its args, `$$NAME` the secret's (or the
/// environment variable's).
pub fn connector_env(ws: &Workspace, connector: &str) -> Result<BTreeMap<String, String>, String> {
    let none = Map::new();
    let mut values = Map::new();
    for (k, v) in connector_args(ws, connector)? {
        let v =
            resolve(&v, &none).map_err(|why| format!("connector.{connector}.args.{k}: {why}"))?;
        values.insert(k, v);
    }
    to_env(values, &none)
}

/// Values as environment variables, as text. A name that cannot be one,
/// and a `null`, are left out; a value past `ENV_MAX` is an error, unless
/// it is the run's state's own (on stdin only).
fn to_env(
    values: Map<String, Value>,
    state: &Map<String, Value>,
) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    for (k, v) in values {
        if !env_name(&k) {
            continue;
        }
        let text = match &v {
            Value::Null => continue,
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        // the system refuses to start a program with a value this big
        if text.len() > ENV_MAX {
            if state.get(&k) == Some(&v) {
                continue;
            }
            return Err(format!(
                "{k} is {} bytes, too big for an environment variable: only the run's state goes on stdin",
                text.len()
            ));
        }
        env.insert(k, text);
    }
    Ok(env)
}

/// The biggest value given as an environment variable; a bigger one from
/// the state is on stdin only. Linux takes 128 KB for one.
const ENV_MAX: usize = 64 * 1024;

/// What a worker is given for an `action.requested` event: the action's
/// file, its values bound, the run's state.
fn step_run(ws: &Workspace, j: &Journal, data: &Value, now: &str) -> Result<StepRun, String> {
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
        Err(_) if worker::BUILT_IN.contains(&job.action.as_str()) && job.version.is_none() => {
            PathBuf::from(&job.action)
        }
        Err(e) => return Err(format!("#{}: {e:#}", job.action)),
    };
    let args = ws.args(&job.action).map_err(|e| format!("{e:#}"))?;
    Ok(StepRun {
        file,
        root: ws.root.clone(),
        workflow: workflow.into(),
        env: bind(&run.runtime.state, args.as_ref(), &job)?,
        state: run.runtime.state.clone(),
        memory: ws.root.join(".itero").join("memory").join(workflow),
        waiting_since: job.waiting_since.clone(),
        quiet: ws.limits.quiet.as_secs(),
        most: ws.limits.most.as_secs(),
        now: now.into(),
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
/// daemon's own folder first on its PATH. Quiet past its `quiet`, or
/// running past its `most`, the group is killed and the job is lost: it
/// runs again.
fn spawn_worker(exe: &Path, step: &StepRun) -> Result<Outcome, Failure> {
    let mut c = Command::new(exe);
    c.arg("worker")
        .env("PATH", crate::connectors::supervise::path_with(exe))
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
    // the worker's blank lines say the action printed: each is the latest
    // sign of life; its answer is one line of JSON, kept without them
    let started = std::time::Instant::now();
    let heard = std::sync::Arc::new(Mutex::new(started));
    let out = {
        let heard = heard.clone();
        let stdout = child.stdout.take();
        std::thread::spawn(move || {
            let mut buf = worker::read_telling(stdout, &|| {
                *heard.lock().unwrap_or_else(PoisonError::into_inner) = std::time::Instant::now();
            })
            .unwrap_or_default();
            buf.retain(|&b| b != b'\n');
            buf
        })
    };
    let err = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    );
    // when the group is killed, the readers end as its pipes close; they
    // are not waited for, in case something outside the group holds them
    let quiet = std::time::Duration::from_secs(step.quiet);
    let most = std::time::Duration::from_secs(step.most);
    let status = loop {
        let silent = heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elapsed();
        let ran = started.elapsed();
        let why = if ran >= most {
            Some(format!("ran for {}", humantime::format_duration(most)))
        } else if silent >= quiet {
            Some(format!("quiet for {}", humantime::format_duration(quiet)))
        } else {
            None
        };
        if let Some(why) = why {
            // still running, not yet reaped: its group's id is still its own
            stop(&mut child);
            return Err(Failure::Lost(why));
        }
        let next = (quiet - silent).min(most - ran);
        if let Some(status) = child.wait_timeout(next).map_err(|e| e.to_string())? {
            break status;
        }
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
    let step = format!("#{}", data["job"]["action"].as_str().unwrap_or("?"));
    let (kind, result) = match outcome {
        Ok(Outcome::Done(m)) => (
            "action.finished",
            json!({"path": path, "step": step, "output": m}),
        ),
        Ok(Outcome::NotYet(m)) => (
            "action.waiting",
            json!({"path": path, "step": step, "output": m}),
        ),
        Err(why) => (
            "action.failed",
            json!({"path": path, "step": step, "error": why}),
        ),
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
