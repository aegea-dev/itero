//! The daemon as it runs: it waits for something to do, and makes no pass
//! until a message, an edit or the next due time wakes it.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use itero::engine::journal::{Journal, subject};
use itero::engine::steps;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

/// The built-in keep runs hourly: off, so that nothing is due.
const KEEP_OFF: &str = r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#;

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
        .args(["ws", "daemon"])
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
    itero(d.path(), &["ws", "emit", "order.placed", r#"{"order": 1}"#]);
    // nothing but the alarm wakes it at its deadline; the waiting itself is
    // read from the journal after, not watched for: on a slow machine the
    // run can start late, and its 2 s of waiting pass between two looks
    until("the deadline", Duration::from_secs(15), || {
        (statuses(d.path()) == ["error"]).then_some(())
    });
    let ws = itero::workspace::find(d.path()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let run = j.runs().unwrap().remove(0);
    assert_eq!(run.runtime.why.as_deref(), Some("waited 2s for data"));
    let waited = j
        .events(Some(&subject(&run.id)))
        .unwrap()
        .into_iter()
        .find(|e| e.event.kind == "action.waiting")
        .expect("the run waited");
    // the journal keeps times to the second, and "waiting since" is the
    // second it began: a 2 s deadline comes between 1 s and 2 s on, and
    // the pass that ends it may take a moment more
    let at = |t: &str| chrono::DateTime::parse_from_rfc3339(t).unwrap();
    let took = at(run.ended.as_deref().unwrap()) - at(&waited.event.time);
    assert!(
        (1..=3).contains(&took.num_seconds()),
        "the deadline came {took} after the run began waiting"
    );
    // a pass for the message, its job, the deadline: a few, not one a second
    assert!(passes(port) <= 8, "{}", passes(port));
}

#[test]
fn a_click_saved_by_another_process_wakes_it() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/tıkla.json",
        r##"{"on": [{ "event": "itero.notification.clicked" }],
             "steps": [{"#yaz": {"args": {"answer": "$event.answer"}}}]}"##,
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

/// The milliseconds of a UUIDv7: its first 48 bits.
fn ms(id: &str) -> i64 {
    i64::from_str_radix(&id.replace('-', "")[..12], 16).unwrap()
}

/// For each run of `workflow`, how long from its message saved to the run
/// started, in ms, read from the ids in the log: the daemon's own clock,
/// whatever the polling here costs.
fn picked_up(dir: &Path, workflow: &str) -> Vec<i64> {
    let log = std::fs::read_to_string(dir.join(".itero/itero.log")).unwrap();
    let lines: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    lines
        .iter()
        .filter(|l| l["event"] == "run.started" && l["workflow"] == workflow)
        .map(|l| ms(l["run"].as_str().unwrap()) - ms(l["message"].as_str().unwrap()))
        .collect()
}

fn median(mut v: Vec<i64>) -> i64 {
    v.sort_unstable();
    v[v.len() / 2]
}

fn ended(dir: &Path, workflow: &str) -> usize {
    let log = std::fs::read_to_string(dir.join(".itero/itero.log")).unwrap_or_default();
    log.lines()
        .filter(|l| l.contains(r#""event":"run.ended""#))
        .filter(|l| l.contains(&format!(r#""workflow":"{workflow}""#)))
        .count()
}

/// Sends `count` order.placed with `itero ws emit`, each after the run of
/// the one before has ended, so each comes to a daemon with nothing to do
/// for it.
fn emit_one_at_a_time(dir: &Path, count: usize) {
    for i in 0..count {
        itero(
            dir,
            &["ws", "emit", "order.placed", &format!(r#"{{"sıra": {i}}}"#)],
        );
        until("the order's run", Duration::from_secs(15), || {
            (ended(dir, "hızlı") > i).then_some(())
        });
    }
}

#[test]
fn an_event_saved_by_another_process_is_picked_up_at_once() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/hızlı.json",
        r##"{"on": [{ "event": "order.placed" }], "steps": ["#yaz"]}"##,
    );
    action(d.path(), "yaz", "cat > /dev/null; echo '{\"ok\": true}'");
    let (_daemon, _port) = start(d.path());
    emit_one_at_a_time(d.path(), 7);
    let took = picked_up(d.path(), "hızlı");
    // no waiting: what is left is the pass's own work, a few ms (the
    // wake file's quiet time made it 500 ms); room for a busy machine
    assert!(median(took.clone()) < 100, "saved to started, ms: {took:?}");
}

#[test]
fn an_event_that_comes_while_a_step_runs_is_picked_up_at_once() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", KEEP_OFF);
    write(
        d.path(),
        "workflows/yavaş.json",
        r##"{"on": [{ "event": "slow.started" }], "steps": ["#uyu"]}"##,
    );
    write(
        d.path(),
        "workflows/hızlı.json",
        r##"{"on": [{ "event": "order.placed" }], "steps": ["#yaz"]}"##,
    );
    action(d.path(), "uyu", "cat > /dev/null; sleep 60");
    action(d.path(), "yaz", "cat > /dev/null; echo '{\"ok\": true}'");
    let (_daemon, _port) = start(d.path());
    itero(d.path(), &["ws", "emit", "slow.started", "{}"]);
    until("the slow step", Duration::from_secs(15), || {
        let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).ok()?;
        log.lines()
            .any(|l| l.contains(r#""event":"step.started""#) && l.contains(r##""step":"#uyu""##))
            .then_some(())
    });
    // the pass started for the slow step runs as long as it does: each
    // order comes to it while it waits for that step
    emit_one_at_a_time(d.path(), 9);
    let took = picked_up(d.path(), "hızlı");
    // a pass that looked every 200 ms took 100-200 ms; now it is woken
    assert!(median(took.clone()) < 100, "saved to started, ms: {took:?}");
}
