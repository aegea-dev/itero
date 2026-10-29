//! Connectors: executables in `connectors/` that keep running, each line they
//! print the data of one message of their file name's type; and `itero
//! watch`, for connectors that watch a file.
#![cfg(any(target_os = "macos", target_os = "linux"))]
use itero::connectors::supervise::{self, Connector, Policy};
use itero::engine::daemon::Health;
use itero::engine::journal::Journal;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[test]
fn itero_watch_prints_a_debounced_line_and_ends_with_its_stdin() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "From ayşe\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("watch")
        .arg(&f)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
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
    let v: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(v["path"], f.to_str().unwrap());
    assert_eq!(v["kind"], "changed");
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
    assert!(lines.next().is_none(), "five appends, one line");
}

fn quick() -> Policy {
    Policy {
        first_pause: Duration::from_millis(10),
        max_pause: Duration::from_millis(40),
        calm_after: Duration::from_secs(60),
        give_up_after: 3,
        window: Duration::from_secs(60),
    }
}

/// A workspace with one connector, `connectors/deneme.başladı`.
fn ws_with_connector(d: &std::path::Path, script: &str) -> itero::workspace::Workspace {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(d.join("itero.config.json"), "{}").unwrap();
    std::fs::create_dir_all(d.join("connectors")).unwrap();
    let s = d.join("connectors/deneme.başladı");
    std::fs::write(&s, script).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    itero::workspace::find(d).unwrap()
}

fn log(ws: &itero::workspace::Workspace) -> itero::engine::log::Log {
    let w = Journal::open(ws).unwrap().workspace().unwrap();
    itero::engine::log::Log::new(ws, &w.id)
}

/// Runs the workspace's connector until it is given up on.
fn run_it(ws: &itero::workspace::Workspace) -> Health {
    let (kind, file) = ws.connector_files().unwrap().remove(0);
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-21T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let connector = Connector {
        kind: &kind,
        file: &file,
    };
    supervise::run(
        ws,
        exe,
        &connector,
        &quick(),
        &wake,
        &health,
        &log(ws),
        &Default::default(),
    );
    health.into_inner().unwrap()
}

#[test]
fn each_line_is_a_message_of_the_connectors_type() {
    let d = tempfile::tempdir().unwrap();
    // `itero` is the daemon's own, found first on its PATH
    let ws = ws_with_connector(
        d.path(),
        "#!/bin/sh\nprintf '{\"konu\": \"çay\"}\\n'\nprintf 'bozuk satır\\n'\nprintf '{\"yer\": \"%s\"}\\n' \"$(pwd)\"\ncommand -v itero >/dev/null && echo '{\"itero\": true}'\nexit 1\n",
    );
    let h = run_it(&ws);
    assert_eq!(h.connectors["deneme.başladı"].state, "gave up");
    let j = Journal::open(&ws).unwrap();
    let mut evs = j.events(Some("itero.app")).unwrap();
    // the last is the daemon's own: the connector was given up on
    assert_eq!(evs.pop().unwrap().event.kind, "itero.connector.gave-up");
    // three lines a start, one bad; four starts before it is given up on
    assert_eq!(evs.len(), 3 * 4, "{evs:?}");
    assert!(evs.iter().all(|e| e.event.kind == "deneme.başladı"));
    assert_eq!(evs[0].event.data["konu"], "çay");
    let here = std::fs::canonicalize(d.path()).unwrap();
    assert_eq!(evs[1].event.data["yer"], here.display().to_string());
    assert_eq!(evs[2].event.data["itero"], true);
}

#[test]
fn a_line_is_one_json_object() {
    assert_eq!(
        supervise::parse_line(r#"{"konu": "çay"}"#).unwrap()["konu"],
        "çay"
    );
    for bad in ["bozuk", "[1]", r#""çay""#] {
        assert!(supervise::parse_line(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_connector_that_cannot_start_is_given_up_on_with_why() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_connector(d.path(), "#!/bin/sh\n");
    let (kind, file) = ws.connector_files().unwrap().remove(0);
    std::fs::remove_file(&file).unwrap();
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-21T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let connector = Connector {
        kind: &kind,
        file: &file,
    };
    supervise::run(
        &ws,
        exe,
        &connector,
        &quick(),
        &wake,
        &health,
        &log(&ws),
        &Default::default(),
    );
    let h = health.into_inner().unwrap();
    let s = &h.connectors["deneme.başladı"];
    assert_eq!(s.state, "gave up");
    assert!(
        s.last_exit.as_deref().unwrap().starts_with("error: "),
        "{s:?}"
    );
    let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).unwrap();
    let last: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(last["event"], "connector.gave-up");
    assert_eq!(last["connector"], "deneme.başladı");
    let own = Journal::open(&ws)
        .unwrap()
        .events(Some("itero.app"))
        .unwrap();
    let gave_up = own.last().unwrap();
    assert_eq!(gave_up.event.kind, "itero.connector.gave-up");
    assert_eq!(gave_up.event.data["connector"], "deneme.başladı");
}

#[test]
fn after_an_end_the_pause_doubles_resets_when_calm_and_gives_up() {
    let p = Policy {
        first_pause: Duration::from_secs(1),
        max_pause: Duration::from_secs(4),
        calm_after: Duration::from_secs(300),
        give_up_after: 3,
        window: Duration::from_secs(600),
    };
    let now = Instant::now();
    let mut ends = vec![];
    let quick = Duration::from_secs(1);
    let mut pause = Duration::ZERO;
    let mut pauses = vec![];
    for _ in 0..3 {
        pause = p.after_end(&mut ends, now, quick, pause).unwrap();
        pauses.push(pause.as_secs());
    }
    assert_eq!(pauses, vec![1, 2, 4]);
    // the fourth end within the window: given up
    assert_eq!(p.after_end(&mut ends, now, quick, pause), None);
    // a calm run sets the pause back; ends past the window are forgotten
    let later = now + Duration::from_secs(700);
    let mut ends = vec![now, now];
    assert_eq!(
        p.after_end(
            &mut ends,
            later,
            Duration::from_secs(400),
            Duration::from_secs(4)
        ),
        Some(Duration::from_secs(1))
    );
    assert_eq!(ends.len(), 1);
}
