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

pub struct Machine {
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
        Running::start(ws, &self.exe, held, self.report).map_err(|err| Idle {
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
                        "state": "running", "why": null })
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
    /// folder is watched: the list is written by a rename over it.
    pub fn follow(&self) -> Result<()> {
        use crate::watch::Debouncer;
        let dir = crate::machine::state_dir()?;
        std::fs::create_dir_all(&dir)?;
        let mut backend = crate::watch::open(&dir)?;
        let mut d = Debouncer::new(Duration::from_millis(200), Duration::from_secs(2));
        self.sync()?;
        loop {
            let timeout = d
                .due()
                .map(|at| at.saturating_duration_since(Instant::now()));
            let changes = backend.wait(timeout)?;
            d.add(changes, Instant::now());
            if !d.take(Instant::now()).is_empty()
                && let Err(e) = self.sync()
            {
                eprintln!("itero: reading the machine's list: {e:#}");
            }
        }
    }
}
