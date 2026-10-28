//! `$$NAME`: a condition that holds once the secret or environment
//! variable is defined, and a value an action gets without the journal
//! ever holding it. Its own test binary: it sets environment variables.
#![cfg(unix)]
use itero::engine::journal::Journal;
use serde_json::json;
use std::path::Path;

const NOW: &str = "2026-10-21T10:00:00Z";

#[test]
fn a_secret_starts_nothing_until_defined_and_reaches_the_action_alone() {
    // SAFETY: the only test in this binary; no thread runs yet
    unsafe { std::env::set_var("ITERO_NO_SECRET_STORE", "1") };
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::create_dir_all(d.path().join("actions")).unwrap();
    std::fs::write(
        d.path().join("workflows/kapı.json"),
        json!({
            "on": [{ "event": "kapı.çal" }, { "has": "$$ITERO_TEST_ANAHTAR" }],
            "steps": [{"#aç": {"args": {"anahtar": "$$ITERO_TEST_ANAHTAR"}}}, "#kapat"]
        })
        .to_string(),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    // a secret in args joins the state as its name: the next step gets
    // its value too, on stdin as well
    for (name, body) in [
        ("aç", "echo \"$anahtar\" > açıldı.txt"),
        ("kapat", "echo \"$anahtar\" > kapandı.txt; cat > durum.json"),
    ] {
        let action = d.path().join("actions").join(name);
        std::fs::write(&action, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ws = itero::workspace::find(d.path()).unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    let pass = || itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();

    itero::engine::events::emit(&ws, "kapı.çal", Some(json!({})), NOW).unwrap();
    pass();
    assert!(
        Journal::open(&ws).unwrap().runs().unwrap().is_empty(),
        "not defined: no run"
    );

    // SAFETY: as above; the pass's workers have ended
    unsafe { std::env::set_var("ITERO_TEST_ANAHTAR", "gizli-şifre-123") };
    itero::engine::events::emit(&ws, "kapı.çal", Some(json!({})), NOW).unwrap();
    pass();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(itero::engine::steps::status(&runs[0].runtime), "done");
    for f in ["açıldı.txt", "kapandı.txt"] {
        assert_eq!(
            std::fs::read_to_string(d.path().join(f)).unwrap(),
            "gizli-şifre-123\n",
            "{f}"
        );
    }
    let stdin: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("durum.json")).unwrap())
            .unwrap();
    assert_eq!(stdin["anahtar"], "gizli-şifre-123");
    assert_eq!(runs[0].runtime.state["anahtar"], "$$ITERO_TEST_ANAHTAR");
    // nowhere in the journal, its write-ahead log, nor the log
    let wal = ws.journal().with_extension("sqlite-wal");
    let log = d.path().join(".itero/itero.log");
    assert!(log.exists());
    for f in [ws.journal(), wal, log] {
        let bytes = std::fs::read(&f).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("gizli-şifre-123"),
            "{} holds the secret",
            f.display()
        );
    }
    for e in j.events(None).unwrap() {
        assert!(
            !e.event.data.to_string().contains("gizli-şifre-123"),
            "{e:?}"
        );
    }
}

#[test]
fn an_action_gets_its_args_merged_and_its_repeat_item_on_top() {
    use itero::engine::steps::StepJob;
    let state = json!({"bölge": "tr"}).as_object().unwrap().clone();
    let job = |item: Option<serde_json::Value>| StepJob {
        path: vec![0],
        action: "imap-check".into(),
        version: None,
        data: serde_json::Map::new(),
        item,
        waiting_since: None,
        output: None,
    };
    let args = json!([{"host": "imap.örnek.test", "user": "kimse"}, {"title": "etiket", "text": "$bölge posta"}]);
    let env = itero::engine::daemon::bind(
        &state,
        Some(&args),
        &job(Some(json!({"user": "ayşe@örnek.test"}))),
    )
    .unwrap();
    assert_eq!(env["host"], "imap.örnek.test");
    assert_eq!(
        env["user"], "ayşe@örnek.test",
        "the item on top of the args"
    );
    assert_eq!(env["etiket"], "tr posta");
    let env = itero::engine::daemon::bind(&state, Some(&args), &job(None)).unwrap();
    assert_eq!(env["user"], "kimse");
}
