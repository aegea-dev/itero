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

/// A workspace with one workflow, `workflows/<name>.json`.
fn workspace(d: &Path, name: &str, flow: &str) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    std::fs::write(d.join(format!("workflows/{name}.json")), flow).unwrap();
    itero::workspace::find(d).unwrap()
}

#[test]
fn a_scheduled_workflow_starts_a_run_once_per_slot() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        "sayac",
        r#"{ "name": "sayac", "description": "sayaç",
      "triggers": [ { "schedule": "*/5 * * * *", "args": { "kaynak": "zamanlayıcı" } } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "son": "${run.id}" } } ] } ] }"#,
    );
    // first pass: the schedule is seen, nothing is due yet
    run_once(&ws, t("2026-10-15T14:01:00Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert!(j.runs().unwrap().is_empty());
    // the 14:05 slot: one run, which ends
    run_once(&ws, t("2026-10-15T14:05:10Z"), exe()).unwrap();
    run_once(&ws, t("2026-10-15T14:06:00Z"), exe()).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].workflow, "sayac");
    assert_eq!(runs[0].runtime.state, "bitti");
    assert_eq!(runs[0].args["kaynak"], "zamanlayıcı");
    assert_eq!(runs[0].started, "2026-10-15T14:05:10Z");
    assert!(runs[0].ended.is_some());
    // the 14:10 slot: a second run
    run_once(&ws, t("2026-10-15T14:10:00Z"), exe()).unwrap();
    assert_eq!(j.runs().unwrap().len(), 2);
}

#[test]
fn a_run_with_a_job_runs_it_in_a_worker() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        "yanki",
        r#"{ "name": "yanki",
      "triggers": [ { "schedule": "* * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "command@1", "with": { "run": ["echo", "çıktı"] } } ] } ] }"#,
    );
    run_once(&ws, t("2026-10-15T14:00:30Z"), exe()).unwrap();
    run_once(&ws, t("2026-10-15T14:01:30Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].runtime.state, "bitti");
    let events = j.events(None).unwrap();
    let done = events
        .iter()
        .find(|r| r.event.kind == "action.finished")
        .expect("the job's result");
    assert_eq!(done.event.data["output"]["stdout"], "çıktı\n");
}
