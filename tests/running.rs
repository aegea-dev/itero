//! A workspace's threads in a daemon, started in one place and stopped
//! with nothing left behind: its connectors' processes, its watcher, its
//! alarm, its pass loop, and its lock.
#![cfg(unix)]
use itero::engine::lock;
use itero::engine::running::Running;
use std::path::Path;
use std::time::{Duration, Instant};

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < end, "waited 10s for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only asks whether the process is there
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn a_workspace_started_then_stopped_leaves_nothing_running() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(
        root.join("itero.config.json"),
        r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#,
    )
    .unwrap();
    std::fs::create_dir(root.join("connectors")).unwrap();
    let c = root.join("connectors/kuş.geldi");
    std::fs::write(&c, "#!/bin/sh\nexec sleep 600\n").unwrap();
    std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ws = itero::workspace::find(root).unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));

    let held = lock::take(&ws, "deneme").unwrap();
    let r = Running::start(ws.clone(), exe, held, |_, _| {}, false).unwrap();
    until("the connector to run", || r.connector_pids().len() == 1);
    until("the first pass", || {
        r.engine.health.lock().unwrap().passes >= 1
    });
    let pid = r.connector_pids()[0];
    assert!(alive(pid));

    let t = Instant::now();
    r.stop();
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    until("the connector to end", || !alive(pid));
    until("the lock to be free", || lock::take(&ws, "yine").is_ok());
}
