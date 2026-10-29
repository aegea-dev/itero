//! The daemon's itero-ai: started when a step needs it, used while it
//! answers, started again after it ends. One that ends badly is started
//! again only after a pause that doubles each time, and given up on after
//! too many, as connectors are (`connectors::supervise::Policy`). What
//! happens to it is told as `itero.ai.*` events.
use crate::connectors::supervise::Policy;
use crate::engine::events;
use crate::workspace::Workspace;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How long itero-ai stays up with nothing asked: then it exits, freeing
/// the model's memory.
pub const IDLE: Duration = Duration::from_secs(300);

#[derive(Default)]
struct State {
    /// The process this daemon started, while it runs.
    pid: Option<u32>,
    socket: Option<PathBuf>,
    /// `running`, `stopped` (idle) or `failed`.
    state: Option<&'static str>,
    model: Option<String>,
    /// When it ended badly, within the policy's window.
    failures: Vec<Instant>,
    pause: Duration,
    why: String,
}

pub struct Keeper {
    state: Mutex<State>,
    policy: Policy,
    /// Run by the machine's daemon: one itero-ai for every workspace, its
    /// events told to all of them.
    machine: std::sync::atomic::AtomicBool,
}

/// How itero-ai is doing, for the daemon's `/health`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AiHealth {
    pub state: String,
    pub model: Option<String>,
    /// Bad ends within the policy's window.
    pub failures: usize,
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

    /// For the machine's daemon (`itero daemon --all`): one itero-ai on one
    /// socket for every workspace, and what happens to it told to each.
    pub fn for_machine(&self) {
        self.machine
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_machine(&self) -> bool {
        self.machine.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The socket of an itero-ai answering for `ws`, started now when none
    /// answers; an error when it ended badly too often, or too lately.
    pub fn socket(&'static self, ws: &Workspace) -> Result<PathBuf, String> {
        let mut s = self.lock();
        if let (Some(_), Some(sock)) = (s.pid, &s.socket)
            && super::answers(sock)
        {
            return Ok(sock.clone());
        }
        let now = Instant::now();
        let window = self.policy.window;
        s.failures.retain(|t| now.duration_since(*t) < window);
        if s.failures.len() >= self.policy.give_up_after {
            return Err(format!(
                "itero-ai ended badly {} times in {} minutes, last: {}",
                s.failures.len(),
                window.as_secs() / 60,
                s.why
            ));
        }
        if let Some(last) = s.failures.last()
            && now.duration_since(*last) < s.pause
        {
            let left = s.pause - now.duration_since(*last);
            return Err(format!(
                "itero-ai ended badly ({}); it starts again in {} s",
                s.why,
                left.as_secs().max(1)
            ));
        }
        let told = ws.clone();
        let socket = if self.is_machine() {
            super::machine_socket()
        } else {
            super::socket_for(&ws.root)
        };
        let started = super::ensure_on(socket, IDLE, move |line| {
            if line["event"] == "model.loaded" {
                Keeper::global().lock().model = line["model"].as_str().map(String::from);
                Keeper::global().tell(
                    &told,
                    events::AI_MODEL_LOADED,
                    json!({ "model": line["model"], "seconds": line["seconds"] }),
                );
            }
        })
        .map_err(|e| format!("{e:#}"))?;
        let socket = started.socket.clone();
        let Some(mut child) = started.child else {
            // one answering already, or a socket set by hand
            return Ok(socket);
        };
        s.pid = Some(child.id());
        s.socket = Some(socket.clone());
        s.state = Some("running");
        s.model = None;
        drop(s);
        self.tell(
            ws,
            events::AI_STARTED,
            json!({ "socket": socket.display().to_string() }),
        );
        let ws = ws.clone();
        let said = started.said;
        let policy = self.policy.clone();
        std::thread::spawn(move || {
            let ran = Instant::now();
            let status = child.wait();
            // let the readers take its last lines
            std::thread::sleep(Duration::from_millis(50));
            let why = said.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let me = Keeper::global();
            let mut s = me.lock();
            s.pid = None;
            match status {
                Ok(st) if st.success() => {
                    s.state = Some("stopped");
                    drop(s);
                    me.tell(&ws, events::AI_STOPPED, json!({ "after": IDLE.as_secs() }));
                }
                other => {
                    let exit = match &other {
                        Ok(st) => st.to_string(),
                        Err(e) => e.to_string(),
                    };
                    s.state = Some("failed");
                    s.why = why.clone();
                    s.pause = if ran.elapsed() >= policy.calm_after {
                        policy.first_pause
                    } else {
                        (s.pause * 2).clamp(policy.first_pause, policy.max_pause)
                    };
                    s.failures.push(Instant::now());
                    drop(s);
                    me.tell(&ws, events::AI_FAILED, json!({ "exit": exit, "why": why }));
                }
            }
        });
        Ok(socket)
    }

    /// How it is, once this daemon has started one.
    pub fn health(&self) -> Option<AiHealth> {
        let s = self.lock();
        s.state.map(|state| AiHealth {
            state: state.into(),
            model: s.model.clone(),
            failures: s.failures.len(),
        })
    }

    /// Stops the itero-ai this daemon started, when the daemon stops.
    pub fn stop(&self) {
        if let Some(pid) = self.lock().pid.take() {
            // SAFETY: kill only sends a signal, to the process this daemon
            // started and has not seen end
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }
}

impl Keeper {
    /// Tells what happened to itero-ai: to the workspace `ws`, or on the
    /// machine's daemon to every workspace on the list.
    fn tell(&self, ws: &Workspace, kind: &str, data: Value) {
        if self.is_machine() {
            crate::engine::machine::tell_all(kind, data);
        } else {
            publish(ws, kind, data);
        }
    }
}

/// Saves one of itero's own events for the workspace and wakes its daemon.
pub fn publish(ws: &Workspace, kind: &str, data: Value) {
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    let saved = crate::engine::journal::Journal::open(ws)
        .and_then(|j| j.publish(&events::own(kind, data, &now)));
    if let Err(e) = saved {
        eprintln!("itero: {kind} could not be saved: {e:#}");
        return;
    }
    let _ = events::nudge(ws, &now);
}
