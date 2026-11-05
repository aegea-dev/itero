//! One daemon per workspace's journal: whichever runs it holds a lock,
//! and a second one is refused, naming the first.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace(d: &Path) -> itero::workspace::Workspace {
    std::fs::write(
        d.join("itero.config.json"),
        r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#,
    )
    .unwrap();
    itero::workspace::find(d).unwrap()
}

/// Waits for a lock just let go to be free. It is free at once, but a
/// process this test binary is starting meanwhile (the other tests' daemons)
/// holds a copy of the file until it runs its own program.
fn until_free(ws: &itero::workspace::Workspace) {
    let t = Instant::now();
    while itero::engine::lock::held(ws).unwrap() {
        assert!(t.elapsed() < Duration::from_secs(5), "still held");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn held_says_whether_a_daemon_runs_it_and_takes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path());
    let lock = d.path().join(".itero/daemon.lock");
    assert!(!itero::engine::lock::held(&ws).unwrap());
    assert!(!lock.exists(), "asking makes no file");
    let first = itero::engine::lock::take(&ws, "sakin-nehir, pid 1").unwrap();
    assert!(itero::engine::lock::held(&ws).unwrap());
    // asking lets nothing go, and writes nothing
    assert!(itero::engine::lock::take(&ws, "başka").is_err());
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        "sakin-nehir, pid 1"
    );
    drop(first);
    until_free(&ws);
    // a daemon starting while a command asks (holding the lock shared
    // for an instant, here 50 ms) still takes it, trying again
    use std::os::fd::AsRawFd;
    let asking = std::fs::File::open(&lock).unwrap();
    // SAFETY: flock on a descriptor this test owns
    assert_eq!(
        unsafe { libc::flock(asking.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) },
        0
    );
    let lets_go = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(asking);
    });
    let t = Instant::now();
    drop(itero::engine::lock::take(&ws, "başka").unwrap());
    assert!(t.elapsed() >= Duration::from_millis(40), "it waited");
    lets_go.join().unwrap();
}

#[test]
fn a_second_lock_on_one_workspace_is_refused_naming_the_first() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path());
    let first = itero::engine::lock::take(&ws, "sakin-nehir, pid 1").unwrap();
    let err = itero::engine::lock::take(&ws, "başka").err().unwrap();
    assert!(err.contains("sakin-nehir, pid 1"), "{err}");
    drop(first);
    until_free(&ws);
    assert!(itero::engine::lock::take(&ws, "başka").is_ok());
}

#[test]
fn one_pass_refuses_a_workspace_already_run_and_runs_nothing() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path());
    let held = itero::engine::lock::take(&ws, "sakin-nehir, pid 1").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["daemon", "--once"])
        .current_dir(d.path())
        .env("ITERO_NO_SECRET_STORE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    drop(held);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("is run by sakin-nehir, pid 1"), "{said}");
    assert!(
        !d.path().join(".itero/journal.sqlite").exists(),
        "a pass ran beside the daemon"
    );
}

#[test]
fn a_foreground_daemon_refuses_a_workspace_already_run() {
    let d = tempfile::tempdir().unwrap();
    workspace(d.path());
    let daemon = || {
        Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(d.path())
            .env("ITERO_NO_SECRET_STORE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = stop::Daemon(daemon());
    let end = Instant::now() + Duration::from_secs(15);
    while !d.path().join(".itero/port").exists() {
        assert!(Instant::now() < end, "the first daemon did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut second = daemon();
    let ended = wait_timeout::ChildExt::wait_timeout(&mut second, Duration::from_secs(10)).unwrap();
    if ended.is_none() {
        let _ = second.kill();
    }
    let second = second.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&second.stderr);
    let held_by = first.0.id();
    drop(first);
    assert!(ended.is_some(), "a second daemon ran on the same workspace");
    assert!(!second.status.success(), "{said}");
    assert!(
        said.contains("is run by") && said.contains(&format!("pid {held_by}")),
        "{said}"
    );
}

#[test]
fn any_file_can_be_locked_and_names_its_holder() {
    use itero::engine::lock::{Refused, take_file};
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("durum/kilit");
    let first = take_file(&f, "birinci, pid 7").unwrap();
    match take_file(&f, "ikinci") {
        Err(Refused::Held(by)) => assert_eq!(by, "birinci, pid 7"),
        Err(Refused::Failed(why)) => panic!("{why}"),
        Ok(_) => panic!("taken twice"),
    }
    drop(first);
    let t = Instant::now();
    while take_file(&f, "ikinci").is_err() {
        assert!(t.elapsed() < Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_jobs_lock_is_refused_while_held_and_free_after() {
    use itero::engine::lock::take_job;
    let d = tempfile::tempdir().unwrap();
    let held = take_job(d.path(), 7).unwrap().expect("free at first");
    assert!(take_job(d.path(), 7).unwrap().is_none(), "held");
    assert!(
        take_job(d.path(), 8).unwrap().is_some(),
        "each job has its own"
    );
    drop(held);
    // a process this binary starts meanwhile holds a copy for a moment
    let t = Instant::now();
    while take_job(d.path(), 7).unwrap().is_none() {
        assert!(t.elapsed() < Duration::from_secs(1), "not freed");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_lock_files_of_jobs_done_are_forgotten_and_every_failure_said() {
    use itero::engine::lock::{forget_jobs, take_job};
    let d = tempfile::tempdir().unwrap();
    for seq in [3, 4, 5, 6, 7] {
        drop(take_job(d.path(), seq).unwrap().unwrap());
    }
    let jobs = d.path().join(".itero/jobs");
    std::fs::write(jobs.join("notlar.txt"), "").unwrap();
    let left = || {
        let mut n: Vec<String> = std::fs::read_dir(&jobs)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        n.sort();
        n
    };
    // 4 still waits; 6 and 7 cannot be told: both said, the rest removed
    let err = forget_jobs(d.path(), &|seq| match seq {
        4 => Ok(false),
        6 | 7 => Err(format!("iş {seq}: bilinmiyor")),
        _ => Ok(true),
    })
    .unwrap_err();
    assert!(
        err.contains("iş 6: bilinmiyor") && err.contains("iş 7: bilinmiyor"),
        "{err}"
    );
    assert_eq!(left(), ["4.lock", "6.lock", "7.lock", "notlar.txt"]);
}
