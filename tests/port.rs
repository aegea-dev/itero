//! The daemon's port: the manifest's when it sets one, else one the OS
//! gives, kept in .itero/port and asked for again on the next start.
#![cfg(unix)]
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn itero(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("ITERO_NOTIFIER", "/usr/bin/true")
        .output()
        .unwrap()
}

fn daemon(dir: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("daemon")
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("ITERO_NOTIFIER", "/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn stop(mut c: Child) {
    let _ = c.kill();
    let _ = c.wait();
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < end, "waited 15s for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn saved(dir: &Path) -> Option<u16> {
    std::fs::read_to_string(dir.join(".itero/port"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The daemon answers for this workspace on `port`.
fn answers(dir: &Path, port: u16) -> bool {
    let info = String::from_utf8(itero(dir, &["info"]).stdout).unwrap();
    info.contains(&format!("port {port}")) && info.contains("daemon\n")
}

#[test]
fn without_a_port_the_os_gives_one_which_is_kept() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), r#"{"name": "kapısız"}"#).unwrap();
    let first = daemon(d.path());
    until("a port", || saved(d.path()).is_some());
    let port = saved(d.path()).unwrap();
    until("the daemon on its port", || answers(d.path(), port));
    stop(first);

    // the next start asks for the same one
    let again = daemon(d.path());
    until("the daemon on the same port", || answers(d.path(), port));
    assert_eq!(saved(d.path()), Some(port));
    stop(again);

    // taken meanwhile: another, kept, and the log says so
    let taken = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let other = daemon(d.path());
    until("another port", || {
        saved(d.path()).is_some_and(|p| p != port)
    });
    let new = saved(d.path()).unwrap();
    until("the daemon on the new port", || answers(d.path(), new));
    stop(other);
    drop(taken);
    let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).unwrap();
    assert!(log.contains(&format!("port {port} is taken")), "{log}");
}

#[test]
fn a_set_port_that_is_taken_stops_the_daemon_before_it_runs_anything() {
    let d = tempfile::tempdir().unwrap();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    std::fs::write(
        d.path().join("itero.config.json"),
        format!(r#"{{"port": {port}}}"#),
    )
    .unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::write(
        d.path().join("workflows/selam.json"),
        r##"{ "on": [{ "event": "selam.geldi" }], "steps": ["#notify"] }"##,
    )
    .unwrap();
    assert!(
        itero(d.path(), &["emit", "selam.geldi", r#"{"text": "merhaba"}"#])
            .status
            .success()
    );
    let o = daemon(d.path()).wait_with_output().unwrap();
    assert!(!o.status.success(), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains(&format!("cannot listen on 127.0.0.1:{port}")),
        "{err}"
    );
    assert!(!err.contains("serving on"), "{err}");
    let runs = String::from_utf8(itero(d.path(), &["wf", "runs"]).stdout).unwrap();
    assert!(runs.trim().is_empty(), "nothing ran: {runs}");
    // its loop never began: no start in the log
    let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).unwrap_or_default();
    assert!(!log.contains("daemon.started"), "{log}");
    drop(taken);
}
