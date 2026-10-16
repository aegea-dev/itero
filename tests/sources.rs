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
