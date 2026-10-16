//! Container actions, in Docker: run by `make test-docker`, which sets
//! `ITERO_DOCKER`. Without it these tests say so and pass: `make test`
//! needs no Docker.
#![cfg(unix)]
use chrono::{DateTime, Utc};
use itero::engine::daemon::run_once;
use itero::engine::journal::Journal;
use std::path::Path;

fn docker() -> bool {
    let on = std::env::var_os("ITERO_DOCKER").is_some();
    if !on {
        eprintln!("ITERO_DOCKER is not set: skipped (make test-docker runs it)");
    }
    on
}

fn t(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

/// A workspace whose workflow `kutu` runs `kutu.calis@1` once, on an event.
fn workspace(d: &Path, action: &str) -> itero::workspace::Workspace {
    write(d, "itero.toml", "");
    write(d, "actions/kutu/action.json", action);
    write(
        d,
        "workflows/kutu.json",
        r#"{ "name": "kutu", "description": "kutuda çalış", "triggers": [ { "on": "kutu.calis" } ],
             "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ { "from": "basla", "to": "bitti",
               "do": [ { "id": "is", "action": "kutu.calis@1", "with": { "selam": "merhaba dünya" } },
                       { "action": "memory.set@1", "with": { "cikti": "${steps.is.json}" } } ] } ] }"#,
    );
    let ws = itero::workspace::find(d).unwrap();
    itero::engine::events::emit(&ws, "kutu.calis", None, None, "2026-10-16T10:00:00Z").unwrap();
    ws
}

#[test]
fn a_container_action_runs_in_its_image_with_its_job_and_environment() {
    if !docker() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    // the job on stdin, the action's folder at /action, its environment by
    // name; prints what it saw
    write(
        d.path(),
        "actions/kutu/gor.py",
        "import json, os, sys\n\
         job = json.loads(sys.stdin.readline())\n\
         print(json.dumps({'os': open('/etc/os-release').read().split('ID=')[1].split()[0],\n\
                           'selam': job['with']['selam'], 'dil': os.environ['DIL_ADI'],\n\
                           'burada': os.getcwd(), 'dosyalar': sorted(os.listdir('.'))}))\n",
    );
    let ws = workspace(
        d.path(),
        r#"{ "name": "kutu", "version": 1, "type": "container", "image": "python:3.13-alpine",
             "environment": { "DIL_ADI": "Türkçe" }, "limits": { "timeout": "60s" },
             "targets": { "calis": { "executable": "python3", "arguments": ["gor.py"] } } }"#,
    );
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs[0].runtime.state, "bitti", "{:?}", runs[0]);
    let seen = &j.memory("kutu").unwrap()["cikti"];
    assert_eq!(seen["os"], "alpine");
    assert_eq!(seen["selam"], "merhaba dünya");
    assert_eq!(seen["dil"], "Türkçe");
    assert_eq!(seen["burada"], "/action");
    assert_eq!(
        seen["dosyalar"],
        serde_json::json!(["action.json", "gor.py"])
    );
}

#[test]
fn a_container_past_its_time_limit_is_removed() {
    if !docker() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(
        d.path(),
        r#"{ "name": "kutu", "version": 1, "type": "container", "image": "busybox:1.37",
             "limits": { "timeout": "3s" },
             "targets": { "calis": { "executable": "sleep", "arguments": ["120"] } } }"#,
    );
    run_once(&ws, t("2026-10-16T10:00:01Z"), exe()).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs[0].runtime.state, "failed");
    let left = std::process::Command::new("docker")
        .args([
            "ps",
            "-aq",
            "--filter",
            "label=dev.aegea.itero.job",
            "--filter",
            "ancestor=busybox:1.37",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&left.stdout).trim(),
        "",
        "no container left running"
    );
}
