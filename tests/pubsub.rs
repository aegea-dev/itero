//! Publish and subscribe: events for the workspace reach the runs waiting
//! for them, and runs publish.
use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use serde_json::json;
use std::path::Path;

fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

fn workspace(d: &Path, flows: &[(&str, &str)]) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    for (name, text) in flows {
        std::fs::write(d.join(format!("workflows/{name}.json")), text).unwrap();
    }
    itero::workspace::find(d).unwrap()
}

fn emit(ws: &itero::workspace::Workspace, kind: &str, data: serde_json::Value) {
    itero::engine::events::emit(ws, kind, Some(data), None, "2026-10-16T10:00:00Z").unwrap();
}

/// A conversation per subject: started by `sohbet.basladi`, it waits for a
/// message on its own subject.
const SOHBET: &str = r#"{ "name": "sohbet", "description": "sohbet", "triggers": [ { "on": "sohbet.basladi" } ],
  "states": ["bekliyor", "cevaplandi"], "start": "bekliyor", "end": ["cevaplandi"],
  "transitions": [
    { "from": "bekliyor", "to": "cevaplandi", "on": "mesaj.geldi",
      "match": "event.data.konu == run.input.konu",
      "do": [ { "action": "memory.set@1", "with": { "son": "${input.metin}" } } ] } ] }"#;

#[test]
fn an_event_reaches_only_the_run_it_matches() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("sohbet", SOHBET)]);
    emit(&ws, "sohbet.basladi", json!({"konu": "çay"}));
    emit(&ws, "sohbet.basladi", json!({"konu": "kahve"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    emit(
        &ws,
        "mesaj.geldi",
        json!({"konu": "kahve", "metin": "şekersiz lütfen"}),
    );
    run_once(&ws, t("2026-10-16T10:00:02Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let mut states: Vec<(String, String)> = j
        .runs()
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.input["konu"].as_str().unwrap().to_string(),
                r.runtime.state,
            )
        })
        .collect();
    states.sort();
    assert_eq!(
        states,
        [
            ("kahve".to_string(), "cevaplandi".to_string()),
            ("çay".to_string(), "bekliyor".to_string())
        ]
    );
    assert_eq!(j.memory("sohbet").unwrap()["son"], "şekersiz lütfen");
}

/// Lives for good: every reading for its room is noted.
const DINLEYICI: &str = r#"{ "name": "dinleyici", "description": "dinleyici", "triggers": [ { "on": "oda.acildi" } ],
  "states": ["dinliyor"], "start": "dinliyor",
  "transitions": [
    { "from": "dinliyor", "to": "dinliyor", "on": "olcum.geldi",
      "match": "event.data.oda == run.input.oda",
      "do": [ { "action": "memory.set@1", "with": { "son": "${input.derece}" } } ] } ] }"#;

#[test]
fn a_long_lived_run_takes_every_event_and_stays() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("dinleyici", DINLEYICI)]);
    emit(&ws, "oda.acildi", json!({"oda": "mutfak"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    for (s, derece) in [(2, 19), (3, 21), (4, 23)] {
        emit(
            &ws,
            "olcum.geldi",
            json!({"oda": "mutfak", "derece": derece}),
        );
        emit(&ws, "olcum.geldi", json!({"oda": "salon", "derece": 30}));
        run_once(&ws, t(&format!("2026-10-16T10:00:0{s}Z")), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].runtime.state, "dinliyor");
    assert!(runs[0].ended.is_none(), "it lives on");
    assert_eq!(j.memory("dinleyici").unwrap()["son"], 23);
}

#[test]
fn an_event_nothing_waits_for_is_noted() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("sohbet", SOHBET)]);
    emit(&ws, "mesaj.geldi", json!({"konu": "çay"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let notes: Vec<String> = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter_map(|e| e.note)
        .collect();
    assert_eq!(notes, ["no workflow starts on it"]);
}

const RAPOR: &str = r#"{ "name": "rapor", "description": "rapor", "triggers": [ { "on": "ay.bitti" } ],
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "publish": "rapor.hazir", "data": { "ay": "${input.ay}" } } ] } ] }"#;

const DUYURU: &str = r#"{ "name": "duyuru", "description": "duyuru", "triggers": [ { "on": "rapor.hazir" } ],
  "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
  "transitions": [ { "from": "basla", "to": "bitti",
    "do": [ { "action": "memory.set@1", "with": { "ay": "${input.ay}" } } ] } ] }"#;

#[test]
fn a_published_event_starts_the_workflows_waiting_for_it() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("rapor", RAPOR), ("duyuru", DUYURU)]);
    emit(&ws, "ay.bitti", json!({"ay": "Ekim"}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    assert_eq!(j.memory("duyuru").unwrap()["ay"], "Ekim");
    let published = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .find(|e| e.event.kind == "rapor.hazir")
        .unwrap();
    assert!(
        published.event.source.starts_with("manager/"),
        "{}",
        published.event.source
    );
}

/// Listens to `yanki` and publishes `yanki` again: it must not hear itself.
const YANKI: &str = r#"{ "name": "yanki", "description": "yankı", "triggers": [ { "on": "yanki.basla" } ],
  "states": ["dinliyor"], "start": "dinliyor",
  "transitions": [ { "from": "dinliyor", "to": "dinliyor", "on": "yanki", "match": "true",
    "do": [ { "publish": "yanki", "data": { "kez": "tekrar" } } ] } ] }"#;

#[test]
fn a_run_never_hears_what_it_published() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), &[("yanki", YANKI)]);
    emit(&ws, "yanki.basla", json!({}));
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    emit(&ws, "yanki", json!({"kez": "ilk"}));
    for s in 2..6 {
        run_once(&ws, t(&format!("2026-10-16T10:00:0{s}Z")), exe()).unwrap();
    }
    let j = Journal::open(&ws).unwrap();
    let echoes = j
        .events(Some("itero.app"))
        .unwrap()
        .into_iter()
        .filter(|e| e.event.kind == "yanki")
        .count();
    assert_eq!(echoes, 2, "the first, and one answer to it; no loop");
}
