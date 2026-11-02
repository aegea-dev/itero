//! A daemon a test starts ends with that test, however the test ends: a
//! failed assertion unwinds through the guard that holds it. A bare
//! `Child` is dropped without its process being stopped.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn a_daemon_held_by_a_test_that_fails_is_stopped() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("itero.config.json"),
        r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#,
    )
    .unwrap();
    let root = d.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let failed = std::thread::spawn(move || {
        let daemon = stop::Daemon(
            Command::new(env!("CARGO_BIN_EXE_itero"))
                .args(["daemon", "--no-serve"])
                .current_dir(&root)
                .env("ITERO_NO_SECRET_STORE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        tx.send(daemon.0.id()).unwrap();
        panic!("a test that fails while its daemon runs");
    })
    .join();
    assert!(failed.is_err());
    let pid = rx.recv().unwrap() as libc::pid_t;
    // SAFETY: signal 0 only asks whether the process is there
    let alive = || unsafe { libc::kill(pid, 0) } == 0;
    let end = Instant::now() + Duration::from_secs(15);
    while alive() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let left = alive();
    if left {
        // SAFETY: kill only sends a signal, to the daemon this test started
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(!left, "daemon {pid} outlived the test that held it");
}
