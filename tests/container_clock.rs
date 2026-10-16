//! A container job's time limit starts once its image is here. A stand-in
//! `docker`, first on `PATH`, takes longer to pull than the job's whole
//! time limit, so this needs no Docker and does not depend on its speed.
//! Its own test binary, since it changes `PATH` for the whole process.
#![cfg(unix)]
use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

/// `docker`: no image here, a pull that takes 3 seconds, and a run that
/// reads its job and prints its answer.
const DOCKER: &str = r#"#!/bin/sh
case "$1" in
  image) exit 1 ;;
  pull) sleep 3 ;;
  rm) ;;
  run) cat >/dev/null; echo '{"hazır": true}' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

#[test]
fn getting_the_image_does_not_count_against_the_time_limit() {
    let bin = tempfile::tempdir().unwrap();
    write(bin.path(), "docker", DOCKER);
    let docker = bin.path().join("docker");
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // SAFETY: the only test in this binary, and no other thread is running
    // yet: the workers it starts inherit this PATH
    unsafe { std::env::set_var("PATH", path) };

    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(
        d.path(),
        "actions/kutu/action.json",
        r#"{ "name": "kutu", "version": 1, "type": "container", "image": "örnek/yavaş:1",
             "limits": { "timeout": "2s" },
             "targets": { "calis": { "executable": "true" } } }"#,
    );
    write(
        d.path(),
        "workflows/kutu.json",
        r#"{ "name": "kutu", "triggers": [ { "on": "kutu.calis" } ],
             "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ { "from": "basla", "to": "bitti",
               "do": [ { "id": "is", "action": "kutu.calis@1" },
                       { "action": "memory.set@1", "with": { "cikti": "${steps.is.json}" } } ] } ] }"#,
    );
    let ws = itero::workspace::find(d.path()).unwrap();
    itero::engine::events::emit(&ws, "kutu.calis", None, None, "2026-10-16T10:00:00Z").unwrap();
    run_once(
        &ws,
        t("2026-10-16T10:00:01Z"),
        Path::new(env!("CARGO_BIN_EXE_itero")),
    )
    .unwrap();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs[0].runtime.state, "bitti", "{:?}", runs[0]);
    assert_eq!(j.memory("kutu").unwrap()["cikti"]["hazır"], true);
}
