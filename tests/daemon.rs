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

#[test]
fn a_schedule_keeps_its_last_slot_when_triggers_are_reordered() {
    let d = tempfile::tempdir().unwrap();
    let flow = |triggers: &str| {
        format!(
            r#"{{ "name": "sayac", "triggers": {triggers},
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ {{ "from": "basla", "to": "bitti",
        "do": [ {{ "action": "memory.set@1", "with": {{ "son": "çalıştı" }} }} ] }} ] }}"#
        )
    };
    let ws = workspace(
        d.path(),
        "sayac",
        &flow(r#"[ { "schedule": "*/5 * * * *" } ]"#),
    );
    run_once(&ws, t("2026-10-15T14:01:00Z"), exe()).unwrap();
    // an event trigger put first: the schedule's place changes, its key does not
    std::fs::write(
        d.path().join("workflows/sayac.json"),
        flow(r#"[ { "on": "elle" }, { "schedule": "*/5 * * * *" } ]"#),
    )
    .unwrap();
    run_once(&ws, t("2026-10-15T14:05:10Z"), exe()).unwrap();
    assert_eq!(
        Journal::open(&ws).unwrap().runs().unwrap().len(),
        1,
        "the 14:05 slot ran"
    );
}

/// A workflow started by `dosya.geldi` that runs a shell line, at most one
/// run at a time.
fn yavas(then: &str) -> String {
    format!(
        r#"{{ "name": "yavas", "description": "yavaş iş", "triggers": [ {{ "on": "dosya.geldi" }} ],
             "concurrency": {{ "max": 1, "then": "{then}" }},
             "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ {{ "from": "basla", "to": "bitti",
               "do": [ {{ "action": "sh@1", "with": {{ "run": "true" }} }} ] }} ] }}"#
    )
}

fn arrives(ws: &itero::workspace::Workspace) {
    itero::engine::events::emit(
        ws,
        "dosya.geldi",
        Some(serde_json::json!({"ad": "ağaç.txt"})),
        None,
        "2026-10-16T10:00:00Z",
    )
    .unwrap();
}

#[test]
fn a_run_keeps_the_input_it_started_with() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    arrives(&ws);
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs[0].input, serde_json::json!({"ad": "ağaç.txt"}));
}

#[test]
fn starts_beyond_the_maximum_wait_and_run_in_turn() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    arrives(&ws);
    arrives(&ws);
    // the first run starts and finishes; the second came while the first
    // was under way, so it waits for the next pass
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.runs().unwrap().len(), 1);
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(runs.iter().all(|r| r.runtime.state == "bitti"), "{runs:?}");
    assert_eq!(runs[1].started, "2026-10-16T10:00:02Z");
}

#[test]
fn starts_beyond_the_maximum_are_dropped_when_the_workflow_skips() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("skip"));
    arrives(&ws);
    arrives(&ws);
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.runs().unwrap().len(), 1);
    let notes: Vec<String> = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter_map(|e| e.note)
        .collect();
    assert_eq!(notes, ["yavas: skipped"]);
}

#[test]
fn a_new_version_of_a_paused_workflow_resumes_it() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    let mut j = Journal::open(&ws).unwrap();
    j.handle(itero::engine::journal::Handled {
        seq: 0,
        at: "2026-10-16T10:00:00Z",
        error: None,
        run: None,
        timers: None,
        events: vec![],
        memory: None,
        pause: Some(itero::engine::journal::Pause {
            workflow: "yavas",
            pin: "yavas@eski-sürüm",
            why: "deneme".into(),
        }),
    })
    .unwrap();
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    assert!(
        j.paused().unwrap().is_empty(),
        "the workflow now is not the version that failed"
    );
}

#[test]
fn the_same_version_stays_paused() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "yavas", &yavas("queue"));
    let f = ws.workflows().unwrap().remove(0).1;
    let pin = itero::workflow::pin(&f);
    let mut j = Journal::open(&ws).unwrap();
    j.handle(itero::engine::journal::Handled {
        seq: 0,
        at: "2026-10-16T10:00:00Z",
        error: None,
        run: None,
        timers: None,
        events: vec![],
        memory: None,
        pause: Some(itero::engine::journal::Pause {
            workflow: "yavas",
            pin: &pin,
            why: "deneme".into(),
        }),
    })
    .unwrap();
    arrives(&ws);
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    assert_eq!(j.paused().unwrap().len(), 1);
    assert!(j.runs().unwrap().is_empty(), "paused: nothing starts");
}
