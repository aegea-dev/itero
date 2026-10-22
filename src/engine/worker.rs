//! The worker: runs one step and reports how it went. It is dumb: the
//! action's file with its values, or the built-in `#notify`; nothing else.
use serde_json::{Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Why a step gave no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// It ran and failed, or could not run: an error.
    Error(String),
    /// It ended without an answer (killed by a signal, or past its time):
    /// it runs again.
    Lost(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Failure::Error(why) | Failure::Lost(why) => f.write_str(why),
        }
    }
}

impl From<String> for Failure {
    fn from(why: String) -> Failure {
        Failure::Error(why)
    }
}

/// The exit code of a worker whose job was lost, so the daemon runs it
/// again (EX_TEMPFAIL).
pub const LOST_EXIT: i32 = 75;

/// Stops this worker's process group, its job with it, once the process
/// that started it has gone: a daemon that stopped leaves no job running
/// that its next start would run again.
#[cfg(unix)]
pub fn stop_with_parent() {
    // SAFETY: getppid, getpid and getpgrp only read this process's ids
    let parent = unsafe { libc::getppid() };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if unsafe { libc::getppid() } != parent {
                // SAFETY: signals only this worker's own group when it leads
                // one (the daemon starts it so), else only itself
                unsafe {
                    if libc::getpgrp() == libc::getpid() {
                        libc::killpg(libc::getpgrp(), libc::SIGKILL);
                    }
                    libc::kill(libc::getpid(), libc::SIGKILL);
                }
            }
        }
    });
}

/// `Itero.app`'s notifier on macOS: `ITERO_NOTIFIER` (the app or its
/// program), else beside the itero binary, else in a Homebrew formula's
/// libexec, else in ~/Applications or /Applications.
fn notifier() -> Option<PathBuf> {
    let program = |app: PathBuf| app.join("Contents/MacOS/itero-notifier");
    if let Some(p) = std::env::var_os("ITERO_NOTIFIER").map(PathBuf::from) {
        return Some(if p.extension().is_some_and(|x| x == "app") {
            program(p)
        } else {
            p
        });
    }
    let exe = std::env::current_exe().ok();
    let beside = exe
        .as_ref()
        .and_then(|e| e.parent().map(|d| d.join("Itero.app")));
    // Homebrew: bin/itero is a link into the Cellar; the app is in the
    // formula's libexec, beside that bin
    let libexec = exe
        .and_then(|e| e.canonicalize().ok())
        .and_then(|e| e.parent().map(|d| d.join("../libexec/Itero.app")));
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Applications/Itero.app"));
    [
        beside,
        libexec,
        home,
        Some(PathBuf::from("/Applications/Itero.app")),
    ]
    .into_iter()
    .flatten()
    .map(program)
    .find(|p| p.is_file())
}

/// The ways to show a notification here, in order: each a program and its
/// arguments before the title and text. On macOS, Itero.app's notifier
/// (notifications from itero), else osascript (shown as Script Editor's).
/// Elsewhere, whichever the desktop has: notify-send (libnotify), gdbus
/// (the freedesktop notification service), kdialog (KDE).
fn notifiers(title: &str, body: &str) -> Vec<(PathBuf, Vec<String>)> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if cfg!(target_os = "macos") {
        let script = s(&[
            "-e",
            "on run argv",
            "-e",
            "display notification (item 2 of argv) with title (item 1 of argv)",
            "-e",
            "end run",
        ]);
        let mut out: Vec<(PathBuf, Vec<String>)> =
            notifier().into_iter().map(|p| (p, vec![])).collect();
        out.push(("osascript".into(), script));
        out.iter_mut()
            .for_each(|(_, a)| a.extend([title.to_string(), body.to_string()]));
        return out;
    }
    vec![
        ("notify-send".into(), s(&["--app-name=itero", title, body])),
        (
            "gdbus".into(),
            s(&[
                "call",
                "--session",
                "--dest=org.freedesktop.Notifications",
                "--object-path=/org/freedesktop/Notifications",
                "--method=org.freedesktop.Notifications.Notify",
                "itero",
                "0",
                "",
                title,
                body,
                "[]",
                "{}",
                "-1",
            ]),
        ),
        (
            "kdialog".into(),
            s(&["--title", title, "--passivepopup", body, "10"]),
        ),
    ]
}

/// A desktop notification, the title and text as arguments (never in a
/// script's text), through the first of `notifiers` there is. With
/// `ITERO_NOTIFY_LOG` set, a line is appended to that file instead.
fn show(title: &str, body: &str) -> Result<(), String> {
    if let Some(log) = std::env::var_os("ITERO_NOTIFY_LOG") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("cannot open {}: {e}", PathBuf::from(&log).display()))?;
        // one write: lines from steps side by side never interleave
        f.write_all(format!("{title}\t{body}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut tried = vec![];
    for (program, args) in notifiers(title, body) {
        let out = match Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tried.push(program.display().to_string());
                continue;
            }
            Err(e) => return Err(format!("cannot run {}: {e}", program.display())),
        };
        if !out.status.success() {
            return Err(format!(
                "{}: {}",
                program.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        return Ok(());
    }
    Err(format!(
        "no way to show a notification here (tried {}): install libnotify's notify-send",
        tried.join(", ")
    ))
}

/// Why a program failed: what it needs that is not there, when its stderr
/// says so; its exit code, and the end of its stderr, where a traceback says
/// what went wrong.
fn exit_why(exit: i32, stderr: &[u8]) -> String {
    let all = String::from_utf8_lossy(stderr).trim().to_string();
    let why = plain_why(exit, &all);
    match crate::needs::missing(exit, &all) {
        Some(needs) => format!("{needs}; {why}"),
        None => why,
    }
}

fn plain_why(exit: i32, all: &str) -> String {
    let tail: String = all
        .chars()
        .rev()
        .take(500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.is_empty() {
        format!("exit {exit}")
    } else {
        format!("exit {exit}: {tail}")
    }
}

/// The built-in `#notify`, when the workspace has no action of that name.
pub const NOTIFY: &str = "notify";

/// The built-in `#forget`: forgets ended runs and their messages past
/// `limits.keep`, or all ended ones while `.itero/` is past its space.
pub const FORGET: &str = "forget";

/// The built-in `#roll-log`: `itero.log` becomes `itero.log.1`, `.1`
/// becomes `.2`, and so on, keeping `limits.logs` of them.
pub const ROLL_LOG: &str = "roll-log";

/// The actions itero carries, used when the workspace has none of the name.
pub const BUILT_IN: [&str; 3] = [NOTIFY, FORGET, ROLL_LOG];

fn forget(r: &StepRun) -> Result<Outcome, Failure> {
    let failed = |e: anyhow::Error| Failure::Error(format!("#forget: {e:#}"));
    let ws = crate::workspace::find(&r.root).map_err(failed)?;
    let now: chrono::DateTime<chrono::Utc> = r
        .now
        .parse()
        .map_err(|_| Failure::Error(format!("#forget: now is not a time: {}", r.now)))?;
    let full = ws.used() > ws.limits.space;
    let keep = chrono::Duration::from_std(ws.limits.keep).unwrap_or(chrono::Duration::MAX);
    let before = if full { now } else { now - keep };
    let before = super::daemon::stamp(before);
    let j = super::journal::Journal::open(&ws).map_err(failed)?;
    j.prune(&before).map_err(failed)?;
    if full {
        j.compact().map_err(failed)?;
    }
    let mut out = Map::new();
    out.insert("forgot_before".into(), before.into());
    Ok(Outcome::Done(out))
}

fn roll_log(r: &StepRun) -> Result<Outcome, Failure> {
    let failed = |e: anyhow::Error| Failure::Error(format!("#roll-log: {e:#}"));
    let ws = crate::workspace::find(&r.root).map_err(failed)?;
    let log = ws.root.join(".itero").join("itero.log");
    let nth = |n: usize| log.with_extension(format!("log.{n}"));
    let keep = ws.limits.logs;
    let gone = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(Failure::Error(format!("#roll-log: {e}")))
        }
    };
    std::fs::remove_file(nth(keep)).or_else(gone)?;
    for n in (1..keep).rev() {
        std::fs::rename(nth(n), nth(n + 1)).or_else(gone)?;
    }
    std::fs::rename(&log, nth(1)).or_else(gone)?;
    super::journal::Journal::open(&ws)
        .and_then(|j| j.set_flag(super::journal::LOG_FULL, false))
        .map_err(failed)?;
    Ok(Outcome::Done(Map::new()))
}

/// What `itero worker` reads for one step (`docs/design-simple.md`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StepRun {
    /// The action's file, or `NOTIFY` for the built-in.
    pub file: PathBuf,
    /// The workspace: where the action runs.
    pub root: PathBuf,
    /// Its values, as environment variables.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    /// The run's state, given on stdin.
    #[serde(default)]
    pub state: Map<String, Value>,
    /// `$ITERO_MEMORY`: the workflow's file to keep things in between runs.
    pub memory: PathBuf,
    /// `$ITERO_WAITING_SINCE`, when it runs again after waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_since: Option<String>,
    /// Seconds it may print nothing (the daemon stops it after that).
    pub quiet: u64,
    /// Seconds it may take at most, printing or not.
    pub most: u64,
    /// The daemon's pass time, for the built-in actions.
    #[serde(default)]
    pub now: String,
}

/// How a step went, as `itero worker` prints it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Exit 0: what it printed, to join the state.
    Done(Map<String, Value>),
    /// Exit 75: not yet; what it printed joins the state all the same.
    NotYet(Map<String, Value>),
}

/// The exit code with which an action says "not yet" (`EX_TEMPFAIL`). The
/// worker reads it from the action and never passes it on: its own exit
/// 75, `LOST_EXIT`, means a lost job.
pub const NOT_YET: i32 = 75;

/// Runs one step: the action with its values in its environment and the
/// state on stdin; or a built-in one.
pub fn run_step(r: &StepRun) -> Result<Outcome, Failure> {
    run_step_telling(r, &|| ())
}

/// `run_step`, calling `printed` each time the action prints something, on
/// stdout or stderr: how the daemon knows a long step is still working.
pub fn run_step_telling(r: &StepRun, printed: &(dyn Fn() + Sync)) -> Result<Outcome, Failure> {
    if r.file == Path::new(FORGET) {
        return forget(r);
    }
    if r.file == Path::new(ROLL_LOG) {
        return roll_log(r);
    }
    if r.file == Path::new(NOTIFY) {
        let body = r
            .env
            .get("text")
            .ok_or_else(|| Failure::Error("#notify needs $text".into()))?;
        let title = r.env.get("title").map_or("itero", String::as_str);
        show(title, body)?;
        return Ok(Outcome::Done(Map::new()));
    }
    if let Some(dir) = r.memory.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    }
    let mut c = Command::new(&r.file);
    c.current_dir(&r.root)
        .envs(&r.env)
        .env("ITERO_MEMORY", &r.memory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(since) = &r.waiting_since {
        c.env("ITERO_WAITING_SINCE", since);
    }
    let mut child = c.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            format!("{} is not executable: chmod +x it", r.file.display())
        }
        // the file is there: what its first line runs it with is not
        std::io::ErrorKind::NotFound if r.file.is_file() => {
            match first_line(&r.file).and_then(|l| crate::needs::interpreter(&l)) {
                Some(p) => format!(
                    "needs {p} (the first line of {}): not found",
                    r.file.display()
                ),
                None => format!("cannot run {}: {e}", r.file.display()),
            }
        }
        _ => format!("cannot run {}: {e}", r.file.display()),
    })?;
    let mut stdin = child.stdin.take().expect("piped");
    // an action that reads nothing closes its end early: not an error
    let _ = writeln!(stdin, "{}", Value::Object(r.state.clone()));
    drop(stdin);
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let (stdout, stderr) = std::thread::scope(|s| {
        let out = s.spawn(|| read_telling(stdout, printed));
        let err = s.spawn(|| read_telling(stderr, printed));
        let join = |h: std::thread::ScopedJoinHandle<std::io::Result<Vec<u8>>>| {
            h.join()
                .unwrap_or_else(|_| Err(std::io::Error::other("its reader panicked")))
                .map_err(|e| format!("cannot read the action's output: {e}"))
        };
        Ok::<_, String>((join(out)?, join(err)?))
    })?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let Some(exit) = status.code() else {
        return Err(Failure::Lost(format!("killed ({status})")));
    };
    match exit {
        0 => Ok(Outcome::Done(answer(&stdout)?)),
        NOT_YET => Ok(Outcome::NotYet(answer(&stdout)?)),
        _ => Err(Failure::Error(exit_why(exit, &stderr))),
    }
}

/// Reads a pipe to its end, calling `printed` for each piece read.
pub fn read_telling(
    pipe: Option<impl std::io::Read>,
    printed: &(dyn Fn() + Sync),
) -> std::io::Result<Vec<u8>> {
    let mut all = vec![];
    let Some(mut pipe) = pipe else {
        return Ok(all);
    };
    let mut buf = [0; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                all.extend_from_slice(&buf[..n]);
                printed();
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(all)
}

/// A file's first line, as text.
pub fn first_line(f: &Path) -> Option<String> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(f).ok()?)
        .read_line(&mut line)
        .ok()?;
    Some(line.trim_end().to_string())
}

/// What an action printed: nothing, or one JSON object.
fn answer(stdout: &[u8]) -> Result<Map<String, Value>, Failure> {
    let text = String::from_utf8_lossy(stdout);
    let text = text.trim();
    if text.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        _ => {
            let start: String = text.chars().take(80).collect();
            Err(Failure::Error(format!(
                "it printed something that is not a JSON object: {start}"
            )))
        }
    }
}
