//! The built-in #load: the machine and itero, measured at once, the parts
//! asked for.
#![cfg(unix)]
use itero::engine::load::{PARTS, problems};
use itero::engine::worker::{Failure, LOAD, Outcome, StepRun, run_step};
use serde_json::{Value, json};
use std::path::Path;

fn workspace() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    d
}

fn load(dir: &Path, args: Value) -> Result<Outcome, Failure> {
    run_step(&StepRun {
        file: LOAD.into(),
        root: dir.to_path_buf(),
        workflow: "ölçüm".into(),
        env: Default::default(),
        input: json!({ "args": args }).as_object().cloned().unwrap(),
        memory: dir.join(".itero/memory/ölçüm"),
        waiting_since: None,
        quiet: 10,
        most: 10,
        now: "2026-11-26T10:00:00Z".into(),
    })
}

fn done(o: Result<Outcome, Failure>) -> Value {
    match o {
        Ok(Outcome::Done(v)) => v,
        other => panic!("not done: {other:?}"),
    }
}

fn number(v: &Value, at: &str) -> u64 {
    v.pointer(at)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{at}: {v}"))
}

#[test]
fn the_machines_parts_are_there_of_their_type_and_plausible() {
    let d = workspace();
    let v = done(load(
        d.path(),
        json!({ "measure": ["os", "cpu", "memory", "disk"] }),
    ));
    assert_eq!(v["os"]["name"], std::env::consts::OS, "{v}");
    assert_eq!(v["os"]["arch"], std::env::consts::ARCH, "{v}");
    assert!(
        v["os"]["version"].as_str().is_some_and(|s| !s.is_empty()),
        "{v}"
    );
    assert!(
        v["os"]["host"].as_str().is_some_and(|s| !s.is_empty()),
        "{v}"
    );
    assert!(number(&v, "/cpu/count") >= 1);
    let load = v["cpu"]["load"].as_array().unwrap();
    assert_eq!(load.len(), 3, "{v}");
    assert!(
        load.iter().all(|l| l.as_f64().is_some_and(|l| l >= 0.0)),
        "{v}"
    );
    let total = number(&v, "/memory/total");
    assert!(total > 0);
    assert!(
        number(&v, "/memory/used") <= total && number(&v, "/memory/free") <= total,
        "{v}"
    );
    let _ = number(&v, "/memory/swap_used");
    let disk = number(&v, "/disk/total");
    assert!(disk > 0 && number(&v, "/disk/free") <= disk, "{v}");
    assert!(v["at"].as_str().is_some(), "{v}");
    assert!(
        v["took"]["ms"].is_u64() && v["took"]["cpu_ms"].is_u64(),
        "{v}"
    );
}

#[test]
fn only_the_parts_asked_for_are_measured() {
    let d = workspace();
    let v = done(load(d.path(), json!({ "measure": ["memory", "memory"] })));
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["at", "memory", "took"]);
}

#[test]
fn without_measure_every_part_is_measured() {
    let d = workspace();
    let v = done(load(d.path(), json!({})));
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    let mut every = vec!["at"];
    every.extend(PARTS);
    every.push("took");
    assert_eq!(keys, every);
}

#[test]
fn wrong_parts_are_all_told_at_once() {
    assert_eq!(
        problems(&json!({ "measure": ["memroy", 3, "disk"] }), true),
        [
            r#"measure: "memroy" is not a part; did you mean "memory"?"#,
            "measure: a part is text, not 3",
        ]
    );
    assert_eq!(
        problems(&json!({ "measure": ["ağ"] }), true),
        [format!(
            r#"measure: "ağ" is not a part: {}"#,
            PARTS.join(", ")
        )]
    );
    assert_eq!(
        problems(&json!({ "measure": "memory" }), true),
        [format!(
            r#"measure: a list of parts ({}), not "memory""#,
            PARTS.join(", ")
        )]
    );
    assert_eq!(
        problems(&json!({ "measure": [] }), true),
        [format!("measure: at least one part ({})", PARTS.join(", "))]
    );
    // a value filled when the step runs is checked then
    assert!(problems(&json!({ "measure": "$event.parts" }), true).is_empty());
    assert!(!problems(&json!({ "measure": "$event.parts" }), false).is_empty());
}

#[test]
fn a_wrong_part_filled_at_run_time_fails_the_step_saying_it() {
    let d = workspace();
    match load(d.path(), json!({ "measure": ["bellek"] })) {
        Err(Failure::Error(why)) => assert!(
            why.starts_with(r#"#load: measure: "bellek" is not a part"#),
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
}
