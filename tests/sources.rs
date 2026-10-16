#![cfg(any(target_os = "macos", target_os = "linux"))]
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn the_watch_source_prints_a_debounced_event_and_ends_with_its_stdin() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "From ayşe\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["source", "watch@1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let settings = serde_json::json!({
        "name": "kutu", "emit": "posta.degisti",
        "with": { "path": f, "quiet": "200ms" }
    });
    writeln!(stdin, "{settings}").unwrap();
    std::thread::sleep(Duration::from_millis(500)); // the watch is set
    for _ in 0..5 {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&f)
            .unwrap()
            .write_all(b"From can\n")
            .unwrap();
    }
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let line = lines.next().unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["type"], "posta.degisti");
    assert_eq!(v["data"]["kind"], "changed");
    assert_eq!(v["data"]["path"], f.to_str().unwrap());
    drop(stdin);
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            break;
        }
        assert!(Instant::now() < end, "still running after its stdin closed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(lines.next().is_none(), "five appends, one event");
}

#[test]
fn the_watch_source_says_what_is_wrong_with_its_settings() {
    for (settings, says) in [
        (
            r#"{"name": "kutu", "emit": "a.b", "with": {}}"#,
            "watch@1 needs with.path",
        ),
        (
            r#"{"name": "kutu", "with": {"path": "/tmp"}}"#,
            "watch@1 needs emit",
        ),
        (
            r#"{"name": "kutu", "emit": "a.b", "with": {"path": "/tmp", "quiet": "yarım"}}"#,
            "with.quiet",
        ),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["source", "watch@1"])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{settings}").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(says), "{settings}: {err}");
    }
}

use itero::engine::daemon::Health;
use itero::engine::journal::Journal;
use itero::sources::supervise::{self, Line, Policy, Source};
use std::sync::Mutex;

fn quick() -> Policy {
    Policy {
        first_pause: Duration::from_millis(10),
        max_pause: Duration::from_millis(40),
        calm_after: Duration::from_secs(60),
        give_up_after: 3,
        window: Duration::from_secs(60),
    }
}

/// A workspace with one program source, `sources/deneme.json`, running
/// `sources/kaynak.sh`.
fn ws_with_source(d: &std::path::Path, script: &str, def: &str) -> itero::workspace::Workspace {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(d.join("itero.toml"), "").unwrap();
    std::fs::create_dir_all(d.join("sources")).unwrap();
    let s = d.join("sources/kaynak.sh");
    std::fs::write(&s, script).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(d.join("sources/deneme.json"), def).unwrap();
    itero::workspace::find(d).unwrap()
}

/// Runs the workspace's source until it is given up on.
fn run_it(ws: &itero::workspace::Workspace) -> Health {
    let (name, folder, def) = ws.sources().unwrap().remove(0);
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-16T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let source = Source {
        name: &name,
        folder: &folder,
        def: &def,
    };
    supervise::run(ws, exe, &source, &quick(), &wake, &health);
    health.into_inner().unwrap()
}

/// Prints its settings back as an event, a cursor and a bad line, then
/// fails: the supervisor starts it again until it gives up.
const ECHO: &str = "#!/bin/sh\nread -r settings\nprintf '{\"type\":\"deneme.basladi\",\"data\":{\"ayarlar\":%s}}\\n' \"$settings\"\nprintf '{\"cursor\":{\"sıra\":7}}\\n'\nprintf 'bozuk satır\\n'\nexit 1\n";

#[test]
fn lines_become_events_for_the_workspace_and_cursors_come_back() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_source(
        d.path(),
        ECHO,
        r#"{ "program": ["./kaynak.sh"], "with": { "hesap": "iş" } }"#,
    );
    let h = run_it(&ws);
    assert_eq!(h.sources["deneme"].state, "gave up");
    assert_eq!(h.sources["deneme"].restarts, 3);
    assert_eq!(
        h.sources["deneme"].last_exit.as_deref(),
        Some("exit status: 1")
    );
    let j = Journal::open(&ws).unwrap();
    let evs = j.events(Some("itero.app")).unwrap();
    assert_eq!(evs.len(), 4, "one event per start");
    assert_eq!(evs[0].event.source, "source/deneme");
    assert_eq!(evs[0].event.kind, "deneme.basladi");
    let first = &evs[0].event.data["ayarlar"];
    assert_eq!(first["name"], "deneme");
    assert_eq!(first["with"]["hesap"], "iş");
    assert!(first["cursor"].is_null(), "no cursor at first");
    assert_eq!(
        evs[1].event.data["ayarlar"]["cursor"]["sıra"], 7,
        "the cursor given back"
    );
    assert_eq!(j.cursor("deneme").unwrap().unwrap()["sıra"], 7);
}

#[test]
fn a_line_is_an_event_or_a_cursor_and_nothing_else() {
    use serde_json::json;
    assert_eq!(
        supervise::parse_line(r#"{"type":"a.b","data":{"x":1}}"#, None).unwrap(),
        Line::Event {
            kind: "a.b".into(),
            data: json!({"x": 1})
        }
    );
    assert_eq!(
        supervise::parse_line(r#"{"data":{"konu":"çay"}}"#, Some("posta.geldi")).unwrap(),
        Line::Event {
            kind: "posta.geldi".into(),
            data: json!({"konu": "çay"})
        }
    );
    assert_eq!(
        supervise::parse_line(r#"{"cursor":"ç"}"#, None).unwrap(),
        Line::Cursor(json!("ç"))
    );
    for (bad, says) in [
        ("düz yazı", "not JSON"),
        ("[1]", "not a JSON object"),
        (r#"{"data":{}}"#, "no type"),
        (r#"{"type":"run.started"}"#, "the engine's own"),
        (r#"{"type":"Büyük"}"#, "not an event type"),
        (r#"{"type":"a.b","data":[1]}"#, "data is an object"),
    ] {
        let e = supervise::parse_line(bad, None).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn a_secret_in_with_reaches_the_source_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_source(
        d.path(),
        ECHO,
        r#"{ "program": ["./kaynak.sh"], "with": { "parola": "${env.ITERO_TEST_KAYNAK_PAROLA}" } }"#,
    );
    // SAFETY: no other test reads or writes these variables
    unsafe {
        std::env::set_var("ITERO_TEST_KAYNAK_PAROLA", "gizli-şifre");
        std::env::set_var("ITERO_NO_SECRET_STORE", "1");
    }
    run_it(&ws);
    let j = Journal::open(&ws).unwrap();
    // this test source prints its settings back, so the event shows what it
    // got; the supervisor itself never writes settings anywhere
    let ev = &j.events(Some("itero.app")).unwrap()[0];
    assert_eq!(ev.event.data["ayarlar"]["with"]["parola"], "gizli-şifre");
}

#[test]
fn a_source_that_is_not_there_is_given_up_on_with_why() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_source(d.path(), ECHO, r#"{ "program": ["./yok.sh"] }"#);
    let h = run_it(&ws);
    let s = &h.sources["deneme"];
    assert_eq!(s.state, "gave up");
    assert!(
        s.last_exit.as_deref().unwrap().starts_with("error: "),
        "{s:?}"
    );
}
