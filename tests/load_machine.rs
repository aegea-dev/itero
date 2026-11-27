//! #load under the machine's daemon: its daemon part says so. A binary of
//! its own: it sets XDG_STATE_HOME for the machine's list.
#![cfg(unix)]
use itero::engine::worker::{LOAD, Outcome, StepRun, run_step};
use serde_json::json;

#[test]
fn a_workspace_on_the_machines_list_says_its_daemon_is_the_machines() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let state = d.path().join("durum");
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "add"])
        .current_dir(d.path())
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("XDG_STATE_HOME", &state)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // SAFETY: the only test in this binary, before anything reads the list
    unsafe { std::env::set_var("XDG_STATE_HOME", &state) };
    // the folder as the list has it, and as the daemon gives its workers
    let root = d.path().canonicalize().unwrap();
    let out = run_step(&StepRun {
        file: LOAD.into(),
        root: root.clone(),
        workflow: "ölçüm".into(),
        env: Default::default(),
        input: json!({ "args": { "measure": ["daemon"] } })
            .as_object()
            .cloned()
            .unwrap(),
        memory: root.join(".itero/memory/ölçüm"),
        waiting_since: None,
        quiet: 10,
        most: 10,
        now: "2026-11-26T10:00:00Z".into(),
    });
    match out {
        Ok(Outcome::Done(v)) => assert_eq!(v["daemon"]["machine"], true, "{v}"),
        other => panic!("{other:?}"),
    }
}
