//! The watchdog: keeps `itero daemon` running. It starts the daemon, asks
//! its `/health` every few seconds, and restarts it when it has exited,
//! stops answering, or one pass runs past its limit (a daemon waiting for
//! something to do makes no passes, and is fine). The decision (`decide`) is a pure
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
    /// How long one pass may run before the daemon counts as hung.
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
    Health { busy_since: Option<DateTime<Utc>> },
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
        Probe::Health { busy_since } => {
            w.missed = 0;
            match busy_since {
                Some(began) if since(*began) > rules.stale_after => {
                    Verdict::Restart(format!("a pass running for {}s", secs(now - *began)))
                }
                _ => Verdict::Wait,
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

/// The itero a watchdog was started as: the path it was started by, the
/// file that path leads to, and that file's device, inode and modification
/// time. An upgrade changes it: Homebrew moves the link to a new folder,
/// an install renames a new file over the old one.
#[derive(Debug, Clone, PartialEq)]
pub struct Binary {
    pub path: PathBuf,
    pub file: PathBuf,
    pub id: (u64, u64, i64, i64),
}

impl Binary {
    /// The itero at `path` now.
    #[cfg(unix)]
    pub fn now(path: &std::path::Path) -> std::io::Result<Binary> {
        use std::os::unix::fs::MetadataExt;
        let file = std::fs::canonicalize(path)?;
        let m = std::fs::metadata(&file)?;
        Ok(Binary {
            path: path.to_path_buf(),
            file,
            id: (m.dev(), m.ino(), m.mtime(), m.mtime_nsec()),
        })
    }
}

/// Whether the itero at `old`'s path is another file now.
pub fn changed(old: &Binary, new: &Binary) -> bool {
    old.file != new.file || old.id != new.id
}

/// The daemon a watchdog keeps running.
pub enum Target {
    /// `itero daemon` in a workspace: its port is read at each check, as
    /// the OS may give it another one at a start.
    Workspace(Box<crate::workspace::Workspace>),
    /// `itero daemon --all`: every workspace on the machine's list, its
    /// settings from `config` (else the config folder's `server.json`).
    /// Its /health has no pass of its own, so a stuck workspace never
    /// restarts it: the daemon tells it.
    Machine { config: Option<PathBuf> },
}

impl Target {
    /// The daemon's command line, after the itero binary.
    pub fn args(&self) -> Vec<String> {
        match self {
            Target::Workspace(_) => vec!["daemon".into()],
            Target::Machine { config } => {
                let mut a = vec!["daemon".to_string(), "--all".into()];
                if let Some(c) = config {
                    a.extend(["--config".into(), c.display().to_string()]);
                }
                a
            }
        }
    }

    /// Where the daemon answers now.
    pub fn port(&self) -> Option<u16> {
        match self {
            Target::Workspace(ws) => ws.daemon_port(),
            // the port server.json gives, as the daemon reads it at its
            // start; a file it cannot read, the daemon refuses and says
            // why, and the watchdog asks the port it kept
            Target::Machine { config } => crate::server::Server::read(config.as_deref())
                .ok()
                .and_then(|s| s.port)
                .or_else(super::http::machine_port),
        }
    }
}

/// What the watchdog runs.
pub struct Options {
    /// The itero it runs the daemon with: the path it was started by.
    pub exe: PathBuf,
    /// That itero as the watchdog started: when the path leads to another
    /// file and it runs, the watchdog runs it in its place.
    pub binary: Binary,
    /// The watchdog's own arguments, to run the new itero with.
    pub args: Vec<std::ffi::OsString>,
    pub target: Target,
    pub rules: Rules,
    /// Where its own lines and the daemon's stderr go; stderr when none.
    pub log: Option<PathBuf>,
}

/// The version the file at `path` says, run as `itero --version` (3 s at
/// most), or why it does not run as itero.
fn version_of(path: &std::path::Path) -> std::result::Result<String, String> {
    let mut c = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    match c
        .wait_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())?
    {
        Some(s) if s.success() => {}
        Some(s) => return Err(format!("its --version ended with {s}")),
        None => {
            let _ = c.kill();
            let _ = c.wait();
            return Err("its --version did not answer in 3s".into());
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut c.stdout.take().expect("piped"), &mut out)
        .map_err(|e| e.to_string())?;
    let out = out.trim();
    out.strip_prefix("itero ")
        .map(str::to_string)
        .ok_or_else(|| format!("its --version said {out:?}, not itero's"))
}

/// The new itero's version, when the path the watchdog was started by
/// leads to another file now and it runs. Why not is said once for each
/// reason (`told` keeps the last one said).
fn upgraded(started: &Binary, told: &mut Option<String>, say: &dyn Fn(&str)) -> Option<String> {
    let mut tell = |why: String| {
        if told.as_deref() != Some(why.as_str()) {
            say(&why);
            *told = Some(why);
        }
        None
    };
    let now = match Binary::now(&started.path) {
        Ok(b) => b,
        Err(e) => {
            return tell(format!(
                "{} cannot be read: {e}; the daemon stays",
                started.path.display()
            ));
        }
    };
    if !changed(started, &now) {
        return None;
    }
    match version_of(&now.path) {
        Ok(v) => Some(v),
        Err(why) => tell(format!(
            "itero changed but {} does not run: {why}; the daemon stays",
            now.file.display()
        )),
    }
}

/// Asks the daemon's `/health`; no port yet is no answer.
fn probe(client: &reqwest::blocking::Client, port: Option<u16>) -> Probe {
    let Some(port) = port else {
        return Probe::NoAnswer;
    };
    let answer = client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json::<serde_json::Value>());
    match answer {
        Ok(v) => Probe::Health {
            busy_since: v["busy_since"].as_str().and_then(|s| s.parse().ok()),
        },
        Err(_) => Probe::NoAnswer,
    }
}

/// The daemon the watchdog runs now: 0 before the first, `STOPPED` once
/// the watchdog is stopping (no daemon is started after).
static DAEMON: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const STOPPED: u32 = u32::MAX;

/// Stops the daemon the watchdog runs, when the watchdog itself is asked
/// to stop: SIGTERM, as `itero stop` sends; killed if it has not ended in
/// 10 seconds. The service stops the whole process group, but a person may
/// stop the watchdog alone.
#[cfg(unix)]
pub fn stop_daemon() {
    use std::sync::atomic::Ordering;
    let pid = DAEMON.swap(STOPPED, Ordering::SeqCst);
    if pid == 0 || pid == STOPPED {
        return;
    }
    let pid = pid as libc::pid_t;
    // SAFETY: kill only sends a signal, to the daemon this watchdog started
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    // waitpid reaps it (it is this process's child); -1 when `run` has
    // already reaped it
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: waits on this process's own child; no status is read
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        let _ = tx.send(());
    });
    if rx.recv_timeout(Duration::from_secs(10)).is_err() {
        // SAFETY: as above
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        let _ = rx.recv_timeout(Duration::from_secs(5));
    }
}

/// Keeps the daemon running until the watchdog is stopped (the service
/// stops its whole process group, the daemon with it; a SIGTERM to the
/// watchdog alone stops the daemon through `stop_daemon`).
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
    let client = crate::net::client(Duration::from_secs(5))?;
    let mut wait = Duration::ZERO;
    // why the new itero is not run, said once
    let mut told: Option<String> = None;
    loop {
        let stderr = match &log {
            Some(f) => Stdio::from(f.try_clone()?),
            None => Stdio::inherit(),
        };
        let mut child = Command::new(&o.exe)
            .args(o.target.args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|e| anyhow::anyhow!("cannot start the daemon: {e}"))?;
        let now_running = DAEMON.swap(child.id(), std::sync::atomic::Ordering::SeqCst);
        if now_running == STOPPED {
            // stopping began while it started: it goes too
            DAEMON.store(STOPPED, std::sync::atomic::Ordering::SeqCst);
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        let begun = Instant::now();
        say(&format!("started the daemon (pid {})", child.id()));
        let mut w = Watch::new(Utc::now());
        let why = loop {
            let found = match child.wait_timeout(o.rules.check_every)? {
                Some(status) => Probe::Exited(status.to_string()),
                None => probe(&client, o.target.port()),
            };
            #[cfg(unix)]
            if let Some(v) = upgraded(&o.binary, &mut told, &say) {
                say(&format!(
                    "itero changed: {} \u{2192} {} (itero {v}): running it",
                    o.binary.file.display(),
                    std::fs::canonicalize(&o.binary.path)
                        .unwrap_or_else(|_| o.binary.path.clone())
                        .display()
                ));
                let _ = child.kill();
                let _ = child.wait();
                // the same process, the new itero: it starts a new daemon
                let e = std::os::unix::process::CommandExt::exec(
                    Command::new(&o.binary.path).args(&o.args),
                );
                return Err(anyhow::anyhow!("cannot run the new itero: {e}"));
            }
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

#[cfg(test)]
mod tests {
    use super::{Probe, probe};
    use std::io::{Read, Write};

    /// A port that answers one request with `status` and `body`.
    fn answers_once(status: &str, body: &str) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            s.write_all(reply.as_bytes()).unwrap();
        });
        port
    }

    #[test]
    fn a_check_reads_health_and_counts_anything_else_as_no_answer() {
        let client = crate::net::client(std::time::Duration::from_secs(5)).unwrap();
        // no port yet
        assert_eq!(probe(&client, None), Probe::NoAnswer);
        // idle, then a pass under way
        let p = answers_once("200 OK", r#"{"pid": 7}"#);
        assert_eq!(probe(&client, Some(p)), Probe::Health { busy_since: None });
        let p = answers_once("200 OK", r#"{"busy_since": "2026-10-30T09:15:00Z"}"#);
        assert_eq!(
            probe(&client, Some(p)),
            Probe::Health {
                busy_since: Some("2026-10-30T09:15:00Z".parse().unwrap())
            }
        );
        // an error, a body that is no JSON, nobody listening
        let p = answers_once("500 Internal Server Error", "{}");
        assert_eq!(probe(&client, Some(p)), Probe::NoAnswer);
        let p = answers_once("200 OK", "sağlık yok");
        assert_eq!(probe(&client, Some(p)), Probe::NoAnswer);
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(probe(&client, Some(closed)), Probe::NoAnswer);
    }
}
