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
    assert!(!itero::engine::lock::held(&ws).unwrap());
    // a daemon starting while commands ask still takes it
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let asking: Vec<_> = (0..4)
        .map(|_| {
            let (ws, done) = (ws.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = itero::engine::lock::held(&ws);
                }
            })
        })
        .collect();
    let taken = (0..500)
        .map(|_| itero::engine::lock::take(&ws, "başka").map(drop))
        .collect::<Result<Vec<_>, _>>();
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    for a in asking {
        a.join().unwrap();
    }
    taken.unwrap();
}

#[test]
fn a_second_lock_on_one_workspace_is_refused_naming_the_first() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path());
    let first = itero::engine::lock::take(&ws, "sakin-nehir, pid 1").unwrap();
    let err = itero::engine::lock::take(&ws, "başka").err().unwrap();
    assert!(err.contains("sakin-nehir, pid 1"), "{err}");
    drop(first);
    // free at once, but a process this test binary starts meanwhile (the
    // other test's daemons) holds a copy of the file until it runs its own
    // program: a few milliseconds
    let t = Instant::now();
    loop {
        match itero::engine::lock::take(&ws, "başka") {
            Ok(_) => break,
            Err(e) => assert!(t.elapsed() < Duration::from_secs(1), "{e}"),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
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
