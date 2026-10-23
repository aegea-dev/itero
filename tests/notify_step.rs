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
        workflow: "bildirim".into(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        state: Map::new(),
        memory: d.path().join("hafıza"),
        waiting_since: None,
        quiet: 10,
        most: 10,
        now: String::new(),
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

    // a url, opened on a click: http or https only, as it may come from a
    // message
    std::fs::remove_file(&log).unwrap();
    let url = "https://örnek.test/sipariş/43";
    worker::run_step(&step(&[("text", "43 kargoda"), ("url", url)])).unwrap();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("itero\t43 kargoda\turl={url}\n")
    );
    for bad in ["file:///etc/passwd", "javascript:alert(1)", "örnek.test"] {
        let e = worker::run_step(&step(&[("text", "çay"), ("url", bad)])).unwrap_err();
        assert!(e.to_string().contains("http or https"), "{bad}: {e}");
    }
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);
    assert!(worker::openable("http://localhost:3000/"));
    assert!(!worker::openable("HTTPS-but-not://x"));

    // a subtitle, and a sound: the default one, or one by name
    std::fs::remove_file(&log).unwrap();
    worker::run_step(&step(&[
        ("title", "Gmail"),
        ("subtitle", "Ayşe Yılmaz"),
        ("text", "Sipariş geldi"),
        ("sound", "default"),
    ]))
    .unwrap();
    worker::run_step(&step(&[("text", "çay"), ("sound", "Glass"), ("url", url)])).unwrap();
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!(
            "Gmail\tSipariş geldi\tsubtitle=Ayşe Yılmaz\tsound=default\nitero\tçay\tsound=Glass\turl={url}\n"
        )
    );
}
