//! The daemon as it runs: it waits for something to do, and makes no pass
//! until a message, an edit or the next due time wakes it.
#![cfg(unix)]
use itero::engine::journal::Journal;
use itero::engine::steps;
use serde_json::Value;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The built-in keep runs hourly: off, so that nothing is due.
const KEEP_OFF: &str = r#"{"workflow": {"keep": {"off": true}}}"#;

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn itero(dir: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(dir: &Path, rel: &str, text: &str) {
    let f = dir.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn action(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let rel = format!("actions/{name}");
    write(dir, &rel, &format!("#!/bin/sh\n{body}\n"));
    std::fs::set_permissions(dir.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn start(dir: &Path) -> (Daemon, u16) {
    let child = Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("daemon")
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("ITERO_NOTIFIER", "/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let d = Daemon(child);
    let port = until("the daemon's port", Duration::from_secs(15), || {
        std::fs::read_to_string(dir.join(".itero/port"))
            .ok()?
            .trim()
            .parse()
            .ok()
    });
    until("the daemon's first pass", Duration::from_secs(15), || {
        (health(port)?["passes"].as_u64()? >= 1).then_some(())
    });
    (d, port)
}

fn until<T>(what: &str, limit: Duration, f: impl Fn() -> Option<T>) -> T {
    let end = Instant::now() + limit;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "waited {limit:?} for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn health(port: u16) -> Option<Value> {
    itero::net::client(Duration::from_secs(2))
        .ok()?
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .ok()?
        .json()
        .ok()
}

fn passes(port: u16) -> u64 {
    health(port).unwrap()["passes"].as_u64().unwrap()
}

fn statuses(dir: &Path) -> Vec<&'static str> {
    let ws = itero::workspace::find(dir).unwrap();
    Journal::open(&ws)
        .unwrap()
        .runs()
        .unwrap()
        .iter()
        .map(|r| steps::status(&r.runtime))
        .collect()
}

fn log_has(dir: &Path, event: &str) -> usize {
    std::fs::read_to_string(dir.join(".itero/itero.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(&format!("\"event\":\"{event}\"")))
        .count()
}

#[test]
fn an_idle_daemon_makes_no_passes() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    let (_daemon, port) = start(d.path());
    let before = passes(port);
    std::thread::sleep(Duration::from_secs(3));
    let h = health(port).unwrap();
    assert_eq!(h["passes"].as_u64().unwrap(), before, "{h}");
    assert!(h["busy_since"].is_null(), "{h}");
}

#[test]
fn a_message_wakes_it_and_a_deadline_ends_a_waiting_run_on_time() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/bekle.json",
        r##"{"on": [{ "event": "order.placed" }], "steps": ["#bekle"], "timeout": "2s"}"##,
    );
    action(d.path(), "bekle", "exit 75");
    let (_daemon, port) = start(d.path());
    let sent = Instant::now();
    itero(d.path(), &["emit", "order.placed", r#"{"order": 1}"#]);
    until("the run to wait", Duration::from_secs(3), || {
        (statuses(d.path()) == ["waiting"]).then_some(())
    });
    // nothing but the alarm wakes it at its deadline
    until("the deadline", Duration::from_secs(8), || {
        (statuses(d.path()) == ["error"]).then_some(())
    });
    // the journal keeps times to the second, and "waiting since" is the
    // second it began: a 2 s deadline comes between 1 s and 2 s on
    assert!(
        sent.elapsed() >= Duration::from_millis(1000),
        "{:?}",
        sent.elapsed()
    );
    // a pass for the message, its job, the deadline: a few, not one a second
    assert!(passes(port) <= 8, "{}", passes(port));
}

#[test]
fn a_reply_that_failed_is_tried_again_at_its_time() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/selamla.json",
        r##"{"on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"]}"##,
    );
    action(d.path(), "selamla", "true");
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (_daemon, _port) = start(d.path());
    itero(
        d.path(),
        &[
            "emit",
            "--reply",
            &format!("http://{closed}/itero"),
            "kişi.geldi",
            "{}",
        ],
    );
    until("the first try", Duration::from_secs(5), || {
        (log_has(d.path(), "reply.failed") == 1).then_some(())
    });
    // tried again 10 s on, woken by the alarm
    until("the second try", Duration::from_secs(15), || {
        (log_has(d.path(), "reply.failed") == 2).then_some(())
    });
}

#[test]
fn a_click_saved_by_another_process_wakes_it() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/tıkla.json",
        r##"{"on": [{ "event": "itero.notification.clicked" }], "steps": ["#yaz"]}"##,
    );
    action(d.path(), "yaz", "echo $answer >> cevap.txt");
    let (_daemon, _port) = start(d.path());
    itero(
        d.path(),
        &[
            "notification-clicked",
            r#"{"notification": "n1", "answer": "evet", "workflow": "x", "data": {}}"#,
        ],
    );
    until("the click's run", Duration::from_secs(5), || {
        std::fs::read_to_string(d.path().join("cevap.txt"))
            .ok()
            .filter(|s| s == "evet\n")
    });
}
