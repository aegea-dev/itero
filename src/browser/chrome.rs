//! Which Chrome, and starting it with its own profile and a debugging
//! port on this machine only.
use super::Mode;
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
        None if has_screen() => Mode::Window,
        None => Mode::Headless,
    }
}

/// A Chrome started, and the port its DevTools answer on.
pub struct Launched {
    pub child: std::process::Child,
    pub port: u16,
}

/// How long Chrome is given to say its port.
const START: Duration = Duration::from_secs(20);

/// Starts Chrome on `profile`: port 0, so Chrome picks a free one and
/// writes it to `DevToolsActivePort` there; none of Chrome's own first-run
/// pages, prompts or bars.
pub fn launch(exe: &Path, mode: Mode, profile: &Path) -> Result<Launched, String> {
    std::fs::create_dir_all(profile).map_err(|e| format!("{}: {e}", profile.display()))?;
    stop_left_over(profile)?;
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
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if mode == Mode::Headless {
        c.arg("--headless");
    }
    #[cfg(target_os = "linux")]
    // SAFETY: geteuid has no preconditions
    if unsafe { libc::geteuid() } == 0 {
        // root, as in a container: Chrome's sandbox refuses to start
        c.arg("--no-sandbox");
    }
    c.arg("about:blank");
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let end = Instant::now() + START;
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
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Chrome gave no port in {} s", START.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
