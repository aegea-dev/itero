//! Which Chrome, and starting it with its own profile and a debugging
//! port on this machine only.
use super::Mode;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Chrome's names on Linux, Chromium's after them (Alpine has only
/// Chromium).
const NAMES: [&str; 4] = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
];

fn apps() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/Applications/Google Chrome.app")];
    if let Some(home) = std::env::var_os("HOME") {
        v.push(PathBuf::from(home).join("Applications/Google Chrome.app"));
    }
    v
}

/// The Chrome to run: on macOS the app, else the first of `NAMES` on
/// `PATH`.
pub fn find() -> Result<PathBuf, String> {
    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let apps = if cfg!(target_os = "macos") {
        apps()
    } else {
        vec![]
    };
    find_in(&path, &apps)
}

/// `find`'s rule on given folders and apps; the error names every place
/// looked.
pub fn find_in(path: &[PathBuf], apps: &[PathBuf]) -> Result<PathBuf, String> {
    for app in apps {
        let exe = app.join("Contents/MacOS/Google Chrome");
        if exe.is_file() {
            return Ok(exe);
        }
    }
    for name in NAMES {
        for dir in path {
            let exe = dir.join(name);
            if exe.is_file() {
                return Ok(exe);
            }
        }
    }
    let looked: Vec<String> = apps
        .iter()
        .map(|a| a.display().to_string())
        .chain(NAMES.iter().map(|n| format!("{n} on PATH")))
        .collect();
    Err(format!(
        "no Chrome: looked for {}; install it from https://www.google.com/chrome/",
        looked.join(", ")
    ))
}

/// Whether a window can be shown: always on macOS; on Linux, with a
/// display.
pub fn has_screen() -> bool {
    let set = |v: &str| std::env::var_os(v).is_some_and(|d| !d.is_empty());
    cfg!(target_os = "macos") || set("DISPLAY") || set("WAYLAND_DISPLAY")
}

/// The mode a step asked for (`headless`), else a window where there is
/// a screen.
pub fn choose(headless: Option<bool>) -> Mode {
    match headless {
        Some(true) => Mode::Headless,
        Some(false) => Mode::Window,
        // tests never open a window on the screen of whoever runs them
        None if std::env::var_os("ITERO_TEST_BROWSER_HEADLESS").is_some() => Mode::Headless,
        None if has_screen() => Mode::Window,
        None => Mode::Headless,
    }
}

/// A Chrome started, and the port its DevTools answer on.
pub struct Launched {
    pub child: std::process::Child,
    pub port: u16,
}

/// How long Chrome is given to say its port. A first start reads its
/// files from the disk: on a cold, slow one (a CI runner's, a throttled
/// cloud volume, an SD card) that took over 20 s with no CPU time, waiting
/// on the disk and getting there. One stuck is still stopped, its
/// processes named (Puppeteer gives 30 s, Playwright 180 s).
const START: Duration = Duration::from_secs(60);

/// Whether Chrome starts without its sandbox. `ITERO_BROWSER_SANDBOX=off`
/// says so: a container's own user, whose Chrome cannot make the user
/// namespaces its sandbox needs. Unset, empty or `on`: only as root on
/// Linux, where the sandbox refuses to start. Anything else is a mistake.
pub fn no_sandbox(var: Option<&std::ffi::OsStr>, root_on_linux: bool) -> Result<bool, String> {
    match var.map(|v| v.to_str()) {
        None | Some(Some("" | "on")) => Ok(root_on_linux),
        Some(Some("off")) => Ok(true),
        Some(Some(v)) => Err(format!("ITERO_BROWSER_SANDBOX is {v}: on or off")),
        Some(None) => Err("ITERO_BROWSER_SANDBOX is not text: on or off".into()),
    }
}

/// Starts Chrome on `profile`: port 0, so Chrome picks a free one and
/// writes it to `DevToolsActivePort` there; none of Chrome's own first-run
/// pages, prompts or bars. Its stderr goes nowhere.
pub fn launch(exe: &Path, mode: Mode, profile: &Path) -> Result<Launched, String> {
    launch_with(exe, mode, profile, Stdio::null())
}

/// `launch`, with Chrome's stderr sent to `stderr`: a test keeps it, to say
/// why its Chrome stopped answering.
pub fn launch_with(
    exe: &Path,
    mode: Mode,
    profile: &Path,
    stderr: Stdio,
) -> Result<Launched, String> {
    launch_within(exe, mode, profile, stderr, START)
}

/// `launch_with`, giving up when Chrome gives no port within `limit`:
/// then the error names each of its processes, with its state, its kernel
/// wait and its CPU time, to tell one busy from one waiting.
pub fn launch_within(
    exe: &Path,
    mode: Mode,
    profile: &Path,
    stderr: Stdio,
    limit: Duration,
) -> Result<Launched, String> {
    // before anything is made or stopped: a wrong value fails first
    #[cfg(target_os = "linux")]
    // SAFETY: geteuid has no preconditions
    let root_on_linux = unsafe { libc::geteuid() } == 0;
    #[cfg(not(target_os = "linux"))]
    let root_on_linux = false;
    let drop_sandbox = no_sandbox(
        std::env::var_os("ITERO_BROWSER_SANDBOX").as_deref(),
        root_on_linux,
    )?;
    std::fs::create_dir_all(profile).map_err(|e| format!("{}: {e}", profile.display()))?;
    stop_left_over(profile)?;
    write_prefs(profile)?;
    let active = profile.join("DevToolsActivePort");
    // one left by a Chrome that ended is not this one's
    match std::fs::remove_file(&active) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", active.display())),
    }
    let mut c = Command::new(exe);
    c.arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check"])
        .args([
            "--disable-features=Translate",
            "--hide-crash-restore-bubble",
            // a window Chrome takes as covered (by another run's, or by
            // another app on the screen) is not drawn, and input to its
            // page waits for a frame that never comes
            "--disable-backgrounding-occluded-windows",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    // a process group of its own, with every process it starts, so `stop`
    // ends them all
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    if mode == Mode::Headless {
        // nobody scrolls by hand there; where a scrollbar takes room
        // (Linux), a whole-page screenshot would lose its width
        c.args(["--headless", "--hide-scrollbars"]);
    }
    if std::env::var_os("ITERO_TEST_BROWSER_OFFLINE").is_some() {
        // a test's Chrome reaches only the test's own server
        c.arg("--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1");
    }
    if std::env::var_os("ITERO_TEST_BROWSER_VERBOSE").is_some() {
        // a test's Chrome logs what it does, for a test that fails to say
        c.args(["--enable-logging=stderr", "--v=1"]);
    }
    if drop_sandbox {
        c.arg("--no-sandbox");
    }
    c.arg("about:blank");
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let end = Instant::now() + limit;
    loop {
        if let Ok(text) = std::fs::read_to_string(&active)
            && let Some(port) = text.lines().next().and_then(|l| l.trim().parse().ok())
        {
            return Ok(Launched { child, port });
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("Chrome ended as it started ({status})"));
        }
        if Instant::now() > end {
            // what each of its processes is doing, before they are stopped
            let processes = group_processes(child.id());
            stop(&mut child);
            return Err(format!(
                "Chrome gave no port in {} s; its processes (pid, state, kernel wait, CPU time, command):\n{processes}",
                limit.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Each live process in process group `group`, a line each: its pid,
/// state, kernel wait (`wchan`), CPU time and command (cut short).
fn group_processes(group: u32) -> String {
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let Some(pids) = run("pgrep", &["-g", &group.to_string()]) else {
        return "  (pgrep could not be run)".into();
    };
    let lines: Vec<String> = pids
        .lines()
        .filter_map(|pid| run("ps", &["-o", "pid=,stat=,wchan=,time=,command=", "-p", pid]))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .map(|l| format!("  {}", l.chars().take(160).collect::<String>()))
        .collect();
    if lines.is_empty() {
        return "  (none left)".into();
    }
    lines.join("\n")
}

/// How long a Chrome asked to end is given before it is killed.
const END: Duration = Duration::from_secs(2);

/// Stops a Chrome `launch` started, with every process it started: its
/// process group (it leads one of its own) is asked to end, then killed
/// after `END`; the Chrome itself is reaped.
pub fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = child.id() as libc::pid_t;
        // SAFETY: killpg only sends a signal, to the group the Chrome this
        // itero started leads; its id stays taken while any of it lives
        unsafe { libc::killpg(group, libc::SIGTERM) };
        let end = Instant::now() + END;
        while Instant::now() < end && !matches!(child.try_wait(), Ok(Some(_))) {
            std::thread::sleep(Duration::from_millis(50));
        }
        // the Chrome ended or not, its children too: none outlives it
        // SAFETY: as above
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A Chrome still running on `profile`, left by a daemon that ended
/// without stopping it: one Chrome per profile, so the new one would
/// hand over to it and end. Chrome names its process in the profile's
/// `SingletonLock` (`host-pid`); it is stopped only when its command line
/// names this profile.
fn stop_left_over(profile: &Path) -> Result<(), String> {
    let Ok(target) = std::fs::read_link(profile.join("SingletonLock")) else {
        return Ok(());
    };
    let Some(pid) = target
        .to_string_lossy()
        .rsplit('-')
        .next()
        .and_then(|p| p.parse::<u32>().ok())
    else {
        return Ok(());
    };
    let ours = format!("--user-data-dir={}", profile.display());
    let running = |pid: u32| {
        Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&ours))
            .unwrap_or(false)
    };
    if !running(pid) {
        return Ok(());
    }
    #[cfg(unix)]
    // SAFETY: kill only sends a signal, to a Chrome running on itero's own
    // profile, as its command line says
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let end = Instant::now() + Duration::from_secs(5);
    while running(pid) {
        if Instant::now() > end {
            return Err(format!(
                "a Chrome left running on {} (pid {pid}) did not stop",
                profile.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// What itero sets in a profile's preferences: no translate bar, no offer
/// to save a password, no site asking to send notifications. Each would
/// cover the page a step acts on.
fn quiet() -> Value {
    json!({
        "credentials_enable_service": false,
        "profile": {
            "password_manager_enabled": false,
            "default_content_setting_values": { "notifications": 2 }
        },
        "translate": { "enabled": false }
    })
}

/// `from`'s keys into `into`, objects merged key by key, so every other
/// key of `into` stays.
fn merge(into: &mut Value, from: &Value) {
    match (into, from) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

/// Sets `quiet()` in the profile's `Default/Preferences` before each
/// start, keeping what Chrome or the user put there.
pub fn write_prefs(profile: &Path) -> Result<(), String> {
    let path = profile.join("Default").join("Preferences");
    let shown = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut prefs = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| shown(&e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(shown(&e)),
    };
    if !prefs.is_object() {
        return Err(shown(&"not a JSON object"));
    }
    merge(&mut prefs, &quiet());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, prefs.to_string()).map_err(|e| shown(&e))
}
