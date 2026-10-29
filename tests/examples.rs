//! The examples: each reads as a workspace, and the order example runs.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::process::Output;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// An example, copied into `d`.
fn copy(name: &str, d: &Path) -> PathBuf {
    let ws = d.join(name);
    let ok = std::process::Command::new("cp")
        .arg("-R")
        .arg(examples().join(name))
        .arg(&ws)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let _ = std::fs::remove_dir_all(ws.join(".itero"));
    ws
}

fn itero(ws: &Path, args: &[&str]) -> Output {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(ws)
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "{args:?}: {o:?}");
    o
}

#[test]
fn every_example_checks() {
    let d = tempfile::tempdir().unwrap();
    let mut names: Vec<String> = std::fs::read_dir(examples())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.len() >= 4, "{names:?}");
    for name in names {
        let ws = copy(&name, d.path());
        let out = String::from_utf8(itero(&ws, &["check"]).stdout).unwrap();
        assert!(!out.is_empty(), "{name}");
    }
}

#[test]
fn the_order_example_waits_for_its_payment_then_ships() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("order", d.path());
    itero(
        &ws,
        &[
            "emit",
            "order.placed",
            r#"{"order": 43, "email": "ayşe@örnek.test"}"#,
        ],
    );
    itero(&ws, &["daemon", "--once"]);
    let runs = String::from_utf8(itero(&ws, &["wf", "runs"]).stdout).unwrap();
    assert!(runs.contains("waiting"), "{runs}");
    itero(
        &ws,
        &[
            "emit",
            "payment.received",
            r#"{"payment_ref": "pay-43", "paid": true}"#,
        ],
    );
    itero(&ws, &["daemon", "--once"]);
    let read = |f: &str| std::fs::read_to_string(ws.join(f)).unwrap_or_default();
    assert_eq!(
        read(".itero/sent.log"),
        "to ayşe@örnek.test: order 43, TR-43\n"
    );
    assert_eq!(read(".itero/results.log"), "order 43: done\n");
}

/// An action of an example, run by hand: `env` and `stdin` given, what it
/// printed returned.
fn run_action(ws: &Path, name: &str, env: &[(&str, &str)], stdin: &str) -> String {
    use std::io::Write;
    let mut c = std::process::Command::new(ws.join("actions").join(name))
        .current_dir(ws)
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // an action may end without reading its stdin (check-sites does): a
    // broken pipe then is no failure, as it is none in itero's worker
    if let Err(e) = c.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "#{name}: {e}");
    }
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "#{name}: {o:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

#[test]
fn the_beacon_tells_only_what_changed() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("beacon", d.path());
    // its sites are a setting, a JSON list in $sites as a run gives it; a
    // port nothing listens on is down, without the network
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join("itero.config.json")).unwrap())
            .unwrap();
    assert!(
        manifest["workflow"]["sites"]["settings"]["sites"].is_array(),
        "{manifest}"
    );
    let sites = run_action(
        &ws,
        "check-sites",
        &[("sites", r#"["http://127.0.0.1:9"]"#)],
        "{}",
    );
    assert_eq!(sites, r#"{"http://127.0.0.1:9":"down"}"#);
    let memory = d.path().join("hafıza");
    let memory = memory.to_str().unwrap();
    // it reads the step before's result, $output, from the state on stdin
    let changed = |output: &str| {
        let state = format!(r#"{{"output": {output}}}"#);
        run_action(&ws, "changed", &[("ITERO_MEMORY", memory)], &state)
    };
    // the first run tells what it watches, then only changes
    assert_eq!(changed(&sites), r#"{"text":"http://127.0.0.1:9: down"}"#);
    assert_eq!(changed(&sites), "{}");
    let back = r#"{"http://127.0.0.1:9": "up", "disk": "ok"}"#;
    assert_eq!(
        changed(back),
        r#"{"text":"http://127.0.0.1:9: up\ndisk: ok"}"#
    );
    let disk = |min: &str| run_action(&ws, "check-disk", &[("min_free", min)], "{}");
    assert_eq!(disk("0"), r#"{"disk":"ok"}"#);
    assert_eq!(disk("101"), r#"{"disk":"under 101% free"}"#);
}

#[test]
fn the_beacon_without_jq_says_it_needs_jq() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("beacon", d.path());
    // a PATH with what the actions use, jq apart
    let bin = d.path().join("araç kutusu");
    std::fs::create_dir(&bin).unwrap();
    for p in ["curl", "df", "awk", "cat", "mv"] {
        let o = std::process::Command::new("sh")
            .args(["-c", &format!("command -v {p}")])
            .output()
            .unwrap();
        let at = String::from_utf8(o.stdout).unwrap();
        std::os::unix::fs::symlink(at.trim(), bin.join(p)).unwrap();
    }
    let memory = d.path().join("hafıza");
    for (name, env) in [
        ("check-sites", vec![("sites", r#"["http://127.0.0.1:9"]"#)]),
        ("check-disk", vec![]),
        ("changed", vec![("ITERO_MEMORY", memory.to_str().unwrap())]),
    ] {
        let o = std::process::Command::new(ws.join("actions").join(name))
            .current_dir(&ws)
            .env("PATH", &bin)
            .envs(env)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(127), "#{name}: {o:?}");
        assert_eq!(
            itero::needs::missing(127, &err).as_deref(),
            Some("needs jq: not found on PATH"),
            "#{name}: {err}"
        );
    }
}
