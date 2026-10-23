//! A source's args, `source.<name>.args` in the manifest: environment
//! variables for the source, `$$NAME` from the secret store or the
//! environment. Its own test binary: it sets environment variables.
#![cfg(any(target_os = "macos", target_os = "linux"))]
use itero::engine::daemon::Health;
use itero::engine::journal::Journal;
use itero::sources::supervise::{self, Policy, Source};
use std::sync::Mutex;
use std::time::Duration;

/// A workspace with one source, `sources/posta.geldi`, and its manifest.
fn ws_with(d: &std::path::Path, manifest: &str, script: &str) -> itero::workspace::Workspace {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(d.join("itero.config.json"), manifest).unwrap();
    std::fs::create_dir_all(d.join("sources")).unwrap();
    let s = d.join("sources/posta.geldi");
    std::fs::write(&s, script).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    itero::workspace::find(d).unwrap()
}

/// Runs the workspace's source until it is given up on: how it last ended.
fn run_it(ws: &itero::workspace::Workspace) -> String {
    let (kind, file) = ws.source_files().unwrap().remove(0);
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-21T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let w = Journal::open(ws).unwrap().workspace().unwrap();
    let log = itero::engine::log::Log::new(ws, &w.id);
    let policy = Policy {
        first_pause: Duration::from_millis(10),
        max_pause: Duration::from_millis(40),
        calm_after: Duration::from_secs(60),
        give_up_after: 1,
        window: Duration::from_secs(60),
    };
    let source = Source {
        kind: &kind,
        file: &file,
    };
    supervise::run(ws, exe, &source, &policy, &wake, &health, &log);
    let h = health.into_inner().unwrap();
    h.sources["posta.geldi"].last_exit.clone().unwrap()
}

const PRINTS: &str = "#!/bin/sh\nprintf '{\"user\": \"%s\", \"password\": \"%s\", \"count\": \"%s\", \"home\": \"%s\"}\\n' \"$user\" \"$password\" \"$count\" \"$ITERO_TEST_EV\"\nexit 1\n";

#[test]
fn a_source_gets_its_args_and_secrets_as_environment_variables() {
    // SAFETY: the only test in this binary; no thread runs yet
    unsafe {
        std::env::set_var("ITERO_NO_SECRET_STORE", "1");
        std::env::set_var("ITERO_TEST_POSTA_SIFRE", "gizli-şifre-123");
        std::env::set_var("ITERO_TEST_EV", "çatı katı");
    }

    // its args, a secret among them; the daemon's environment as well
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with(
        d.path(),
        r#"{"source": {"posta.geldi": {"args": {
            "user": "ayşe@örnek.test", "password": "$$ITERO_TEST_POSTA_SIFRE", "count": 3, "none": null}}}}"#,
        PRINTS,
    );
    run_it(&ws);
    let evs = Journal::open(&ws)
        .unwrap()
        .events(Some("itero.app"))
        .unwrap();
    let got = &evs
        .iter()
        .find(|e| e.event.kind == "posta.geldi")
        .unwrap()
        .event
        .data;
    assert_eq!(got["user"], "ayşe@örnek.test");
    assert_eq!(got["password"], "gizli-şifre-123");
    assert_eq!(got["count"], "3");
    assert_eq!(got["home"], "çatı katı");
    // the secret is in nothing itero writes itself
    let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).unwrap();
    assert!(!log.contains("gizli-şifre-123"), "{log}");

    // a secret that is not set: it does not start, and the log says why
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with(
        d.path(),
        r#"{"source": {"posta.geldi": {"args": {"password": "$$ITERO_TEST_YOK"}}}}"#,
        PRINTS,
    );
    let why = run_it(&ws);
    assert!(why.contains("ITERO_TEST_YOK is not set"), "{why}");
    let evs = Journal::open(&ws)
        .unwrap()
        .events(Some("itero.app"))
        .unwrap();
    assert!(evs.iter().all(|e| e.event.kind != "posta.geldi"), "{evs:?}");

    // a source has no run state: "$name" is refused, as is a name no
    // environment variable can have
    for args in [r#"{"user": "$kişi"}"#, r#"{"kullanıcı": "ayşe"}"#] {
        let d = tempfile::tempdir().unwrap();
        let ws = ws_with(
            d.path(),
            &format!(r#"{{"source": {{"posta.geldi": {{"args": {args}}}}}}}"#),
            PRINTS,
        );
        let why = run_it(&ws);
        assert!(why.contains("source.posta.geldi.args"), "{args}: {why}");
    }
}
