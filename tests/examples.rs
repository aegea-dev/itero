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
    let runs = String::from_utf8(itero(&ws, &["runs"]).stdout).unwrap();
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
    assert_eq!(read("sent.log"), "to ayşe@örnek.test: order 43, TR-43\n");
    assert_eq!(read("results.log"), "order 43: done\n");
}
