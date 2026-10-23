//! `.itero/itero.log`: one JSON object per line, everything that happens,
//! each line with its time, workspace, daemon and event.
#![cfg(unix)]
use itero::engine::journal::Journal;
use serde_json::{Value, json};
use std::path::Path;

const NOW: &str = "2026-10-21T10:00:00Z";

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn executable(root: &Path, rel: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(root, rel, text);
    std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn lines(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join(".itero/itero.log"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

fn events(lines: &[Value]) -> Vec<&str> {
    lines.iter().map(|l| l["event"].as_str().unwrap()).collect()
}

#[test]
fn a_run_is_told_step_by_step() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    write(root, "itero.config.json", "{}");
    write(
        root,
        "workflows/sipariş.json",
        &json!({
            "on": [{ "event": "sipariş.geldi" }],
            "steps": [
                "#öde",
                { "#atla": { "on": [{ "$yok": true }] } },
                "#gönder"
            ]
        })
        .to_string(),
    );
    write(
        root,
        "workflows/bozuk.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#kır"] }"##,
    );
    executable(
        root,
        "actions/öde",
        "#!/bin/sh\n[ -n \"$tamam\" ] || { echo '{\"ödeme\": \"bekliyor\"}'; exit 75; }\n",
    );
    executable(root, "actions/atla", "#!/bin/sh\n");
    executable(
        root,
        "actions/gönder",
        "#!/bin/sh\necho '{\"kargo\": \"TR-ü1\"}'\n",
    );
    executable(root, "actions/kır", "#!/bin/sh\necho 'çöktü' >&2\nexit 3\n");
    let ws = itero::workspace::find(root).unwrap();
    let pass = || itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe()).unwrap();
    itero::engine::events::emit(&ws, "sipariş.geldi", Some(json!({"sipariş": 1})), NOW).unwrap();
    pass();
    itero::engine::events::emit(
        &ws,
        "ödeme.geldi",
        Some(json!({"sipariş": 1, "tamam": true})),
        NOW,
    )
    .unwrap();
    pass();

    let all = lines(root);
    let j = Journal::open(&ws).unwrap();
    let workspace = j.workspace().unwrap().id;
    for l in &all {
        assert_eq!(l["workspace"], workspace.as_str(), "{l}");
        assert_eq!(l["daemon"].as_str().unwrap().len(), 36, "{l}");
        assert!(l["time"].as_str().unwrap().ends_with('Z'), "{l}");
    }
    let run = j
        .runs()
        .unwrap()
        .into_iter()
        .find(|r| r.workflow == "sipariş")
        .unwrap();
    let mine: Vec<&Value> = all.iter().filter(|l| l["run"] == run.id.as_str()).collect();
    for l in &mine {
        assert_eq!(l["run_name"], run.name.as_str(), "{l}");
        assert_eq!(l["workflow"], "sipariş", "{l}");
    }
    let told: Vec<String> = mine
        .iter()
        .map(|l| {
            format!(
                "{} {}",
                l["event"].as_str().unwrap(),
                l["step"].as_str().unwrap_or("")
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            "run.started ",
            "step.started #öde",
            "step.waiting #öde",
            "step.started #öde",
            "step.done #öde",
            "step.skipped #atla",
            "step.started #gönder",
            "step.done #gönder",
            "run.ended ",
        ],
        "{all:#?}"
    );
    let ended = mine.last().unwrap();
    assert_eq!(ended["status"], "done");
    let first = &all[0];
    assert_eq!(first["event"], "message");
    assert_eq!(first["type"], "sipariş.geldi");
    assert_eq!(first["data"]["sipariş"], 1);
    assert_eq!(first["started"].as_array().unwrap().len(), 2, "{first}");
    let done = mine
        .iter()
        .find(|l| l["event"] == "step.done" && l["step"] == "#gönder");
    assert_eq!(done.unwrap()["output"]["kargo"], "TR-ü1");
    let paid = all
        .iter()
        .find(|l| l["event"] == "message" && l["type"] == "ödeme.geldi");
    assert_eq!(paid.unwrap()["reached"], json!([run.id]));
    let failed = all.iter().find(|l| l["event"] == "step.failed").unwrap();
    assert_eq!(failed["step"], "#kır");
    assert!(
        failed["why"].as_str().unwrap().contains("çöktü"),
        "{failed}"
    );
    let broke = all
        .iter()
        .find(|l| l["event"] == "run.ended" && l["workflow"] == "bozuk")
        .unwrap();
    assert_eq!(broke["status"], "error");
    assert!(broke["why"].as_str().unwrap().contains("çöktü"), "{broke}");
}

#[test]
fn a_message_nothing_wants_says_so() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", "{}");
    let ws = itero::workspace::find(d.path()).unwrap();
    itero::engine::events::emit(&ws, "kimse.istemez", Some(json!({"ü": 1})), NOW).unwrap();
    itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe()).unwrap();
    let all = lines(d.path());
    assert_eq!(events(&all), ["message"]);
    assert_eq!(all[0]["nothing"], true, "{}", all[0]);
}

/// A daemon process: started, its source started and ended, stopped.
#[test]
fn a_daemon_says_when_it_starts_and_stops() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    write(root, "itero.config.json", r#"{"sahibi": "ayşe"}"#);
    executable(
        root,
        "sources/kapı.çaldı",
        "#!/bin/sh\necho '{\"kim\": \"ayşe\"}'\n",
    );
    /// Kills the daemon however the test ends.
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut daemon = Running(
        std::process::Command::new(exe())
            .args(["daemon", "--no-serve"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let child = &mut daemon.0;
    let saw = |event: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if lines(root).iter().any(|l| l["event"] == event) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    };
    assert!(saw("source.ended"), "{:#?}", lines(root));
    let own = || {
        lines(root)
            .iter()
            .any(|l| l["event"] == "message" && l["type"] == "itero.daemon.started")
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !own() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(own(), "{:#?}", lines(root));
    // SAFETY: a plain kill of our own child
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    child.wait().unwrap();
    let all = lines(root);
    let started = &all[0];
    assert_eq!(started["event"], "daemon.started");
    let ws = itero::workspace::find(root).unwrap();
    let w = Journal::open(&ws).unwrap().workspace().unwrap();
    assert_eq!(started["workspace"], w.id.as_str());
    assert_eq!(started["workspace_name"], w.name.as_str());
    assert_eq!(started["workspace_created"], w.created.as_str());
    // the daemon finds its folder from where it runs: the path as the
    // system has it
    let here = std::fs::canonicalize(root).unwrap();
    assert_eq!(started["path"], here.display().to_string());
    assert_eq!(started["meta"]["sahibi"], "ayşe");
    assert_eq!(
        started["daemon_name"].as_str().unwrap().split('-').count(),
        2
    );
    assert_eq!(started["pid"], child.id());
    assert_eq!(started["itero"], env!("CARGO_PKG_VERSION"));
    let source = all.iter().find(|l| l["event"] == "source.started").unwrap();
    assert_eq!(source["source"], "kapı.çaldı");
    assert_eq!(all.last().unwrap()["event"], "daemon.stopped", "{all:#?}");
    assert!(all.iter().all(|l| l["daemon"] == started["daemon"]));
}
