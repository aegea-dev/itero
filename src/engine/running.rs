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
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// How a pass is told: to the terminal, and to the workspace's log.
pub type Report = fn(&Result<Pass>, Option<&Log>);

pub struct Running {
    pub ws: Workspace,
    /// The workspace's journal name.
    pub name: String,
    pub engine: Arc<Engine>,
    pub log: Log,
    stop: Arc<Stop>,
    threads: Vec<JoinHandle<()>>,
    _lock: DaemonLock,
}

impl Running {
    /// Starts `ws`'s threads, holding its lock: a bad connector file or the
    /// alarm failing is an error, and nothing is left running.
    pub fn start(ws: Workspace, exe: &Path, lock: DaemonLock, report: Report) -> Result<Running> {
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
        {
            let (ws, exe, engine, log, stop) = (
                ws.clone(),
                exe.to_path_buf(),
                engine.clone(),
                log.clone(),
                stop.clone(),
            );
            threads.push(std::thread::spawn(move || {
                daemon::run_loop(&ws, &exe, &alarm, &woken, &engine.health, &stop, |p| {
                    report(p, Some(&log))
                });
                // the alarm's thread ends with it
                drop(alarm);
            }));
        }
        Ok(Running {
            ws,
            name: me.name,
            engine,
            log,
            stop,
            threads,
            _lock: lock,
        })
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
