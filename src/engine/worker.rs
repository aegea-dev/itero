//! The worker: runs one step and reports how it went. It is dumb: the
//! action's file with its values, or the built-in `#notify`; nothing else.
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

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
/// program) when set; else the stable copy (`notifier_app`), brought up
/// to date from the Itero.app itero ships with; that shipped copy itself
/// when the stable one cannot be made, said on stderr.
fn notifier() -> Option<PathBuf> {
    use super::notifier_app::{program, stable, up_to_date};
    let given = std::env::var_os("ITERO_NOTIFIER").map(|p| {
        let p = PathBuf::from(p);
        if p.extension().is_some_and(|x| x == "app") {
            program(&p)
        } else {
            p
        }
    });
    if given.is_some() {
        return given;
    }
    let shipped = shipped_app();
    let made = match (
        &shipped,
        std::env::var_os("HOME"),
        crate::machine::state_dir(),
    ) {
        (Some(app), Some(home), Ok(state)) => Some(up_to_date(
            app,
            &stable(Path::new(&home)),
            &state.join("notifier.lock"),
        )),
        _ => None,
    };
    if let Some(Err(why)) = &made {
        eprintln!("itero: notifying from the shipped Itero.app: {why}");
    }
    choose(None, shipped, made)
}

/// Which notifier to run: the one given, else the stable copy's, else the
/// shipped one's.
fn choose(
    given: Option<PathBuf>,
    shipped: Option<PathBuf>,
    made: Option<Result<PathBuf, String>>,
) -> Option<PathBuf> {
    given
        .or(made.and_then(Result::ok))
        .or(shipped.map(|app| super::notifier_app::program(&app)))
}

/// The Itero.app itero ships with: beside the itero binary, else in a
/// Homebrew formula's libexec, else in ~/Applications or /Applications.
fn shipped_app() -> Option<PathBuf> {
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
    .find(|app| super::notifier_app::program(app).is_file())
}

/// Whether a notification's url can be opened on a click: http or https,
/// never a file or another app's scheme, as it may come from a message.
pub fn openable(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// What `#notify` shows: `$title`, `$text`, and `$subtitle`, `$sound`
/// (`default`, or a sound's name), `$url` and `$buttons` when set.
#[derive(Default)]
pub struct Note {
    pub title: String,
    pub text: String,
    pub subtitle: Option<String>,
    pub sound: Option<String>,
    pub url: Option<String>,
    /// Its buttons' labels, at most `BUTTONS`.
    pub buttons: Vec<String>,
    /// Whether its step waits for a click on it.
    pub wait: bool,
    /// What a click on it is told with, as itero's own message.
    pub click: Option<Click>,
}

/// Where a click on a notification goes: `itero notification-clicked`,
/// run by the `itero` that showed it in its workspace, with its id, its
/// workflow and `$data`.
pub struct Click {
    pub id: String,
    pub workspace: PathBuf,
    pub itero: PathBuf,
    pub workflow: String,
    pub data: Option<String>,
}

/// The most buttons a notification has.
pub const BUTTONS: usize = 4;

impl Note {
    /// Its values past the title and text, by name, in this order: those
    /// shown, then (`all`, for the notifier) what a click is told with.
    /// The id is among the first when it waits, so tests can answer it.
    pub fn fields(&self, all: bool) -> Vec<(&'static str, String)> {
        let mut f: Vec<(&'static str, String)> = vec![];
        for (k, v) in [
            ("subtitle", &self.subtitle),
            ("sound", &self.sound),
            ("url", &self.url),
        ] {
            f.extend(v.clone().map(|v| (k, v)));
        }
        if !self.buttons.is_empty() {
            f.push(("buttons", Value::from(self.buttons.clone()).to_string()));
        }
        if self.wait {
            f.push(("wait", "true".into()));
        }
        if let Some(c) = &self.click {
            if all || self.wait {
                f.push(("notification", c.id.clone()));
            }
            if all {
                f.push(("workspace", c.workspace.display().to_string()));
                f.push(("itero", c.itero.display().to_string()));
                f.push(("workflow", c.workflow.clone()));
                f.extend(c.data.clone().map(|d| ("data", d)));
            }
        }
        f
    }
}

/// `$buttons`: a JSON list of 1 to `BUTTONS` labels.
fn labels(text: &str) -> Result<Vec<String>, String> {
    let bad = || format!("#notify's buttons are a list of 1 to {BUTTONS} labels: {text}");
    let labels: Vec<String> = serde_json::from_str(text).map_err(|_| bad())?;
    if labels.is_empty() || labels.len() > BUTTONS || labels.iter().any(|l| l.trim().is_empty()) {
        return Err(bad());
    }
    Ok(labels)
}

/// The built-in `#notify`. One that waits (with `$buttons`, or `$wait`)
/// says "not yet" with its notification's id as its `$output`, which the
/// click is matched on; run again while waiting, it shows nothing, and is
/// done with the click as its `$output` once one has come.
fn notify(r: &StepRun) -> Result<Outcome, Failure> {
    let get = |k: &str| r.env.get(k).cloned();
    let text = get("text").ok_or_else(|| Failure::Error("#notify needs $text".into()))?;
    let buttons = match r.env.get("buttons") {
        Some(t) => labels(t)?,
        None => vec![],
    };
    let wait = !buttons.is_empty() || r.env.get("wait").is_some_and(|w| w == "true");
    if wait && r.waiting_since.is_some() {
        let output = r.state.get("output").cloned().unwrap_or_default();
        let clicked = output.get("answer").is_some();
        return Ok(if clicked {
            Outcome::Done(output)
        } else {
            Outcome::NotYet(output)
        });
    }
    let data = get("data");
    if let Some(d) = &data
        && !serde_json::from_str::<Value>(d).is_ok_and(|v| v.is_object())
    {
        return Err(Failure::Error(format!("#notify's data is an object: {d}")));
    }
    let id = uuid::Uuid::now_v7().to_string();
    let itero = std::env::current_exe().map_err(|e| format!("which itero is this: {e}"))?;
    show(&Note {
        title: get("title").unwrap_or_else(|| "itero".into()),
        text,
        subtitle: get("subtitle"),
        sound: get("sound"),
        url: get("url"),
        buttons,
        wait,
        click: Some(Click {
            id: id.clone(),
            workspace: r.root.clone(),
            itero,
            workflow: r.workflow.clone(),
            data,
        }),
    })?;
    if !wait {
        return Ok(Outcome::Done(Value::String(String::new())));
    }
    let mut out = Map::new();
    out.insert("notification".into(), Value::String(id));
    Ok(Outcome::NotYet(Value::Object(out)))
}

/// The ways to show a notification here, in order: each a program and its
/// arguments. On macOS, Itero.app's notifier (notifications from itero,
/// all of the note), else osascript (shown as Script Editor's; no click,
/// and a sound only by name). Elsewhere, whichever the desktop has:
/// notify-send (libnotify; a sound as a hint), gdbus (the freedesktop
/// notification service), kdialog (KDE); a subtitle is the text's first
/// line there.
fn notifiers(n: &Note) -> Vec<(PathBuf, Vec<String>)> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if cfg!(target_os = "macos") {
        let mut out: Vec<(PathBuf, Vec<String>)> = notifier()
            .into_iter()
            .map(|p| {
                let mut a = vec![n.title.clone(), n.text.clone()];
                for (k, v) in n.fields(true) {
                    a.extend([format!("--{k}"), v]);
                }
                (p, a)
            })
            .collect();
        // the values only as arguments, never in the script's text
        let mut shows = "display notification (item 2 of argv) with title (item 1 of argv) subtitle (item 3 of argv)".to_string();
        let named = n.sound.as_deref().filter(|s| *s != "default");
        if named.is_some() {
            shows.push_str(" sound name (item 4 of argv)");
        }
        let mut script = s(&["-e", "on run argv", "-e", &shows, "-e", "end run"]);
        script.extend(s(&[&n.title, &n.text, n.subtitle.as_deref().unwrap_or("")]));
        script.extend(named.map(String::from));
        out.push(("osascript".into(), script));
        return out;
    }
    let text = match &n.subtitle {
        Some(sub) => format!("{sub}\n{}", n.text),
        None => n.text.clone(),
    };
    let (title, body) = (n.title.as_str(), text.as_str());
    let mut send = s(&["--app-name=itero"]);
    if let Some(sound) = n.sound.as_deref() {
        let name = if sound == "default" {
            "message-new-instant"
        } else {
            sound
        };
        send.push(format!("--hint=string:sound-name:{name}"));
    }
    send.extend(s(&[title, body]));
    vec![
        ("notify-send".into(), send),
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

/// A desktop notification, through the first of `notifiers` there is. With
/// `ITERO_NOTIFY_LOG` set, a line is appended to that file instead: the
/// title, the text, then `Note::fields` as `name=value`, tab separated.
fn show(n: &Note) -> Result<(), String> {
    if let Some(u) = &n.url
        && !openable(u)
    {
        return Err(format!("#notify's url is http or https: {u}"));
    }
    if let Some(log) = std::env::var_os("ITERO_NOTIFY_LOG") {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("cannot open {}: {e}", PathBuf::from(&log).display()))?;
        // one write: lines from steps side by side never interleave
        let mut line = format!("{}\t{}", n.title, n.text);
        for (k, v) in n.fields(false) {
            line.push_str(&format!("\t{k}={v}"));
        }
        line.push('\n');
        f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut tried = vec![];
    for (program, args) in notifiers(n) {
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
            let why = format!(
                "{}: {}",
                program.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
            // macOS refused Itero.app outright: its record follows a copy
            // that is gone. Launched through LaunchServices, the record
            // moves to this one (and a first run asks)
            if let Some(app) = app_of(&program) {
                return after(Err(why), || through_launch_services(&app, &args));
            }
            return Err(why);
        }
        return Ok(());
    }
    Err(format!(
        "no way to show a notification here (tried {}): install libnotify's notify-send",
        tried.join(", ")
    ))
}

/// Whether notifications can be shown here, for `itero info`: on macOS,
/// macOS's setting for Itero (asked of its notifier, three seconds at
/// most) and the copy notifying; elsewhere, the desktop's notifier.
pub fn notifications() -> Value {
    if !cfg!(target_os = "macos") {
        let found = ["notify-send", "gdbus", "kdialog"].into_iter().find(|p| {
            std::env::var_os("PATH")
                .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(p).is_file()))
        });
        return match found {
            Some(p) => json!({ "state": "available", "notifier": p }),
            None => json!({ "state": "none", "why": "no notify-send, gdbus or kdialog on PATH" }),
        };
    }
    let Some(program) = notifier() else {
        return json!({ "state": "none", "why": "no Itero.app found" });
    };
    let app = app_of(&program);
    let version = app.as_deref().and_then(super::notifier_app::version);
    let asked = Command::new(&program)
        .arg("--status")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())
        .and_then(|mut c| match c.wait_timeout(Duration::from_secs(3)) {
            Ok(Some(_)) => c.wait_with_output().map_err(|e| e.to_string()),
            Ok(None) => {
                let _ = c.kill();
                let _ = c.wait();
                Err("no answer in three seconds".into())
            }
            Err(e) => Err(e.to_string()),
        });
    let mut v = match asked {
        Ok(o) if o.status.success() => {
            json!({ "state": String::from_utf8_lossy(&o.stdout).trim() })
        }
        Ok(o) => json!({ "state": "unknown", "why": String::from_utf8_lossy(&o.stderr).trim() }),
        Err(why) => json!({ "state": "unknown", "why": why }),
    };
    v["app"] = json!(app.map(|a| a.display().to_string()));
    v["version"] = json!(version);
    v
}

/// The Itero.app a notifier program is inside, on macOS.
fn app_of(program: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") || !program.ends_with("Contents/MacOS/itero-notifier") {
        return None;
    }
    program.ancestors().nth(3).map(Path::to_path_buf)
}

/// Whether the notifier's error says macOS refused it outright.
fn refused(why: &str) -> bool {
    why.contains("Notifications are not allowed")
}

/// After the notifier's `first` try: a refusal is healed once (`heal`);
/// refused again, notifications are off for Itero.
fn after(
    first: Result<(), String>,
    heal: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    match first {
        Err(why) if refused(&why) => match heal() {
            Err(again) if refused(&again) => Err(
                "notifications are off for Itero: turn them on in System Settings, Notifications, Itero"
                    .into(),
            ),
            other => other,
        },
        other => other,
    }
}

/// Shows the notification by launching `app` through LaunchServices (a new
/// instance, in the background): what moves macOS's record of Itero to
/// this copy. The notifier writes how it went to a result file, waited
/// for (`open -W` cannot wait on a notifier that ends at once).
fn through_launch_services(app: &Path, args: &[String]) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("itero-notify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let result = dir.join("result");
    let status = Command::new("open")
        .args(["-n", "-g"])
        .arg(app)
        .arg("--args")
        .args(args)
        .arg("--result")
        .arg(&result)
        .stdin(Stdio::null())
        .status();
    let shown = match status {
        Err(e) => Err(format!("cannot run open: {e}")),
        Ok(s) if !s.success() => Err(format!("open {}: {s}", app.display())),
        // time for the permission prompt a first run shows
        Ok(_) => wait_for_result(&result, Duration::from_secs(150)),
    };
    let _ = std::fs::remove_dir_all(&dir);
    shown
}

/// Waits for the notifier's result file (`ok`, or its error), watching its
/// folder, `limit` at most.
fn wait_for_result(result: &Path, limit: Duration) -> Result<(), String> {
    let read = || {
        std::fs::read_to_string(result)
            .ok()
            .map(|t| t.trim().to_string())
    };
    let folder = result.parent().unwrap_or(Path::new("."));
    let mut watching = crate::watch::open(folder).map_err(|e| e.to_string())?;
    let end = std::time::Instant::now() + limit;
    loop {
        if let Some(t) = read() {
            return if t == "ok" { Ok(()) } else { Err(t) };
        }
        let left = end.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(format!("no answer from Itero.app in {}s", limit.as_secs()));
        }
        watching.wait(Some(left)).map_err(|e| e.to_string())?;
    }
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
/// The args `#notify` reads.
pub const NOTIFY_ARGS: [&str; 8] = [
    "title", "text", "subtitle", "sound", "url", "buttons", "wait", "data",
];

/// The built-in `#forget`: forgets ended runs and their messages past
/// `limits.keep`, or all ended ones while `.itero/` is past its space.
pub const FORGET: &str = "forget";

/// The built-in `#roll-log`: `itero.log` becomes `itero.log.1`, `.1`
/// becomes `.2`, and so on, keeping `limits.logs` of them.
pub const ROLL_LOG: &str = "roll-log";

/// The built-in `#ai`: asks the local model (`crate::ai::step`).
pub const AI: &str = "ai";

/// The built-in `#find-models`: the models already on the machine, into
/// the index itero-ai reads (`crate::ai::find`).
pub const FIND_MODELS: &str = "find-models";

/// The actions itero carries, used when the workspace has none of the name.
pub const BUILT_IN: [&str; 5] = [NOTIFY, FORGET, ROLL_LOG, AI, FIND_MODELS];

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
    Ok(Outcome::Done(Value::Object(out)))
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
    Ok(Outcome::Done(Value::String(String::new())))
}

/// What `itero worker` reads for one step (`docs/design-simple.md`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StepRun {
    /// The action's file, or a built-in's name (`NOTIFY`, `AI`, ...).
    pub file: PathBuf,
    /// The workspace: where the action runs.
    pub root: PathBuf,
    /// The run's workflow: what a click on its notification names.
    #[serde(default)]
    pub workflow: String,
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
    /// Exit 0: what it printed, its `$output`.
    Done(Value),
    /// Exit 75: not yet; what it printed is its `$output` all the same.
    NotYet(Value),
}

/// The exit code with which an action says "not yet" (`EX_TEMPFAIL`). The
/// worker reads it from the action and never passes it on: its own exit
/// 75, `LOST_EXIT`, means a lost job.
pub const NOT_YET: i32 = 75;

/// Runs one step: the action with its values in its environment and the
/// state on stdin; or a built-in one.
pub fn run_step(r: &StepRun) -> Result<Outcome, Failure> {
    run_step_telling(r, &|_| ())
}

/// `run_step`, calling `told` each time the action prints something, on
/// stdout or stderr: how the daemon knows a long step is still working.
/// When a piece on stderr ends a line, `told` gets the last whole line of
/// it, the step's progress (`[21/40] ...`); else `None`.
pub fn run_step_telling(
    r: &StepRun,
    told: &(dyn Fn(Option<&str>) + Sync),
) -> Result<Outcome, Failure> {
    if r.file == Path::new(FORGET) {
        return forget(r);
    }
    if r.file == Path::new(ROLL_LOG) {
        return roll_log(r);
    }
    if r.file == Path::new(NOTIFY) {
        return notify(r);
    }
    if r.file == Path::new(AI) {
        return crate::ai::step::run(r, told);
    }
    if r.file == Path::new(FIND_MODELS) {
        return crate::ai::find::run(r);
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
    // a line on stderr may come in two pieces: the start of one is kept
    let partial = std::sync::Mutex::new(Vec::new());
    let (stdout, stderr) = std::thread::scope(|s| {
        let out = s.spawn(|| read_telling(stdout, &|_| told(None)));
        let err = s.spawn(|| {
            read_telling(stderr, &|piece| {
                let mut line = partial.lock().unwrap_or_else(|e| e.into_inner());
                line.extend_from_slice(piece);
                match last_line(&line) {
                    Some(said) => {
                        told(Some(&said));
                        let end = line.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                        line.drain(..end);
                    }
                    None => told(None),
                }
            })
        });
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
        0 => Ok(Outcome::Done(answer(&stdout))),
        NOT_YET => Ok(Outcome::NotYet(answer(&stdout))),
        _ => Err(Failure::Error(exit_why(exit, &stderr))),
    }
}

/// The last whole, non-blank line of `text`, trimmed and cut to 200
/// characters: what a step said last. `None` when no line has ended yet.
fn last_line(text: &[u8]) -> Option<String> {
    let end = text.iter().rposition(|&b| b == b'\n')?;
    String::from_utf8_lossy(&text[..end])
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect())
}

/// Reads a pipe to its end, calling `printed` with each piece read.
pub fn read_telling(
    pipe: Option<impl std::io::Read>,
    printed: &(dyn Fn(&[u8]) + Sync),
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
                printed(&buf[..n]);
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

/// What an action printed, trimmed, as its `$output`: JSON when it is
/// JSON, else the text; `""` when it printed nothing.
fn answer(stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    let text = text.trim();
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

#[cfg(test)]
mod notifier_tests {
    use super::choose;
    use std::path::PathBuf;

    #[test]
    fn a_refusal_is_healed_once_then_says_to_turn_notifications_on() {
        use super::{after, refused};
        let no = "/x/Itero.app/Contents/MacOS/itero-notifier: itero-notifier: cannot ask to notify: Notifications are not allowed for this application";
        assert!(refused(no));
        assert!(!refused(
            "itero-notifier: a url to open is http or https: ftp://x"
        ));
        let heals = std::cell::Cell::new(0);
        let heal = |r: Result<(), String>| {
            heals.set(heals.get() + 1);
            r
        };
        // shown at once: no healing
        assert_eq!(after(Ok(()), || heal(Ok(()))), Ok(()));
        assert_eq!(heals.get(), 0);
        // refused, healed through LaunchServices
        assert_eq!(after(Err(no.into()), || heal(Ok(()))), Ok(()));
        assert_eq!(heals.get(), 1);
        // refused twice: notifications are off
        let off = after(Err(no.into()), || heal(Err(no.into()))).unwrap_err();
        assert!(
            off.contains("System Settings, Notifications, Itero"),
            "{off}"
        );
        assert_eq!(heals.get(), 2);
        // another error: said as it is, no healing
        assert_eq!(
            after(Err("disk dolu".into()), || heal(Ok(()))),
            Err("disk dolu".into())
        );
        assert_eq!(heals.get(), 2);
    }

    #[test]
    fn the_result_written_later_is_waited_for_and_read() {
        use super::wait_for_result;
        use std::time::Duration;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sonuç");
        let g = f.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            std::fs::write(&g, "ok").unwrap();
        });
        assert_eq!(wait_for_result(&f, Duration::from_secs(5)), Ok(()));
        t.join().unwrap();
        // an error the notifier wrote is the result
        std::fs::write(&f, "Notifications are not allowed for this application").unwrap();
        assert!(
            wait_for_result(&f, Duration::from_secs(5))
                .unwrap_err()
                .contains("not allowed")
        );
        // none in time
        let none = d.path().join("yok");
        let started = std::time::Instant::now();
        assert!(wait_for_result(&none, Duration::from_millis(400)).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn the_given_notifier_wins_then_the_stable_copy_then_the_shipped_one() {
        let p = |s: &str| PathBuf::from(s);
        let shipped = || Some(p("/Cellar/21/Itero.app"));
        let stable = || Some(Ok(p("/destek/Itero.app/Contents/MacOS/itero-notifier")));
        assert_eq!(
            choose(Some(p("/deneme/bildirici")), shipped(), stable()),
            Some(p("/deneme/bildirici"))
        );
        assert_eq!(
            choose(None, shipped(), stable()),
            Some(p("/destek/Itero.app/Contents/MacOS/itero-notifier"))
        );
        assert_eq!(
            choose(None, shipped(), Some(Err("disk dolu".into()))),
            Some(p("/Cellar/21/Itero.app/Contents/MacOS/itero-notifier"))
        );
        assert_eq!(choose(None, None, None), None);
    }
}
