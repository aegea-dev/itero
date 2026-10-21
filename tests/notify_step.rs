//! The built-in `#notify`. Its own test binary: it sets
//! `ITERO_NOTIFY_LOG` for the whole process.
#![cfg(unix)]
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::Map;
use std::path::PathBuf;

#[test]
fn the_built_in_notify_shows_text() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    // SAFETY: the only test in this binary, before any thread starts
    unsafe { std::env::set_var("ITERO_NOTIFY_LOG", &log) };
    let step = |env: &[(&str, &str)]| StepRun {
        file: PathBuf::from(worker::NOTIFY),
        root: d.path().to_path_buf(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        state: Map::new(),
        memory: d.path().join("hafıza"),
        waiting_since: None,
        timeout: 10,
    };
    assert_eq!(
        worker::run_step(&step(&[("text", "Ayşe: Toplantı")])).unwrap(),
        Outcome::Done(Map::new())
    );
    worker::run_step(&step(&[("title", "Sipariş"), ("text", "43 kargoda")])).unwrap();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "itero\tAyşe: Toplantı\nSipariş\t43 kargoda\n"
    );
    let e = worker::run_step(&step(&[])).unwrap_err();
    assert!(e.to_string().contains("$text"), "{e}");
}
