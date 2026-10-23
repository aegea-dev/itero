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
            "steps": [{"#aç": {"anahtar": "$$ITERO_TEST_ANAHTAR"}}]
        })
        .to_string(),
    )
    .unwrap();
    let action = d.path().join("actions/aç");
    std::fs::write(&action, "#!/bin/sh\necho \"$anahtar\" > açıldı.txt\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
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
    assert_eq!(
        std::fs::read_to_string(d.path().join("açıldı.txt")).unwrap(),
        "gizli-şifre-123\n"
    );
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
