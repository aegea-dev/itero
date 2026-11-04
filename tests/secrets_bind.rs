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
fn an_action_gets_its_steps_args_and_a_waited_event() {
    use itero::engine::steps::StepJob;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let job = |event: Option<serde_json::Value>| StepJob {
        path: vec![0],
        action: "imap-check".into(),
        version: None,
        data: json!({"host": "imap.örnek.test", "user": "ayşe@örnek.test"})
            .as_object()
            .unwrap()
            .clone(),
        secrets: vec![],
        waiting_since: None,
        event,
    };
    let (env, input) = itero::engine::daemon::bind(&ws, &job(None)).unwrap();
    assert_eq!(env["host"], "imap.örnek.test");
    assert_eq!(env["user"], "ayşe@örnek.test");
    assert_eq!(input["args"]["host"], "imap.örnek.test");
    assert!(input.get("event").is_none() && !env.contains_key("event"));
    // run again after waiting: the event that came, as $event and on stdin
    let came = json!({"order": 7, "amount": 25});
    let (env, input) = itero::engine::daemon::bind(&ws, &job(Some(came.clone()))).unwrap();
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
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let get = |n: &str| itero::engine::secrets::get_in(&ws, n);
    assert_eq!(get("KLASOR").as_deref(), Some("~/Belgeler"));
    assert_eq!(get("BOYUT").as_deref(), Some("1M"), "export and quotes");
    assert_eq!(get("YOK"), None);
    // the environment after .env
    // SAFETY: no thread of this test binary reads the environment now
    unsafe { std::env::set_var("ITERO_TEST_ORTAM_DEGERI", "ortamdan") };
    assert_eq!(get("ITERO_TEST_ORTAM_DEGERI").as_deref(), Some("ortamdan"));
}

/// Only a `$$NAME` written in the workflow is a secret. One that comes in
/// data, however it comes, is plain text, and the run goes on: anyone who
/// can put text in an event (a mail's subject) must not be able to name a
/// secret, nor to break a run by trying.
#[test]
fn a_secret_named_by_data_is_only_text() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    std::fs::write(root.join(".env"), "ITERO_TEST_GIZLI=gizli-değer-ı\n").unwrap();
    std::fs::create_dir_all(root.join("workflows")).unwrap();
    std::fs::create_dir_all(root.join("actions")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for (name, body) in [
        ("söyle", "printf '%s' \"$text\""),
        ("yaz", "printf '{\"text\": \"$$ITERO_TEST_GIZLI\"}'"),
    ] {
        let action = root.join("actions").join(name);
        std::fs::write(&action, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let say = |args: serde_json::Value| json!({"#söyle": {"args": args}});
    let written = "$$ITERO_TEST_GIZLI";
    for (name, steps, said) in [
        ("tam", json!([say(json!({"text": "$event.x"}))]), written),
        (
            "metin",
            json!([say(json!({"text": "konu: $event.x"}))]),
            "konu: $$ITERO_TEST_GIZLI",
        ),
        (
            "birlesik",
            json!([say(json!({"text": "$event.a$event.b"}))]),
            written,
        ),
        (
            "cift",
            json!([say(json!([{"title": "text", "text": "$event.x"}]))]),
            written,
        ),
        (
            "liste",
            json!([say(json!([{"text": "$event.x"}]))]),
            written,
        ),
        (
            "tum",
            json!(["#yaz", {"#söyle": {"args": "$output"}}]),
            written,
        ),
        (
            "adim",
            json!(["#yaz", say(json!({"text": "$steps.yaz.text"}))]),
            written,
        ),
        (
            "yan",
            json!([
                [say(json!({"text": "$event.x"})), "#yaz"],
                say(json!({"text": "$steps.söyle"}))
            ]),
            written,
        ),
    ] {
        std::fs::write(
            root.join("workflows").join(format!("{name}.json")),
            json!({"on": [{"event": format!("deneme.{name}")}], "steps": steps}).to_string(),
        )
        .unwrap();
        let ws = itero::workspace::find(root).unwrap();
        itero::engine::events::emit(
            &ws,
            &format!("deneme.{name}"),
            Some(json!({"x": written, "a": "$$ITERO_TEST", "b": "_GIZLI"})),
            NOW,
        )
        .unwrap();
        let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
        itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
        let j = Journal::open(&ws).unwrap();
        let runs = j.runs().unwrap();
        let run = runs.iter().find(|r| r.workflow == name).expect(name);
        assert_eq!(
            itero::engine::steps::status(&run.runtime),
            "done",
            "{name}: {:?}",
            run.runtime.why
        );
        assert_eq!(run.runtime.state["output"], said, "{name}");
    }
    let ws = itero::workspace::find(root).unwrap();
    let wal = ws.journal().with_extension("sqlite-wal");
    for f in [ws.journal(), wal, root.join(".itero/itero.log")] {
        let bytes = std::fs::read(&f).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("gizli-değer"),
            "{} holds the secret",
            f.display()
        );
    }
}

#[test]
fn bind_gives_the_secret_only_to_the_args_written_as_one() {
    use itero::engine::steps::StepJob;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    std::fs::write(d.path().join(".env"), "ITERO_TEST_ANAHTAR2=değer\n").unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let job = StepJob {
        path: vec![0],
        action: "a".into(),
        version: None,
        data: json!({"yazılı": "$$ITERO_TEST_ANAHTAR2", "gelen": "$$ITERO_TEST_ANAHTAR2"})
            .as_object()
            .unwrap()
            .clone(),
        secrets: vec!["yazılı".into()],
        waiting_since: None,
        event: None,
    };
    let (_, input) = itero::engine::daemon::bind(&ws, &job).unwrap();
    assert_eq!(input["args"]["yazılı"], "değer");
    assert_eq!(input["args"]["gelen"], "$$ITERO_TEST_ANAHTAR2");
}
