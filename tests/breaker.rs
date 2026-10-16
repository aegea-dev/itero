use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use std::path::Path;

fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

fn workspace(d: &Path, flow: &str) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    std::fs::write(d.join("workflows/bozuk.json"), flow).unwrap();
    itero::workspace::find(d).unwrap()
}

/// Fails every run: its shell line exits 3.
const BOZUK: &str = r#"{ "name": "bozuk", "description": "hep bozuk", "triggers": [ { "on": "dene.simdi" } ], "pause_after": 2,
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "action": "sh@1", "with": { "run": "echo çöktü >&2; exit 3" } } ] } ] }"#;

fn kick(ws: &itero::workspace::Workspace) {
    itero::engine::events::emit(ws, "dene.simdi", None, None, "2026-10-16T10:00:00Z").unwrap();
}

/// `n` runs, one pass each.
fn runs(ws: &itero::workspace::Workspace, n: u32) {
    for second in 1..=n {
        kick(ws);
        run_once(ws, t(&format!("2026-10-16T10:00:{second:02}Z")), exe()).unwrap();
    }
}

#[test]
fn a_workflow_pauses_after_its_failed_runs_in_a_row_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), BOZUK);
    runs(&ws, 1);
    let j = Journal::open(&ws).unwrap();
    assert!(j.paused().unwrap().is_empty(), "one failure is not two");
    runs(&ws, 1);
    let p = j.paused().unwrap();
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].workflow, "bozuk");
    assert_eq!(p[0].why, "2 failed runs in a row");
    let said: Vec<_> = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter(|e| e.event.kind == "workflow.paused")
        .collect();
    assert_eq!(said.len(), 1);
    let data = &said[0].event.data;
    assert_eq!(data["workflow"], "bozuk");
    assert_eq!(data["failed"], 2);
    assert_eq!(data["version"].as_str().unwrap().len(), 12);
    let last = j.runs_page(Some("bozuk"), 1).unwrap().remove(0);
    assert_eq!(data["run"], last.id.as_str());
    // paused: the next event starts nothing
    runs(&ws, 1);
    assert_eq!(j.runs().unwrap().len(), 2);
}

#[test]
fn itero_resume_lets_it_start_again() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), BOZUK);
    runs(&ws, 2);
    let resume = || {
        let o = std::process::Command::new(exe())
            .args(["resume", "bozuk"])
            .current_dir(d.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    assert_eq!(resume(), "bozuk: resumed");
    assert_eq!(resume(), "bozuk: not paused");
    let j = Journal::open(&ws).unwrap();
    assert!(j.paused().unwrap().is_empty());
    runs(&ws, 1);
    assert_eq!(j.runs().unwrap().len(), 3, "it starts again");
}

#[test]
fn a_success_between_failures_starts_the_count_again() {
    let d = tempfile::tempdir().unwrap();
    // fails, then succeeds, and so on: a marker file in the workspace
    let flaky = BOZUK.replace(
        "echo çöktü >&2; exit 3",
        "if test -e işaret; then rm işaret; else touch işaret; exit 3; fi",
    );
    let ws = workspace(d.path(), &flaky);
    runs(&ws, 4);
    let j = Journal::open(&ws).unwrap();
    let states: Vec<String> = j
        .runs()
        .unwrap()
        .into_iter()
        .map(|r| r.runtime.state)
        .collect();
    assert_eq!(states, ["failed", "bitti", "failed", "bitti"]);
    assert!(j.paused().unwrap().is_empty());
}

#[test]
fn pause_after_zero_never_pauses() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        &BOZUK.replace("\"pause_after\": 2", "\"pause_after\": 0"),
    );
    runs(&ws, 3);
    let j = Journal::open(&ws).unwrap();
    assert!(j.paused().unwrap().is_empty());
    assert_eq!(j.runs().unwrap().len(), 3);
}
