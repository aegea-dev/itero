//! The daemon's Chromes: started when a `#browser` step needs one, a
//! window for each run (its page always in front, so drawn and clickable),
//! closed when the run ends; a Chrome with no run's tab for
//! `IDLE` exits. At most two, one windowed and one headless, as one
//! Chrome is either. One that ends badly is started again only after a
//! pause that doubles each time, and given up on after too many, as
//! itero-ai is. What happens to them is told as `itero.browser.*` events.
use super::cdp::{self, Tab};
use super::{Mode, chrome};
use crate::connectors::supervise::Policy;
use crate::engine::events;
use crate::workspace::Workspace;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Child;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How long a Chrome with no run's tab stays up.
pub const IDLE: Duration = Duration::from_secs(300);

/// How often the keeper looks at its Chromes and its runs.
const LOOK: Duration = Duration::from_secs(1);

struct Running {
    child: Child,
    port: u16,
    /// Since when it has had no tab.
    idle_since: Option<Instant>,
    /// The workspace it was last started or used for: who is told.
    ws: Workspace,
}

#[derive(Default)]
struct Failures {
    at: Vec<Instant>,
    pause: Duration,
    why: String,
}

/// A run's tab.
struct RunTab {
    mode: Mode,
    tab: Tab,
    /// Tabs the run had before it followed one a click opened: closed
    /// with it.
    others: Vec<Tab>,
    ws: Workspace,
}

#[derive(Default)]
struct State {
    chromes: HashMap<Mode, Running>,
    failures: HashMap<Mode, Failures>,
    runs: HashMap<String, RunTab>,
    looking: bool,
}

pub struct Keeper {
    state: Mutex<State>,
    policy: Policy,
    machine: std::sync::atomic::AtomicBool,
}

/// What a step is given: its run's tab, and the mode it runs in.
#[derive(Debug, Clone, PartialEq)]
pub struct Given {
    pub ws: String,
    pub mode: Mode,
}

impl Keeper {
    /// The daemon's one keeper.
    pub fn global() -> &'static Keeper {
        static K: OnceLock<Keeper> = OnceLock::new();
        K.get_or_init(|| Keeper {
            state: Mutex::new(State::default()),
            policy: Policy::default(),
            machine: Default::default(),
        })
    }

    /// For the machine's daemon: what happens told to every workspace.
    pub fn for_machine(&self) {
        self.machine
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The tab of run `run`, opened now in a window of its own when it has
    /// none, in the Chrome of
    /// the mode asked (`headless`), started when none runs. A run's tab
    /// stays in the mode its first step chose.
    pub fn tab_for_run(
        &'static self,
        ws: &Workspace,
        run: &str,
        headless: Option<bool>,
    ) -> Result<Given, String> {
        let mut s = self.lock();
        if let Some(t) = s.runs.get_mut(run) {
            // a step that followed a tab its click opened left it here
            if let Some(next) = take_followed(run) {
                t.others.push(std::mem::replace(&mut t.tab, next));
            }
            let asked = headless.map(|h| chrome::choose(Some(h)));
            if let Some(m) = asked
                && m != t.mode
            {
                return Err(format!(
                    "this run's browser is {}; a step of it cannot be {}",
                    t.mode.name(),
                    m.name()
                ));
            }
            return Ok(Given {
                ws: t.tab.ws.clone(),
                mode: t.mode,
            });
        }
        let mode = chrome::choose(headless);
        let port = self.chrome(&mut s, ws, mode)?;
        let tab = cdp::new_window(port, "about:blank")?;
        let given = Given {
            ws: tab.ws.clone(),
            mode,
        };
        s.runs.insert(
            run.into(),
            RunTab {
                mode,
                tab,
                others: vec![],
                ws: ws.clone(),
            },
        );
        if let Some(c) = s.chromes.get_mut(&mode) {
            c.idle_since = None;
        }
        if !s.looking {
            s.looking = true;
            std::thread::spawn(|| Keeper::global().look());
        }
        Ok(given)
    }

    /// The port of `mode`'s Chrome, started now when none runs.
    fn chrome(&self, s: &mut State, ws: &Workspace, mode: Mode) -> Result<u16, String> {
        if let Some(c) = s.chromes.get_mut(&mode) {
            c.ws = ws.clone();
            return Ok(c.port);
        }
        let now = Instant::now();
        let f = s.failures.entry(mode).or_default();
        f.at.retain(|t| now.duration_since(*t) < self.policy.window);
        if f.at.len() >= self.policy.give_up_after {
            return Err(format!(
                "Chrome ended badly {} times in {} minutes, last: {}",
                f.at.len(),
                self.policy.window.as_secs() / 60,
                f.why
            ));
        }
        if let Some(last) = f.at.last()
            && now.duration_since(*last) < f.pause
        {
            let left = f.pause - now.duration_since(*last);
            return Err(format!(
                "Chrome ended badly ({}); it starts again in {} s",
                f.why,
                left.as_secs().max(1)
            ));
        }
        let started = chrome::find()
            .and_then(|exe| {
                let profile = super::profile_dir(mode).map_err(|e| format!("{e:#}"))?;
                chrome::launch(&exe, mode, &profile)
            })
            .inspect_err(|why| {
                self.tell(
                    ws,
                    events::BROWSER_START_FAILED,
                    json!({ "mode": mode.name(), "why": why }),
                );
            })?;
        let port = started.port;
        s.chromes.insert(
            mode,
            Running {
                child: started.child,
                port,
                idle_since: None,
                ws: ws.clone(),
            },
        );
        self.tell(
            ws,
            events::BROWSER_STARTED,
            json!({ "mode": mode.name(), "port": port }),
        );
        Ok(port)
    }

    /// Closes run `run`'s tab, when it has one.
    pub fn end_run(&self, run: &str) {
        let mut s = self.lock();
        let Some(mut t) = s.runs.remove(run) else {
            return;
        };
        if let Some(last) = take_followed(run) {
            t.others.push(last);
        }
        let has_tabs = s.runs.values().any(|r| r.mode == t.mode);
        if let Some(c) = s.chromes.get_mut(&t.mode) {
            for tab in t.others.iter().chain([&t.tab]) {
                if let Err(e) = cdp::close_tab(c.port, &tab.id) {
                    eprintln!("itero: the browser tab of run {run}: {e}");
                }
            }
            if !has_tabs {
                c.idle_since = Some(Instant::now());
            }
        }
    }

    /// Every `LOOK`: a Chrome that ended is told and its runs' tabs
    /// forgotten; a run that ended without the daemon closing its tab has
    /// it closed; a Chrome idle for `IDLE` is stopped.
    fn look(&'static self) {
        loop {
            std::thread::sleep(LOOK);
            let ended: Vec<String> = {
                let s = self.lock();
                s.runs
                    .iter()
                    .filter(|(id, t)| run_ended(&t.ws, id))
                    .map(|(id, _)| id.clone())
                    .collect()
            };
            for id in ended {
                self.end_run(&id);
            }
            let mut s = self.lock();
            let idle = idle_after();
            let modes: Vec<Mode> = s.chromes.keys().copied().collect();
            for mode in modes {
                let c = s.chromes.get_mut(&mode).expect("listed");
                match c.child.try_wait() {
                    Ok(None) => {
                        if c.idle_since.is_some_and(|t| t.elapsed() >= idle) {
                            let mut c = s.chromes.remove(&mode).expect("listed");
                            stop_child(&mut c.child);
                            self.tell(
                                &c.ws,
                                events::BROWSER_STOPPED,
                                json!({ "mode": mode.name(), "after": idle.as_secs() }),
                            );
                        }
                    }
                    other => {
                        let exit = match other {
                            Ok(Some(st)) => st.to_string(),
                            Err(e) => e.to_string(),
                            Ok(None) => unreachable!(),
                        };
                        let c = s.chromes.remove(&mode).expect("listed");
                        s.runs.retain(|_, r| r.mode != mode);
                        let f = s.failures.entry(mode).or_default();
                        f.pause =
                            (f.pause * 2).clamp(self.policy.first_pause, self.policy.max_pause);
                        f.at.push(Instant::now());
                        f.why = format!("it ended: {exit}");
                        self.tell(
                            &c.ws,
                            events::BROWSER_FAILED,
                            json!({ "mode": mode.name(), "exit": exit }),
                        );
                    }
                }
            }
        }
    }

    /// The port of `mode`'s Chrome, while one runs.
    pub fn port(&self, mode: Mode) -> Option<u16> {
        self.lock().chromes.get(&mode).map(|c| c.port)
    }

    /// How the Chromes are, for the daemon's `/health`; nothing before
    /// one was needed.
    pub fn health(&self) -> Option<Value> {
        let s = self.lock();
        if s.chromes.is_empty() && s.failures.is_empty() {
            return None;
        }
        let state = |m: Mode| {
            if s.chromes.contains_key(&m) {
                "running"
            } else {
                "stopped"
            }
        };
        Some(json!({
            "window": state(Mode::Window),
            "headless": state(Mode::Headless),
            "tabs": s.runs.len(),
            "failures": s.failures.values().map(|f| f.at.len()).sum::<usize>(),
        }))
    }

    /// Stops the Chromes this daemon started, when the daemon stops.
    pub fn stop(&self) {
        let mut s = self.lock();
        s.runs.clear();
        for (_, mut c) in s.chromes.drain() {
            stop_child(&mut c.child);
        }
    }

    fn tell(&self, ws: &Workspace, kind: &str, data: Value) {
        if self.machine.load(std::sync::atomic::Ordering::SeqCst) {
            crate::engine::machine::tell_all(kind, data);
        } else {
            crate::ai::keeper::publish(ws, kind, data);
        }
    }
}

/// Where a step leaves the tab its run follows now: the step runs in a
/// worker, apart from the daemon that keeps the tabs.
pub fn followed_file(run: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::machine::state_dir()?
        .join("browser")
        .join("followed")
        .join(run))
}

/// The tab a step of run `run` followed, taken.
fn take_followed(run: &str) -> Option<Tab> {
    let path = followed_file(run).ok()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(Tab {
        id: v["id"].as_str()?.into(),
        ws: v["ws"].as_str()?.into(),
    })
}

/// `IDLE`, or for a test `ITERO_TEST_BROWSER_IDLE_SECS`.
fn idle_after() -> Duration {
    std::env::var("ITERO_TEST_BROWSER_IDLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE)
}

/// Whether run `id` has ended, or is gone.
fn run_ended(ws: &Workspace, id: &str) -> bool {
    match crate::engine::journal::Journal::open(ws).and_then(|j| j.run(id)) {
        Ok(Some(r)) => r.runtime.ended.is_some(),
        Ok(None) => true,
        // a journal that cannot be read now says nothing about the run
        Err(_) => false,
    }
}

/// Asks Chrome to end, so it closes its profile cleanly; after two
/// seconds, ends it.
fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: kill only sends a signal, to the Chrome this daemon started
    // and has not reaped
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}
