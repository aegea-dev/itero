//! Runs one source for the daemon: starts it, turns each line it prints
//! into a message for the workspace, and starts it again when it ends,
//! waiting longer each time; one that keeps ending is given up on, and the
//! log and `/health` say so.
use crate::engine::daemon::{Health, SourceHealth, stamp};
use crate::engine::journal::{self, Envelope, Journal};
use crate::engine::log::Log;
use crate::workspace::Workspace;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// When a source is started again, and when it is given up on.
pub struct Policy {
    /// The first wait before starting it again; it doubles each time.
    pub first_pause: Duration,
    pub max_pause: Duration,
    /// A run this long sets the wait back to `first_pause`.
    pub calm_after: Duration,
    /// Started again at most this many times within `window`; the next end
    /// gives it up.
    pub give_up_after: usize,
    pub window: Duration,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            first_pause: Duration::from_secs(1),
            max_pause: Duration::from_secs(60),
            calm_after: Duration::from_secs(300),
            give_up_after: 10,
            window: Duration::from_secs(600),
        }
    }
}

/// A source of the workspace: the type of the messages it sends (its file
/// name), and its file.
pub struct Source<'a> {
    pub kind: &'a str,
    pub file: &'a Path,
}

/// One line a source printed: the data of a message, a JSON object.
pub fn parse_line(line: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(line) {
        Ok(Value::Object(o)) => Ok(o),
        _ => Err(format!("not a JSON object: {line}")),
    }
}

fn set(health: &Mutex<Health>, name: &str, f: impl FnOnce(&mut SourceHealth)) {
    let mut h = health.lock().unwrap_or_else(PoisonError::into_inner);
    let s = h.sources.entry(name.to_string()).or_insert(SourceHealth {
        state: "running".into(),
        restarts: 0,
        last_exit: None,
    });
    f(s);
}

/// The PATH of what the daemon runs: the daemon's own folder first, so
/// `itero` is the daemon's own.
pub fn path_with(exe: &Path) -> std::ffi::OsString {
    let mut dirs: Vec<std::path::PathBuf> =
        exe.parent().into_iter().map(Path::to_path_buf).collect();
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// One start of the source, until it ends: how it ended.
fn once(
    ws: &Workspace,
    exe: &Path,
    s: &Source,
    wake: &Sender<()>,
    log: &Log,
) -> anyhow::Result<String> {
    let j = Journal::open(ws)?;
    let mut child = Command::new(s.file)
        .current_dir(&ws.root)
        .env("PATH", path_with(exe))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    // stdin stays open while it runs: its closing tells the source that
    // the daemon has gone
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped");
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(&line) {
            Ok(data) => {
                let e = Envelope::new(
                    &format!("source/{}", s.kind),
                    s.kind,
                    &journal::subject("app"),
                    &stamp(chrono::Utc::now()),
                    None,
                    Value::Object(data),
                );
                j.publish(&e)?;
                let _ = wake.send(());
            }
            Err(why) => log.write("error", json!({ "source": s.kind, "why": why })),
        }
    }
    drop(stdin);
    Ok(child.wait()?.to_string())
}

/// Runs a source until it is given up on.
pub fn run(
    ws: &Workspace,
    exe: &Path,
    s: &Source,
    policy: &Policy,
    wake: &Sender<()>,
    health: &Mutex<Health>,
    log: &Log,
) {
    let mut ends: Vec<Instant> = vec![];
    let mut pause = Duration::ZERO;
    loop {
        let began = Instant::now();
        set(health, s.kind, |h| {
            h.state = "running".into();
            h.restarts = ends.len();
        });
        log.write("source.started", json!({ "source": s.kind }));
        let ended = match once(ws, exe, s, wake, log) {
            Ok(status) => status,
            Err(e) => format!("error: {e:#}"),
        };
        let ran = began.elapsed();
        log.write(
            "source.ended",
            json!({ "source": s.kind, "why": ended, "seconds": ran.as_secs() }),
        );
        ends.retain(|t| t.elapsed() < policy.window);
        ends.push(Instant::now());
        let restarts = ends.len() - 1;
        if restarts >= policy.give_up_after {
            log.write(
                "source.gave-up",
                json!({
                    "source": s.kind,
                    "why": format!("started again {restarts} times in {}s", policy.window.as_secs()),
                }),
            );
            set(health, s.kind, |h| {
                h.state = "gave up".into();
                h.restarts = restarts;
                h.last_exit = Some(ended);
            });
            return;
        }
        pause = if ran >= policy.calm_after {
            policy.first_pause
        } else {
            (pause * 2).clamp(policy.first_pause, policy.max_pause)
        };
        set(health, s.kind, |h| {
            h.state = "waiting".into();
            h.restarts = restarts;
            h.last_exit = Some(ended);
        });
        std::thread::sleep(pause);
    }
}
