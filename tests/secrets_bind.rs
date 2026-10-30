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
            "steps": [
                {"#aç": {"args": {"anahtar": "$$ITERO_TEST_ANAHTAR"}}},
                {"#kapat": {"args": {"anahtar": "$$ITERO_TEST_ANAHTAR"}}}
            ]
        })
        .to_string(),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    // each step that is given the secret gets its value, on stdin as well
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
    assert_eq!(stdin["args"]["anahtar"], "gizli-şifre-123");
    // the run's state holds no args at all
    assert!(runs[0].runtime.state.get("anahtar").is_none());
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
fn an_action_gets_its_steps_args_with_its_repeat_item_on_top_and_a_waited_event() {
    use itero::engine::steps::StepJob;
    let d = tempfile::tempdir().unwrap();
    let state = json!({"event": {"bölge": "tr"}})
        .as_object()
        .unwrap()
        .clone();
    let job = |item: Option<serde_json::Value>, event: Option<serde_json::Value>| StepJob {
        path: vec![0],
        action: "imap-check".into(),
        version: None,
        data: json!({"host": "imap.örnek.test"})
            .as_object()
            .unwrap()
            .clone(),
        item,
        waiting_since: None,
        event,
    };
    let item = json!({"user": "ayşe@örnek.test", "host": "başka", "etiket": "$event.bölge posta"});
    let (env, input) =
        itero::engine::daemon::bind(d.path(), &state, &job(Some(item), None)).unwrap();
    assert_eq!(env["host"], "başka", "the item on top of the step's args");
    assert_eq!(env["user"], "ayşe@örnek.test");
    assert_eq!(env["etiket"], "tr posta");
    assert_eq!(input["args"]["host"], "başka");
    assert!(input.get("event").is_none() && !env.contains_key("event"));
    // nothing else of the run reaches it
    assert!(!env.contains_key("bölge"));
    // run again after waiting: the event that came, as $event and on stdin
    let came = json!({"order": 7, "amount": 25});
    let (env, input) =
        itero::engine::daemon::bind(d.path(), &state, &job(None, Some(came.clone()))).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&env["event"]).unwrap(),
        came
    );
    assert_eq!(input["event"], came);
}

#[test]
fn a_value_comes_from_dot_env_after_the_secret_store() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(".env"),
        "# kişisel değerler\nKLASOR=~/Belgeler\nexport BOYUT=\"1M\"\n",
    )
    .unwrap();
    let get = |n: &str| itero::engine::secrets::get_in(d.path(), n);
    assert_eq!(get("KLASOR").as_deref(), Some("~/Belgeler"));
    assert_eq!(get("BOYUT").as_deref(), Some("1M"), "export and quotes");
    assert_eq!(get("YOK"), None);
    // the environment after .env
    // SAFETY: no thread of this test binary reads the environment now
    unsafe { std::env::set_var("ITERO_TEST_ORTAM_DEGERI", "ortamdan") };
    assert_eq!(get("ITERO_TEST_ORTAM_DEGERI").as_deref(), Some("ortamdan"));
}
