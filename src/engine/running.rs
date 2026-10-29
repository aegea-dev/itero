//! One workspace's threads in a daemon: its connectors, its path watcher,
//! its alarm and its pass loop, started together and stopped together.
//! `itero daemon` in a folder runs one; `itero daemon --all` runs one per
//! workspace on the machine's list.
use super::daemon::{self, Health, Pass};
use super::http::Engine;
use super::lock::DaemonLock;
use super::log::Log;
use super::stop::Stop;
use crate::workspace::Workspace;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How a pass is told: to the terminal, and to the workspace's log.
pub type Report = fn(&Result<Pass>, Option<&Log>);

pub struct Running {
    pub ws: Workspace,
    /// The workspace's journal name.
    pub name: String,
    pub engine: Arc<Engine>,
    pub log: Log,
    /// `running`, `stuck` or `failed`.
    state: Arc<Mutex<&'static str>>,
    stop: Arc<Stop>,
    threads: Vec<JoinHandle<()>>,
    _lock: DaemonLock,
}

impl Running {
    /// Starts `ws`'s threads, holding its lock: a bad connector file or the
    /// alarm failing is an error, and nothing is left running. On the
    /// machine's daemon (`machine`) a pass past its limit is told, and a
    /// loop that panics is started again, or given up on; the other
    /// workspaces run on either way.
    pub fn start(
        ws: Workspace,
        exe: &Path,
        lock: DaemonLock,
        report: Report,
        machine: bool,
    ) -> Result<Running> {
        let me = super::journal::Journal::open(&ws)?.workspace()?;
        let log = Log::new(&ws, &me.id);
        let connectors = ws.connector_files()?;
        let d = daemon::Daemon::current();
        log.write(
            "daemon.started",
            json!({
                "workspace_name": me.name,
                "workspace_created": me.created,
                "workspace_itero": me.itero,
                "path": ws.root.display().to_string(),
                "meta": ws.meta,
                "daemon_name": d.name,
                "started": d.started,
                "pid": d.pid,
                "host": d.host,
                "itero": d.itero,
            }),
        );
        // a $$NAME both a secret and a variable: said once, at the start
        for name in super::secrets::shadowed(&ws)? {
            let why = super::secrets::shadow_warning(&name);
            eprintln!("itero: warning: {why}");
            log.write("warning", json!({ "why": why }));
        }
        super::journal::Journal::open(&ws)?.publish(&super::events::own(
            "itero.daemon.started",
            json!({ "daemon": d.id, "name": d.name, "itero": d.itero }),
            &d.started,
        ))?;
        let (wake, woken) = std::sync::mpsc::channel();
        let engine = Arc::new(Engine {
            health: Mutex::new(Health::starting(&daemon::stamp(chrono::Utc::now()))),
            wake,
        });
        let stop = Arc::new(Stop::default());
        let state = Arc::new(Mutex::new("running"));
        // the next due time, kept by the kernel; it wakes the loop as
        // everything else does
        let alarm = {
            let log = log.clone();
            super::alarm::Alarm::start(engine.wake.clone(), move |e| {
                log.write("error", json!({ "why": format!("the alarm stopped: {e}") }));
            })?
        };
        let mut threads = vec![];
        for (kind, file) in connectors {
            let (ws, exe, engine, log, stop) = (
                ws.clone(),
                exe.to_path_buf(),
                engine.clone(),
                log.clone(),
                stop.clone(),
            );
            let wake = engine.wake.clone();
            threads.push(std::thread::spawn(move || {
                let connector = crate::connectors::supervise::Connector {
                    kind: &kind,
                    file: &file,
                };
                let policy = crate::connectors::supervise::Policy::default();
                crate::connectors::supervise::run(
                    &ws,
                    &exe,
                    &connector,
                    &policy,
                    &wake,
                    &engine.health,
                    &log,
                    &stop,
                );
            }));
        }
        {
            // one thread watches every path the workflows name
            let (ws, wake, log, stop) =
                (ws.clone(), engine.wake.clone(), log.clone(), stop.clone());
            threads.push(std::thread::spawn(move || {
                daemon::watch_paths(&ws, &wake, &log, &stop)
            }));
        }
        // a pass's start and end, for the watch on passes past their limit
        let (beats, beaten) = std::sync::mpsc::channel::<Option<Instant>>();
        if machine {
            let (name, state, stop) = (me.name.clone(), state.clone(), stop.clone());
            threads.push(std::thread::spawn(move || {
                watch_passes(&name, &beaten, stuck_after(), &state, &stop)
            }));
        }
        {
            let (ws, exe, engine, log, stop, state, name) = (
                ws.clone(),
                exe.to_path_buf(),
                engine.clone(),
                log.clone(),
                stop.clone(),
                state.clone(),
                me.name.clone(),
            );
            threads.push(std::thread::spawn(move || {
                let beat = |busy: bool| {
                    let _ = beats.send(busy.then(Instant::now));
                };
                let policy = crate::connectors::supervise::Policy::default();
                let (mut ends, mut pause) = (vec![], Duration::ZERO);
                loop {
                    let began = Instant::now();
                    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        daemon::run_loop(
                            &ws,
                            &exe,
                            &alarm,
                            &woken,
                            &engine.health,
                            &stop,
                            &beat,
                            |p| report(p, Some(&log)),
                        )
                    }));
                    let Err(panic) = ran else { break };
                    // a panic ends the pass under way
                    let _ = beats.send(None);
                    let why = panic_message(&panic);
                    log.write(
                        "error",
                        json!({ "why": format!("the pass loop panicked: {why}") }),
                    );
                    if !machine {
                        std::panic::resume_unwind(panic);
                    }
                    match policy.after_end(&mut ends, Instant::now(), began.elapsed(), pause) {
                        Some(next) if !stop.sleep(next) => pause = next,
                        Some(_) => break,
                        None => {
                            *state.lock().unwrap_or_else(PoisonError::into_inner) = "failed";
                            eprintln!("itero: {name} failed: {why}");
                            super::machine::tell_all(
                                super::events::WORKSPACE_FAILED,
                                json!({ "workspace": name, "why": why }),
                            );
                            // its connectors and watcher end too
                            stop.set();
                            let _ = super::events::nudge(&ws, &daemon::stamp(chrono::Utc::now()));
                            break;
                        }
                    }
                }
                // the alarm's thread ends with it
                drop(alarm);
            }));
        }
        Ok(Running {
            ws,
            name: me.name,
            engine,
            log,
            state,
            stop,
            threads,
            _lock: lock,
        })
    }

    /// `running`, `stuck` (a pass past its limit) or `failed` (given up on).
    pub fn state(&self) -> &'static str {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The processes its connectors run now.
    pub fn connector_pids(&self) -> Vec<u32> {
        self.stop.pids()
    }

    /// Stops its threads and waits for them: the loop ends after the pass
    /// under way, the connectors' processes are asked to end, the watcher
    /// ends on the wake file it is touched; then the lock is free.
    pub fn stop(self) {
        self.stop.set();
        let _ = self.engine.wake.send(());
        if let Err(e) = super::events::nudge(&self.ws, &daemon::stamp(chrono::Utc::now())) {
            self.log
                .write("error", json!({ "why": format!("stopping: {e}") }));
        }
        for t in self.threads {
            let _ = t.join();
        }
        self.log.write("daemon.stopped", json!({}));
    }
}

/// The health of a running workspace, for the API.
pub fn health(r: &Running) -> Health {
    r.engine
        .health
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// How long a pass may run before its workspace is stuck: the watchdog's
/// limit, or `ITERO_STUCK_AFTER` seconds (for tests).
fn stuck_after() -> Duration {
    std::env::var("ITERO_STUCK_AFTER")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(super::watchdog::Rules::default().stale_after)
}

/// Watches a workspace's passes, told their starts and ends: one past
/// `limit` makes it stuck, told to every workspace; its end makes it
/// running again. It waits on the pass's own deadline, never polls.
fn watch_passes(
    name: &str,
    beaten: &Receiver<Option<Instant>>,
    limit: Duration,
    state: &Mutex<&'static str>,
    stop: &Stop,
) {
    let set = |s: &'static str| *state.lock().unwrap_or_else(PoisonError::into_inner) = s;
    let mut busy: Option<Instant> = None;
    let mut stuck = false;
    loop {
        let got = match busy {
            Some(began) if !stuck => beaten.recv_timeout(limit.saturating_sub(began.elapsed())),
            _ => beaten.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        if stop.is_set() {
            return;
        }
        match got {
            Ok(Some(began)) => busy = Some(began),
            Ok(None) => {
                busy = None;
                if stuck {
                    stuck = false;
                    set("running");
                    eprintln!("itero: {name} runs again");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                stuck = true;
                set("stuck");
                let since = busy.map(|b| b.elapsed().as_secs()).unwrap_or_default();
                eprintln!("itero: {name} is stuck: a pass running for {since}s");
                let at = chrono::Utc::now() - chrono::Duration::seconds(since as i64);
                super::machine::tell_all(
                    super::events::WORKSPACE_STUCK,
                    json!({ "workspace": name, "since": daemon::stamp(at) }),
                );
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// What a panic said.
fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".into())
}
