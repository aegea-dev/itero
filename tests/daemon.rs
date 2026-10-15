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
    assert_eq!(
        runs[0].ended.as_deref(),
        Some("2026-10-15T14:05:10Z"),
        "the pass's time, not the clock's"
    );
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

#[test]
fn memory_set_by_one_run_is_read_by_the_next() {
    let d = tempfile::tempdir().unwrap();
    // each run adds one to sayi, reading what the last run left
    let ws = workspace(
        d.path(),
        "sayac",
        r#"{ "name": "sayac",
      "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1",
                  "with": { "sayi": "${has(memory.sayi) ? memory.sayi + 1 : 1}",
                            "not": "çalıştı" } } ] } ] }"#,
    );
    for at in [
        "2026-10-15T14:01:00Z",
        "2026-10-15T14:05:10Z",
        "2026-10-15T14:10:10Z",
    ] {
        run_once(&ws, t(at), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let m = j.memory("sayac").unwrap();
    assert_eq!(m["sayi"], 2);
    assert_eq!(m["not"], "çalıştı");
    assert!(j.memory("başka").unwrap().is_empty());
}

#[test]
fn a_failed_run_sets_no_memory() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        "bozuk",
        r#"{ "name": "bozuk",
      "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "yarim": "kalmamalı" } },
                { "action": "command@1", "with": { "run": ["false"] } } ] } ] }"#,
    );
    for at in ["2026-10-15T14:01:00Z", "2026-10-15T14:05:10Z"] {
        run_once(&ws, t(at), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.runs().unwrap()[0].runtime.state, "failed");
    assert!(j.runs().unwrap()[0].ended.is_some());
    assert!(j.memory("bozuk").unwrap().is_empty());
}

#[test]
fn an_app_event_starts_the_workflows_waiting_for_it_once() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        "kucuk-resim",
        r#"{ "name": "kucuk-resim",
      "triggers": [ { "on": "foto.eklendi", "args": { "boyut": 256 } } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "son": "${input.yol}", "boyut": "${args.boyut}" } } ] } ] }"#,
    );
    let now = "2026-10-15T14:00:00Z";
    let data = serde_json::json!({"yol": "çarşı.jpg"});
    itero::engine::events::emit(&ws, "foto.eklendi", Some(data), None, now).unwrap();
    itero::engine::events::emit(&ws, "baska.olay", None, None, now).unwrap();
    run_once(&ws, t(now), exe()).unwrap();
    run_once(&ws, t(now), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1, "one run, however many passes");
    assert_eq!(runs[0].runtime.state, "bitti");
    let m = j.memory("kucuk-resim").unwrap();
    assert_eq!(m["son"], "çarşı.jpg");
    assert_eq!(m["boyut"], 256);
    // the event nothing waits for is handled, with a note, not an error
    let other = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .find(|r| r.event.kind == "baska.olay")
        .unwrap();
    assert!(other.handled.is_some() && other.error.is_none());
    assert_eq!(other.note.as_deref(), Some("no workflow starts on it"));
}
