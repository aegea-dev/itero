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
    let r = Running::start(ws.clone(), exe, held, |_, _| {}, false, None).unwrap();
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

/// A workspace whose connector `kutu.geldi` prints its `KUTU` arg, and
/// workflows listening with the args given.
fn with_listeners(
    root: &Path,
    listeners: &[(&str, serde_json::Value)],
) -> itero::workspace::Workspace {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(
        root.join("itero.config.json"),
        r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#,
    )
    .unwrap();
    std::fs::write(root.join(".env"), "KUTU_PAROLA=gizli-123\n").unwrap();
    std::fs::create_dir_all(root.join("connectors")).unwrap();
    std::fs::create_dir_all(root.join("workflows")).unwrap();
    let c = root.join("connectors/kutu.geldi");
    std::fs::write(
        &c,
        "#!/bin/sh\necho \"{\\\"kutu\\\": \\\"$KUTU\\\", \\\"p\\\": \\\"$PAROLA\\\"}\"\nexec sleep 600\n",
    )
    .unwrap();
    std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (name, args) in listeners {
        let on = if args.is_null() {
            serde_json::json!([{ "event": "kutu.geldi" }])
        } else {
            serde_json::json!([{ "event": "kutu.geldi", "args": args }])
        };
        std::fs::write(
            root.join(format!("workflows/{name}.json")),
            serde_json::json!({ "on": on, "steps": ["#yok"] }).to_string(),
        )
        .unwrap();
    }
    itero::workspace::find(root).unwrap()
}

/// What the connector's copies sent: each event's source and data.
fn sent(ws: &itero::workspace::Workspace) -> Vec<(String, serde_json::Value)> {
    let mut out: Vec<(String, serde_json::Value)> = itero::engine::journal::Journal::open(ws)
        .unwrap()
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter(|e| e.event.kind == "kutu.geldi")
        .map(|e| (e.event.source, e.event.data))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn a_connector_runs_once_for_each_set_of_args_the_workflows_give_it() {
    let d = tempfile::tempdir().unwrap();
    let pass = serde_json::json!("$$KUTU_PAROLA");
    let ws = with_listeners(
        d.path(),
        &[
            (
                "satış",
                serde_json::json!({ "KUTU": "satış", "PAROLA": pass }),
            ),
            (
                "satış-yedek",
                serde_json::json!({ "PAROLA": pass, "KUTU": "satış" }),
            ),
            ("destek", serde_json::json!({ "KUTU": "destek" })),
        ],
    );
    let copies = itero::connectors::copies(&ws).unwrap();
    assert_eq!(copies.len(), 2, "the same args share one: {copies:#?}");
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    let held = lock::take(&ws, "deneme").unwrap();
    let r = Running::start(ws.clone(), exe, held, |_, _| {}, false, None).unwrap();
    until("both copies to send", || sent(&ws).len() == 2);
    let got = sent(&ws);
    assert_eq!(got[0].0, r#"connector/kutu.geldi?{"KUTU":"destek"}"#);
    assert_eq!(got[0].1, serde_json::json!({ "kutu": "destek", "p": "" }));
    assert_eq!(
        got[1].0,
        r#"connector/kutu.geldi?{"KUTU":"satış","PAROLA":"$$KUTU_PAROLA"}"#
    );
    // its $$NAME from .env, given to the connector, never in its source
    assert_eq!(got[1].1["p"], "gizli-123");
    assert!(!got[1].0.contains("gizli"));
    assert_eq!(r.connector_pids().len(), 2);

    // a workflow changes its args: that copy stops, a new one starts
    let old = r.connector_pids();
    std::fs::write(
        d.path().join("workflows/destek.json"),
        r##"{ "on": [{ "event": "kutu.geldi", "args": { "KUTU": "iade" } }], "steps": ["#yok"] }"##,
    )
    .unwrap();
    until("the new copy to send", || {
        sent(&ws).iter().any(|(_, d)| d["kutu"] == "iade")
    });
    until("the old copy to end", || {
        old.iter().filter(|p| alive(**p)).count() == 1
    });
    assert_eq!(r.connector_pids().len(), 2);
    let pids = r.connector_pids();
    r.stop();
    until("every copy to end", || pids.iter().all(|p| !alive(*p)));
}

#[test]
fn a_connector_no_entry_gives_args_runs_once_with_none() {
    let d = tempfile::tempdir().unwrap();
    let ws = with_listeners(d.path(), &[("hepsi", serde_json::Value::Null)]);
    let copies = itero::connectors::copies(&ws).unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].source(), "connector/kutu.geldi");
    let e = tempfile::tempdir().unwrap();
    let ws = with_listeners(e.path(), &[]);
    assert_eq!(
        itero::connectors::copies(&ws).unwrap().len(),
        1,
        "no listener"
    );
}
