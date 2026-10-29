//! `itero daemon --all`: one daemon running every workspace on the
//! machine's list, each on threads of its own (`Running`), following the
//! list as it changes.
use super::journal::Journal;
use super::lock;
use super::running::{Report, Running};
use crate::machine::{Entry, List, list_path};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Why a listed workspace does not run: its state and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Idle {
    pub state: &'static str,
    pub why: String,
}

/// The file in itero's state folder a worker touches when a step names a
/// model the machine's list of models has not: the daemon scans again.
pub const MODELS_WANTED: &str = "models.wanted";

pub struct Machine {
    /// Asks for a scan for models (`start_scans`).
    scans: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    exe: PathBuf,
    report: Report,
    /// The workspaces running, by journal id.
    running: Mutex<BTreeMap<String, Running>>,
    /// The listed ones not running, by journal id.
    idle: Mutex<BTreeMap<String, (Entry, Idle)>>,
    /// One sync at a time.
    syncing: Mutex<()>,
}

impl Machine {
    pub fn new(exe: PathBuf, report: Report) -> Machine {
        Machine {
            scans: Mutex::new(None),
            exe,
            report,
            running: Mutex::new(BTreeMap::new()),
            idle: Mutex::new(BTreeMap::new()),
            syncing: Mutex::new(()),
        }
    }

    /// Reads the list, and starts or stops workspaces to match it.
    pub fn sync(&self) -> Result<()> {
        let _one = self.syncing.lock().unwrap_or_else(PoisonError::into_inner);
        let list = List::read(&list_path()?)?;
        // off the list, or moved: stopped
        let gone: Vec<String> = {
            let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            running
                .iter()
                .filter(|(id, r)| list.by_id(id).is_none_or(|e| e.folder != r.ws.root))
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in gone {
            let r = self
                .running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            if let Some(r) = r {
                eprintln!("itero: stopping {}", r.name);
                r.stop();
            }
        }
        let mut idle = BTreeMap::new();
        for e in &list.workspaces {
            if self
                .running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains_key(&e.id)
            {
                continue;
            }
            match self.start(e) {
                Ok(r) => {
                    eprintln!("itero: running {} ({})", e.name, e.folder.display());
                    self.running
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(e.id.clone(), r);
                }
                Err(i) => {
                    eprintln!("itero: {} is {}: {}", e.name, i.state, i.why);
                    idle.insert(e.id.clone(), (e.clone(), i));
                }
            }
        }
        *self.idle.lock().unwrap_or_else(PoisonError::into_inner) = idle;
        Ok(())
    }

    /// Starts one listed workspace, or says why not.
    fn start(&self, e: &Entry) -> Result<Running, Idle> {
        let missing = |why: String| Idle {
            state: "missing",
            why,
        };
        let ws = crate::workspace::find(&e.folder)
            .ok()
            .filter(|ws| ws.root == e.folder)
            .ok_or_else(|| missing(format!("no workspace in {}", e.folder.display())))?;
        if !ws.journal().is_file() {
            return Err(missing(format!("no journal in {}", e.folder.display())));
        }
        let id = Journal::open(&ws)
            .and_then(|j| j.workspace())
            .map_err(|err| missing(format!("{err:#}")))?
            .id;
        if id != e.id {
            return Err(missing(format!(
                "{} holds another workspace now",
                e.folder.display()
            )));
        }
        let d = super::daemon::Daemon::current();
        let who = format!("{}, pid {}", d.name, d.pid);
        // a lock just freed may be held a moment longer by a process
        // started meanwhile, until it runs its own program
        let mut tries = 0;
        let held = loop {
            match lock::take(&ws, &who) {
                Ok(l) => break l,
                Err(_) if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(why) => {
                    return Err(Idle {
                        state: "stopped",
                        why,
                    });
                }
            }
        };
        Running::start(ws, &self.exe, held, self.report, true).map_err(|err| Idle {
            state: "failed",
            why: format!("{err:#}"),
        })
    }

    /// Each listed workspace: its name, folder, state and why.
    pub fn states(&self) -> Vec<Value> {
        let mut out: Vec<Value> = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|r| {
                json!({ "name": r.name, "folder": r.ws.root.display().to_string(),
                        "state": r.state(), "why": null })
            })
            .collect();
        out.extend(
            self.idle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .map(|(e, i)| {
                    json!({ "name": e.name, "folder": e.folder.display().to_string(),
                            "state": i.state, "why": i.why })
                }),
        );
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    /// A running workspace by its name: what its routes need.
    pub fn find(
        &self,
        name: &str,
    ) -> Option<(
        crate::workspace::Workspace,
        std::sync::Arc<super::http::Engine>,
    )> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|r| r.name == name)
            .map(|r| (r.ws.clone(), r.engine.clone()))
    }

    /// Each running workspace's health, by name.
    pub fn health(&self) -> Value {
        let mut out = serde_json::Map::new();
        self.each(|r| {
            let h = super::running::health(r);
            out.insert(
                r.name.clone(),
                json!({ "passes": h.passes, "last_pass": h.last_pass,
                        "last_error": h.last_error, "busy_since": h.busy_since,
                        "connectors": h.connectors }),
            );
        });
        Value::Object(out)
    }

    /// The machine's one scan for models, in place of each workspace's
    /// `models` workflow: now, at the top of every hour on an alarm of its
    /// own, and when a step names a model not on the list (`scan_now`).
    pub fn start_scans(&self) -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let alarm = super::alarm::Alarm::start(tx.clone(), |e| {
            eprintln!("itero: the alarm for scanning models stopped: {e}");
        })?;
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                // asked several times meanwhile: one scan
                while rx.try_recv().is_ok() {}
                match crate::ai::find::scan_on(
                    crate::ai::machine_socket().with_extension("find.sock"),
                ) {
                    Ok(v) => eprintln!("itero: models: {v}"),
                    Err(e) => eprintln!("itero: looking for models: {e}"),
                }
                let now = chrono::Utc::now();
                let hour = chrono::Duration::hours(1);
                let next = chrono::DurationRound::duration_trunc(now, hour).map(|h| h + hour);
                if let Err(e) = alarm.set(next.ok()) {
                    eprintln!("itero: setting the next scan for models: {e}");
                }
            }
        });
        let _ = tx.send(());
        *self.scans.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        Ok(())
    }

    /// Asks for a scan for models now.
    pub fn scan_now(&self) {
        if let Some(tx) = self
            .scans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = tx.send(());
        }
    }

    /// Calls `f` with each running workspace.
    pub fn each(&self, mut f: impl FnMut(&Running)) {
        for r in self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
        {
            f(r);
        }
    }

    /// Syncs at once, then each time the list changes, for good. Its
    /// folder is watched (the list is written by a rename over it), and
    /// `MODELS_WANTED` by its own path: a folder's change names the
    /// folder, not the file.
    pub fn follow(&self) -> Result<()> {
        use crate::watch::Debouncer;
        let dir = crate::machine::state_dir()?;
        std::fs::create_dir_all(&dir)?;
        let mut backend = crate::watch::open_many(&[dir.clone(), dir.join(MODELS_WANTED)])?;
        let mut d = Debouncer::new(Duration::from_millis(200), Duration::from_secs(2));
        self.sync()?;
        loop {
            let timeout = d
                .due()
                .map(|at| at.saturating_duration_since(Instant::now()));
            let changes = backend.wait(timeout)?;
            d.add(changes, Instant::now());
            let (wanted, other): (Vec<_>, Vec<_>) = d
                .take(Instant::now())
                .into_iter()
                .partition(|c| c.path.file_name().is_some_and(|n| n == MODELS_WANTED));
            if !wanted.is_empty() {
                self.scan_now();
            }
            if !other.is_empty()
                && let Err(e) = self.sync()
            {
                eprintln!("itero: reading the machine's list: {e:#}");
            }
        }
    }
}

/// Tells every workspace on the machine's list one of the daemon's own
/// events, and wakes each one's loop: what happens to one workspace (stuck,
/// failed) is something the others may act on.
pub fn tell_all(kind: &str, data: Value) {
    let now = super::daemon::stamp(chrono::Utc::now());
    let Ok(list) = list_path().and_then(|p| List::read(&p)) else {
        return;
    };
    for e in &list.workspaces {
        let Some(ws) = crate::workspace::find(&e.folder)
            .ok()
            .filter(|ws| ws.root == e.folder && ws.journal().is_file())
        else {
            continue;
        };
        let told = Journal::open(&ws)
            .and_then(|j| j.publish(&super::events::own(kind, data.clone(), &now)));
        match told {
            Ok(()) => {
                let _ = super::events::nudge(&ws, &now);
            }
            Err(err) => eprintln!("itero: telling {} {kind}: {err:#}", e.name),
        }
    }
}
