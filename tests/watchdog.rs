use chrono::{DateTime, Duration, Utc};
use itero::engine::watchdog::{Probe, Rules, Verdict, Watch, decide, pause};

fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn rules() -> Rules {
    Rules {
        check_every: std::time::Duration::from_secs(10),
        misses: 3,
        grace: std::time::Duration::from_secs(30),
        stale_after: std::time::Duration::from_secs(600),
    }
}

#[test]
fn the_watchdog_decides_when_to_restart() {
    let start = at("2026-10-15T10:00:00Z");
    let mut w = Watch::new(start);
    let idle = Probe::Health { busy_since: None };
    let busy = |since: &str| Probe::Health {
        busy_since: Some(at(since)),
    };
    // a healthy daemon is left alone, busy or waiting
    assert_eq!(
        decide(&mut w, &idle, start + Duration::seconds(10), &rules()),
        Verdict::Wait
    );
    assert_eq!(
        decide(
            &mut w,
            &busy("2026-10-15T10:00:05Z"),
            start + Duration::seconds(10),
            &rules()
        ),
        Verdict::Wait
    );
    // waiting a day for something to do is not hanging
    assert_eq!(
        decide(&mut w, &idle, start + Duration::days(1), &rules()),
        Verdict::Wait
    );
    // no answer while it starts up is not counted
    let mut w = Watch::new(start);
    assert_eq!(
        decide(
            &mut w,
            &Probe::NoAnswer,
            start + Duration::seconds(20),
            &rules()
        ),
        Verdict::Wait
    );
    assert_eq!(w.missed, 0);
    // three misses in a row after the grace: restart
    let later = start + Duration::seconds(60);
    assert_eq!(
        decide(&mut w, &Probe::NoAnswer, later, &rules()),
        Verdict::Wait
    );
    assert_eq!(
        decide(&mut w, &Probe::NoAnswer, later, &rules()),
        Verdict::Wait
    );
    let Verdict::Restart(why) = decide(&mut w, &Probe::NoAnswer, later, &rules()) else {
        panic!("three misses restart it");
    };
    assert!(why.contains("no answer"), "{why}");
    // an answer resets the count
    let mut w = Watch::new(start);
    decide(&mut w, &Probe::NoAnswer, later, &rules());
    decide(&mut w, &idle, later, &rules());
    assert_eq!(w.missed, 0);
    // answering, but one pass running past its limit: restart
    let Verdict::Restart(why) = decide(
        &mut w,
        &busy("2026-10-15T10:00:00Z"),
        start + Duration::minutes(11),
        &rules(),
    ) else {
        panic!("a pass that hangs restarts it");
    };
    assert!(why.contains("a pass running for 660s"), "{why}");
    // exited: restart, saying how
    let Verdict::Restart(why) = decide(
        &mut w,
        &Probe::Exited("exit status: 1".into()),
        start,
        &rules(),
    ) else {
        panic!("an exit restarts it");
    };
    assert!(why.contains("exited (exit status: 1)"), "{why}");
}

#[test]
fn the_pause_before_a_restart_doubles_and_resets() {
    let s = std::time::Duration::from_secs;
    assert_eq!(pause(s(0), s(1)), s(1));
    assert_eq!(pause(s(1), s(1)), s(2));
    assert_eq!(pause(s(32), s(1)), s(60));
    assert_eq!(pause(s(60), s(1)), s(60));
    assert_eq!(
        pause(s(60), s(300)),
        s(1),
        "a daemon that ran 5 minutes starts over"
    );
}

/// A process group started for a test, stopped whole when dropped: SIGTERM
/// first, so each process ends as it would for a user and writes its
/// coverage (`make coverage`), then SIGKILL for anything left.
#[cfg(unix)]
struct Group(std::process::Child);

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        // not `kill -9 -<pgid>`: Linux's kill takes no negative pid that way,
        // and the wait below would then never end
        // SAFETY: killpg only sends a signal to the group this test started,
        // which the child leads (process_group(0))
        unsafe {
            libc::killpg(self.0.id() as i32, libc::SIGTERM);
        }
        let _ =
            wait_timeout::ChildExt::wait_timeout(&mut self.0, std::time::Duration::from_secs(10));
        unsafe {
            libc::killpg(self.0.id() as i32, libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
#[test]
fn a_watchdog_restarts_a_daemon_that_dies() {
    use std::os::unix::process::CommandExt;
    let d = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    // a workspace with no workflows: the daemon idles, and serves its health
    std::fs::write(
        d.path().join("itero.config.json"),
        format!("{{\"port\": {port}}}"),
    )
    .unwrap();
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("watchdog.log");
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_itero"));
    c.args([
        "watchdog",
        "--check-every",
        "1",
        "--log",
        log.to_str().unwrap(),
    ])
    .current_dir(d.path())
    .env("ITERO_NO_SECRET_STORE", "1")
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .process_group(0);
    let mut watchdog = Group(c.spawn().unwrap());
    let client = itero::net::client(std::time::Duration::from_secs(5)).unwrap();
    let pid = |client: &reqwest::blocking::Client| -> Option<u64> {
        let v: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .ok()?
            .json()
            .ok()?;
        v["pid"].as_u64()
    };
    let wait = |what: &str, f: &dyn Fn() -> Option<u64>| -> u64 {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                std::time::Instant::now() < until,
                "timed out waiting for {what}; the watchdog's log:\n{}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let first = wait("the daemon", &|| pid(&client));
    // SAFETY: sends a signal to the daemon this test's watchdog started
    let killed = unsafe { libc::kill(first as i32, libc::SIGKILL) };
    assert_eq!(killed, 0, "{}", std::io::Error::last_os_error());
    let second = wait("a new daemon", &|| pid(&client).filter(|p| *p != first));
    assert_ne!(first, second);

    // SIGTERM to the watchdog alone (a person's kill, not the service's
    // stop of the whole group): it stops its daemon too, then ends
    // SAFETY: sends a signal to the watchdog this test started
    unsafe {
        libc::kill(watchdog.0.id() as i32, libc::SIGTERM);
    }
    let ended =
        wait_timeout::ChildExt::wait_timeout(&mut watchdog.0, std::time::Duration::from_secs(15))
            .unwrap();
    assert!(
        ended.is_some_and(|e| e.success()),
        "{ended:?}; the watchdog's log:\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    // SAFETY: signal 0 only asks whether the process is there
    let alive = || unsafe { libc::kill(second as i32, 0) } == 0;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while alive() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(!alive(), "the daemon outlived its watchdog");
}

#[test]
fn the_machines_watchdog_runs_the_daemon_over_every_workspace() {
    use itero::engine::watchdog::Target;
    let t = Target::Machine { port: Some(7600) };
    assert_eq!(t.args(), ["daemon", "--all", "--port", "7600"]);
    assert_eq!(t.port(), Some(7600));
    let t = Target::Machine { port: None };
    assert_eq!(t.args(), ["daemon", "--all"]);
}

#[test]
fn a_watchdog_over_every_workspace_restarts_its_daemon() {
    use std::os::unix::process::CommandExt;
    let d = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let log = d.path().join("watchdog.log");
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_itero"));
    c.args([
        "watchdog",
        "--all",
        "--port",
        &port.to_string(),
        "--check-every",
        "1",
        "--log",
        log.to_str().unwrap(),
    ])
    .current_dir(d.path())
    .env("HOME", d.path().join("ev"))
    .env("XDG_STATE_HOME", d.path().join("durum"))
    .env("XDG_CACHE_HOME", d.path().join("önbellek"))
    .env("ITERO_NO_SECRET_STORE", "1")
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .process_group(0);
    let _watchdog = Group(c.spawn().unwrap());
    let client = itero::net::client(std::time::Duration::from_secs(5)).unwrap();
    let pid = || -> Option<u64> {
        let v: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .ok()?
            .json()
            .ok()?;
        // the machine's daemon: its workspaces, no pass of its own
        v.get("workspaces")?;
        v["pid"].as_u64()
    };
    let wait = |what: &str, f: &dyn Fn() -> Option<u64>| -> u64 {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                std::time::Instant::now() < until,
                "timed out waiting for {what}; the watchdog's log:\n{}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let first = wait("the daemon", &pid);
    // SAFETY: sends a signal to the daemon this test's watchdog started
    let killed = unsafe { libc::kill(first as i32, libc::SIGKILL) };
    assert_eq!(killed, 0, "{}", std::io::Error::last_os_error());
    let second = wait("a new daemon", &|| pid().filter(|p| *p != first));
    assert_ne!(first, second);
}

#[cfg(unix)]
mod follows {
    use super::Group;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// A copy of `from` at `dir/itero`, as an install puts one: a new file
    /// renamed over the old.
    fn install(dir: &Path, from: &Path) {
        let tmp = dir.join(".itero.new");
        std::fs::copy(from, &tmp).unwrap();
        std::fs::rename(&tmp, dir.join("itero")).unwrap();
    }

    /// A workspace with no workflows on a free port, and a watchdog over it
    /// started as `exe`, checking every second, its lines in `log`.
    fn start(d: &Path, exe: &Path, log: &Path) -> (u16, Group) {
        use std::os::unix::process::CommandExt;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(d.join("itero.config.json"), format!("{{\"port\": {port}}}")).unwrap();
        let mut c = Command::new(exe);
        c.args([
            "watchdog",
            "--check-every",
            "1",
            "--log",
            log.to_str().unwrap(),
        ])
        .current_dir(d)
        .env("ITERO_NO_SECRET_STORE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
        (port, Group(c.spawn().unwrap()))
    }

    /// The pid of the daemon answering on `port`.
    fn daemon(port: u16) -> Option<u64> {
        let client = itero::net::client(Duration::from_secs(2)).ok()?;
        let v: serde_json::Value = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .ok()?
            .json()
            .ok()?;
        v["pid"].as_u64()
    }

    fn wait(log: &Path, what: &str, f: &dyn Fn() -> Option<u64>) -> u64 {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                Instant::now() < until,
                "timed out waiting for {what}; the watchdog's log:\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn a_watchdog_runs_the_itero_installed_in_its_place() {
        let (d, bin, logs) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let itero = Path::new(env!("CARGO_BIN_EXE_itero"));
        install(bin.path(), itero);
        let log = logs.path().join("watchdog.log");
        let (port, mut watchdog) = start(d.path(), &bin.path().join("itero"), &log);
        let first = wait(&log, "the daemon", &|| daemon(port));
        install(bin.path(), itero);
        let second = wait(&log, "the new itero's daemon", &|| {
            daemon(port).filter(|p| *p != first)
        });
        assert_ne!(first, second);
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(
            watchdog.0.try_wait().unwrap().is_none(),
            "the watchdog ended:\n{said}"
        );
        assert!(
            said.contains("itero changed:") && said.contains("running it"),
            "{said}"
        );
    }

    #[test]
    fn a_new_file_that_does_not_run_leaves_the_daemon_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let (d, bin, logs) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        install(bin.path(), Path::new(env!("CARGO_BIN_EXE_itero")));
        let log = logs.path().join("watchdog.log");
        let (port, _watchdog) = start(d.path(), &bin.path().join("itero"), &log);
        let first = wait(&log, "the daemon", &|| daemon(port));
        let tmp = bin.path().join(".itero.new");
        std::fs::write(&tmp, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&tmp, bin.path().join("itero")).unwrap();
        // three checks
        std::thread::sleep(Duration::from_millis(3500));
        let said = std::fs::read_to_string(&log).unwrap();
        assert_eq!(daemon(port), Some(first), "{said}");
        assert_eq!(
            said.matches("does not run").count(),
            1,
            "said once:\n{said}"
        );
    }

    #[test]
    fn a_new_file_a_new_inode_or_a_new_time_is_another_itero() {
        use itero::engine::watchdog::{Binary, changed};
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("itero");
        std::fs::write(&f, "bir").unwrap();
        let old = Binary::now(&f).unwrap();
        assert!(!changed(&old, &Binary::now(&f).unwrap()), "the same file");
        // a new file renamed over it: a new inode
        std::fs::write(d.path().join("yeni"), "bir").unwrap();
        std::fs::rename(d.path().join("yeni"), &f).unwrap();
        assert!(changed(&old, &Binary::now(&f).unwrap()), "a new inode");
        // written in place: a new time
        let old = Binary::now(&f).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&f, "iki").unwrap();
        assert!(changed(&old, &Binary::now(&f).unwrap()), "a new time");
        // a link moved to another file, as Homebrew does
        let link = d.path().join("bağ");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        let old = Binary::now(&link).unwrap();
        std::fs::write(d.path().join("başka"), "üç").unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(d.path().join("başka"), &link).unwrap();
        assert!(
            changed(&old, &Binary::now(&link).unwrap()),
            "the link moved"
        );
    }
}
