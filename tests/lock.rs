//! One daemon per workspace's journal: whichever runs it holds a lock,
//! and a second one is refused, naming the first.
#![cfg(unix)]
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
    let mut first = daemon();
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
    let _ = first.kill();
    let _ = first.wait();
    assert!(ended.is_some(), "a second daemon ran on the same workspace");
    assert!(!second.status.success(), "{said}");
    assert!(
        said.contains("is run by") && said.contains(&format!("pid {}", first.id())),
        "{said}"
    );
}
