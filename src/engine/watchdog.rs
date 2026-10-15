//! The watchdog: keeps `itero daemon` running. It starts the daemon, asks
//! its `/health` every few seconds, and restarts it when it has exited,
//! stops answering, or stops making passes. The decision (`decide`) is a pure
//! function of what the watchdog knows; `run` does the processes and HTTP.
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// When the watchdog checks, and what it puts up with.
#[derive(Debug, Clone)]
pub struct Rules {
    pub check_every: Duration,
    /// Checks in a row without an answer before a restart.
    pub misses: u32,
    /// How long a new daemon may take to answer.
    pub grace: Duration,
    /// How old the last pass may be before the daemon counts as hung.
    pub stale_after: Duration,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules {
            check_every: Duration::from_secs(10),
            misses: 3,
            grace: Duration::from_secs(30),
            stale_after: Duration::from_secs(600),
        }
    }
}

/// What one check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    /// The daemon process ended, and how.
    Exited(String),
    /// `/health` did not answer.
    NoAnswer,
    /// `/health` answered.
    Health { last_pass: Option<DateTime<Utc>> },
}

/// What the watchdog knows about the daemon it started.
#[derive(Debug, Clone, PartialEq)]
pub struct Watch {
    pub started: DateTime<Utc>,
    /// Checks in a row without an answer.
    pub missed: u32,
}

impl Watch {
    pub fn new(started: DateTime<Utc>) -> Watch {
        Watch { started, missed: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Wait,
    Restart(String),
}

fn secs(d: chrono::Duration) -> i64 {
    d.num_seconds()
}

/// Whether to restart the daemon, given one check.
pub fn decide(w: &mut Watch, probe: &Probe, now: DateTime<Utc>, rules: &Rules) -> Verdict {
    let since = |t: DateTime<Utc>| (now - t).to_std().unwrap_or_default();
    match probe {
        Probe::Exited(how) => Verdict::Restart(format!("the daemon exited ({how})")),
        Probe::NoAnswer => {
            if since(w.started) < rules.grace {
                return Verdict::Wait;
            }
            w.missed += 1;
            if w.missed >= rules.misses {
                Verdict::Restart(format!("no answer from /health {} times", w.missed))
            } else {
                Verdict::Wait
            }
        }
        Probe::Health { last_pass } => {
            w.missed = 0;
            let last = last_pass.unwrap_or(w.started);
            if since(last) > rules.stale_after {
                Verdict::Restart(format!("no pass for {}s", secs(now - last)))
            } else {
                Verdict::Wait
            }
        }
    }
}

/// The pause before the next restart: doubling from 1 s to at most 60 s,
/// and back to 1 s once a daemon has run 5 minutes.
pub fn pause(previous: Duration, ran: Duration) -> Duration {
    if ran >= Duration::from_secs(300) {
        return Duration::from_secs(1);
    }
    (previous * 2).clamp(Duration::from_secs(1), Duration::from_secs(60))
}

/// What the watchdog runs.
pub struct Options {
    /// This itero binary.
    pub exe: PathBuf,
    /// The flags `itero daemon` gets.
    pub daemon_args: Vec<String>,
    pub port: u16,
    pub rules: Rules,
    /// Where its own lines and the daemon's stderr go; stderr when none.
    pub log: Option<PathBuf>,
}

/// Asks the daemon's `/health`.
fn probe(client: &reqwest::blocking::Client, port: u16) -> Probe {
    let answer = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json::<serde_json::Value>());
    match answer {
        Ok(v) => Probe::Health {
            last_pass: v["last_pass"].as_str().and_then(|s| s.parse().ok()),
        },
        Err(_) => Probe::NoAnswer,
    }
}

/// Keeps the daemon running until the watchdog is stopped (the service
/// stops its whole process group, the daemon with it).
pub fn run(o: &Options) -> Result<()> {
    let log = match &o.log {
        Some(path) => Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| anyhow::anyhow!("cannot open {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let say = |line: &str| {
        let line = format!("{} watchdog: {line}\n", super::daemon::stamp(Utc::now()));
        match &log {
            // `Write` is implemented for `&File`: appends need no `mut` file
            Some(f) => {
                let mut f: &std::fs::File = f;
                let _ = f.write_all(line.as_bytes());
            }
            None => eprint!("{line}"),
        }
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut wait = Duration::ZERO;
    loop {
        let stderr = match &log {
            Some(f) => Stdio::from(f.try_clone()?),
            None => Stdio::inherit(),
        };
        let mut child = Command::new(&o.exe)
            .arg("daemon")
            .args(&o.daemon_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| anyhow::anyhow!("cannot start the daemon: {e}"))?;
        let begun = Instant::now();
        say(&format!("started the daemon (pid {})", child.id()));
        let mut w = Watch::new(Utc::now());
        let why = loop {
            let found = match child.wait_timeout(o.rules.check_every)? {
                Some(status) => Probe::Exited(status.to_string()),
                None => probe(&client, o.port),
            };
            if let Verdict::Restart(why) = decide(&mut w, &found, Utc::now(), &o.rules) {
                break why;
            }
        };
        say(&why);
        // a no-op when it has already exited
        let _ = child.kill();
        let _ = child.wait();
        wait = pause(wait, begun.elapsed());
        say(&format!("restarting in {}s", wait.as_secs()));
        std::thread::sleep(wait);
    }
}
