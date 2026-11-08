//! Windows in a workflow's `on`: across events, which start a run. Passes
//! run in this process at the times given, so nothing waits on a clock.
#![cfg(unix)]
mod common;
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use serde_json::{Value, json};
use std::path::Path;

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

/// The built-in workflows off: only the test's own run.
const MANIFEST: &str =
    r#"{"workflow": {"keep": {"off": true}, "logs": {"off": true}, "models": {"off": true}}}"#;

struct Ws {
    d: tempfile::TempDir,
    ws: itero::workspace::Workspace,
}

impl Ws {
    /// A workspace with the workflow `bildir`, on `sayfa.değişti` and the
    /// windows given, whose one step prints nothing.
    fn with(windows: Value) -> Ws {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.config.json"), MANIFEST).unwrap();
        let mut on = vec![json!({ "event": "sayfa.değişti" })];
        on.extend(windows.as_array().unwrap().iter().cloned());
        std::fs::create_dir_all(d.path().join("workflows")).unwrap();
        std::fs::write(
            d.path().join("workflows/bildir.json"),
            json!({ "on": on, "steps": ["#sessiz"] }).to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(d.path().join("actions")).unwrap();
        let a = d.path().join("actions/sessiz");
        std::fs::write(&a, "#!/bin/sh\necho '{}'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ws = itero::workspace::find(d.path()).unwrap();
        Ws { d, ws }
    }

    /// An event, routed by a pass at `now`.
    fn event(&self, data: Value, now: &str) {
        itero::engine::events::emit(&self.ws, "sayfa.değişti", Some(data), now).unwrap();
        run_once(&self.ws, now.parse().unwrap(), exe()).unwrap();
    }

    /// The `$event` of each run started, in order.
    fn started(&self) -> Vec<Value> {
        let mut runs = Journal::open(&self.ws).unwrap().runs().unwrap();
        runs.retain(|r| r.workflow == "bildir");
        runs.sort_by(|a, b| a.started.cmp(&b.started).then(a.id.cmp(&b.id)));
        runs.iter().map(|r| r.input.clone()).collect()
    }

    /// The log's `message` lines that held an event back from `bildir`.
    fn held(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.d.path().join(".itero/itero.log")).unwrap();
        common::log_lines(&text)
            .into_iter()
            .filter(|l| l["event"] == "message")
            .filter_map(|l| l["held"].as_array().cloned())
            .flatten()
            .collect()
    }
}

#[test]
fn unique_starts_a_run_once_per_value() {
    let ws = Ws::with(json!([{ "unique": "$event.kimlik" }]));
    for (k, now) in [(1, "10:00"), (1, "10:01"), (2, "10:02"), (1, "10:03")] {
        ws.event(json!({ "kimlik": k }), &format!("2026-11-08T{now}:00Z"));
    }
    assert_eq!(ws.started(), [json!({"kimlik": 1}), json!({"kimlik": 2})]);
    let held = ws.held();
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(held[0]["workflow"], "bildir");
    assert!(
        held[0]["why"].as_str().unwrap().starts_with("unique:"),
        "{held:?}"
    );
}

#[test]
fn unique_for_a_while_lets_a_value_through_again_after_it() {
    let ws = Ws::with(json!([{ "unique": "$event.kimlik", "for": "1h" }]));
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T10:00:00Z");
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T10:59:59Z");
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T11:00:01Z");
    assert_eq!(ws.started().len(), 2);
}

#[test]
fn changed_starts_a_run_when_the_value_is_not_the_last_runs() {
    let ws = Ws::with(json!([{ "changed": "$event.durum" }]));
    for (i, durum) in ["açık", "açık", "kapalı", "açık", "açık"]
        .iter()
        .enumerate()
    {
        ws.event(
            json!({ "durum": durum }),
            &format!("2026-11-08T10:0{i}:00Z"),
        );
    }
    let got: Vec<Value> = ws.started().iter().map(|e| e["durum"].clone()).collect();
    assert_eq!(got, ["açık", "kapalı", "açık"]);
}

#[test]
fn at_most_starts_one_run_per_period() {
    let ws = Ws::with(json!([{ "at_most": "1h" }]));
    ws.event(json!({ "n": 1 }), "2026-11-08T10:00:00Z");
    ws.event(json!({ "n": 2 }), "2026-11-08T10:30:00Z");
    ws.event(json!({ "n": 3 }), "2026-11-08T11:00:01Z");
    assert_eq!(ws.started(), [json!({"n": 1}), json!({"n": 3})]);
}

#[test]
fn per_keeps_a_window_for_each_value() {
    let ws = Ws::with(json!([{ "at_most": "1h", "per": "$event.site" }]));
    for (site, now) in [
        ("bir", "10:00"),
        ("iki", "10:01"),
        ("bir", "10:02"),
        ("iki", "10:03"),
    ] {
        ws.event(json!({ "site": site }), &format!("2026-11-08T{now}:00Z"));
    }
    assert_eq!(
        ws.started(),
        [json!({"site": "bir"}), json!({"site": "iki"})]
    );
}

#[test]
fn every_window_must_let_an_event_through() {
    // a value seen again is held back by unique even in a new period
    let ws = Ws::with(json!([
        { "unique": "$event.kimlik" },
        { "at_most": "1h" }
    ]));
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T10:00:00Z");
    // held back by at_most: unique does not count it as seen
    ws.event(json!({ "kimlik": 2 }), "2026-11-08T10:10:00Z");
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T12:00:00Z");
    ws.event(json!({ "kimlik": 2 }), "2026-11-08T12:10:00Z");
    assert_eq!(ws.started(), [json!({"kimlik": 1}), json!({"kimlik": 2})]);
}

#[test]
fn a_window_keeps_its_state_in_the_journal() {
    let ws = Ws::with(json!([{ "unique": "$event.kimlik" }]));
    ws.event(json!({ "kimlik": 1 }), "2026-11-08T10:00:00Z");
    // another pass, as a daemon started again makes: the state is the journal's
    let again = itero::workspace::find(ws.d.path()).unwrap();
    itero::engine::events::emit(
        &again,
        "sayfa.değişti",
        Some(json!({ "kimlik": 1 })),
        "2026-11-08T10:05:00Z",
    )
    .unwrap();
    run_once(&again, "2026-11-08T10:05:00Z".parse().unwrap(), exe()).unwrap();
    assert_eq!(ws.started().len(), 1);
}

#[test]
fn what_a_window_no_longer_holds_back_for_is_forgotten() {
    let ws = Ws::with(json!([
        { "unique": "$event.kimlik", "for": "1h" },
        { "changed": "$event.durum" }
    ]));
    ws.event(
        json!({ "kimlik": 1, "durum": "açık" }),
        "2026-11-08T10:00:00Z",
    );
    let j = Journal::open(&ws.ws).unwrap();
    assert_eq!(j.forget_seen("2026-11-08T10:59:59Z").unwrap(), 0);
    // the unique value, past its hour; the last value changed keeps
    assert_eq!(j.forget_seen("2026-11-08T11:00:00Z").unwrap(), 1);
    ws.event(
        json!({ "kimlik": 1, "durum": "açık" }),
        "2026-11-08T11:00:01Z",
    );
    assert_eq!(
        ws.started().len(),
        1,
        "changed still holds the same value back"
    );
}
