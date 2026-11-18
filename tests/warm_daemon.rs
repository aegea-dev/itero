//! Warm steps under the daemon: copies started as steps need them, kept
//! between passes, replaced when their file changes, stopped when idle,
//! and none left once the daemon is gone.
#![cfg(unix)]
mod common;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

/// A workspace in a temporary folder, and its daemon once started.
struct Ws {
    d: tempfile::TempDir,
    daemon: Option<Child>,
}

impl Ws {
    fn new(manifest: Value) -> Ws {
        let d = tempfile::tempdir().unwrap();
        let mut m = json!({ "name": "ılık", "description": "warm steps" });
        m.as_object_mut()
            .unwrap()
            .extend(manifest.as_object().unwrap().clone());
        std::fs::write(d.path().join("itero.config.json"), m.to_string()).unwrap();
        Ws { d, daemon: None }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.d.path().join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let f = self.path(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }

    /// An action in `language` (its first line), executable.
    fn action(&self, name: &str, first: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let rel = format!("actions/{name}");
        self.write(&rel, &format!("{first}\n{body}\n"));
        std::fs::set_permissions(self.path(&rel), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn py(&self, name: &str, body: &str) {
        self.action(name, "#!/usr/bin/env python3", body);
    }

    /// A workflow on `event` with these steps.
    fn flow(&self, name: &str, event: &str, steps: Value) {
        self.write(
            &format!("workflows/{name}.json"),
            &json!({ "on": [{ "event": event }], "steps": steps }).to_string(),
        );
    }

    fn start(&mut self, env: &[(&str, &str)]) {
        let mut c = Command::new(exe());
        c.args(["ws", "daemon", "--no-serve"])
            .current_dir(self.d.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        self.daemon = Some(c.spawn().unwrap());
        self.wait_for(
            |l| l.iter().any(|e| e["event"] == "daemon.started"),
            "the daemon to start",
        );
    }

    fn emit(&self, event: &str, data: Value) {
        let out = Command::new(exe())
            .args(["ws", "emit", event, &data.to_string()])
            .current_dir(self.d.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn log(&self) -> Vec<Value> {
        let text = std::fs::read_to_string(self.path(".itero/itero.log")).unwrap_or_default();
        common::log_lines(&text)
    }

    fn wait_for(&self, done: impl Fn(&[Value]) -> bool, what: &str) -> Vec<Value> {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            let l = self.log();
            if done(&l) {
                return l;
            }
            assert!(Instant::now() < end, "waited for {what}; the log:\n{l:#?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until `n` runs of `workflow` ended; their ends.
    fn ended(&self, workflow: &str, n: usize) -> Vec<Value> {
        let l = self.wait_for(
            |l| {
                l.iter()
                    .filter(|e| e["event"] == "run.ended" && e["workflow"] == workflow)
                    .count()
                    >= n
            },
            &format!("{n} runs of {workflow}"),
        );
        l.into_iter()
            .filter(|e| e["event"] == "run.ended" && e["workflow"] == workflow)
            .collect()
    }

    /// The outputs of `step` in `workflow`'s runs, in order.
    fn outputs(&self, workflow: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|e| e["event"] == "step.done" && e["workflow"] == workflow)
            .map(|e| e["output"].clone())
            .collect()
    }

    fn events(&self, event: &str) -> Vec<Value> {
        self.log()
            .into_iter()
            .filter(|e| e["event"] == event)
            .collect()
    }
}

impl Drop for Ws {
    fn drop(&mut self) {
        if let Some(mut d) = self.daemon.take() {
            let _ = d.kill();
            let _ = d.wait();
        }
    }
}

fn alive(pid: i64) -> bool {
    // SAFETY: kill with signal 0 only asks whether the process is there
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// A Python action printing its process id, and its runner's environment
/// value `tag`.
const PID: &str = "import os, json\nprint(json.dumps({\"pid\": os.getpid()}))";

#[test]
fn copies_start_as_steps_need_them_and_stay_between_passes() {
    let mut ws = Ws::new(json!({ "limits": { "steps": 2 } }));
    ws.py("kim", PID);
    ws.flow("sor", "çay.istendi", json!([{ "#kim": { "warm": true } }]));
    ws.start(&[]);
    for i in 0..20 {
        ws.emit("çay.istendi", json!({ "sıra": i }));
    }
    let ends = ws.ended("sor", 20);
    assert!(ends.iter().all(|e| e["status"] == "done"), "{ends:#?}");
    let pids: std::collections::BTreeSet<i64> = ws
        .outputs("sor")
        .iter()
        .map(|o| o["pid"].as_i64().unwrap())
        .collect();
    assert!(pids.len() <= 2, "at most limits.steps copies: {pids:?}");
    assert!(ws.events("warm.started").len() <= 2);
    // a later pass takes the same copies
    ws.emit("çay.istendi", json!({ "sıra": 20 }));
    ws.ended("sor", 21);
    let last = ws.outputs("sor").last().unwrap()["pid"].as_i64().unwrap();
    assert!(
        pids.contains(&last),
        "{last} is a new copy; before: {pids:?}"
    );
}

#[test]
fn a_step_without_warm_runs_fresh_beside_warm_ones() {
    let mut ws = Ws::new(json!({}));
    ws.py("kim", PID);
    ws.flow("ılık", "çay.istendi", json!([{ "#kim": { "warm": true } }]));
    ws.flow("soğuk", "çay.istendi", json!(["#kim"]));
    ws.start(&[]);
    for i in 0..4 {
        ws.emit("çay.istendi", json!({ "sıra": i }));
        ws.ended("ılık", i + 1);
        ws.ended("soğuk", i + 1);
    }
    let pids = |w: &str| -> std::collections::BTreeSet<i64> {
        ws.outputs(w)
            .iter()
            .map(|o| o["pid"].as_i64().unwrap())
            .collect()
    };
    let (warm, fresh) = (pids("ılık"), pids("soğuk"));
    assert_eq!(warm.len(), 1, "one warm copy: {warm:?}");
    assert_eq!(fresh.len(), 4, "a fresh process each time: {fresh:?}");
}

#[test]
fn a_busy_warm_action_does_not_hold_step_slots() {
    // one slot more than the CPUs: every copy busy leaves a slot free, and
    // the warm steps past the copies wait for one, not for that slot
    let cpus = std::thread::available_parallelism().unwrap().get();
    let mut ws = Ws::new(json!({ "limits": { "steps": cpus + 1 } }));
    ws.py("yavaş", "import time\ntime.sleep(1)\nprint('ılık')");
    ws.action("hızlı", "#!/bin/sh", "echo şeftali");
    ws.flow(
        "yavaş",
        "yavaş.istendi",
        json!([{ "#yavaş": { "warm": true } }]),
    );
    ws.flow("hızlı", "hızlı.istendi", json!(["#hızlı"]));
    ws.start(&[]);
    for i in 0..cpus + 2 {
        ws.emit("yavaş.istendi", json!({ "sıra": i }));
    }
    ws.emit("hızlı.istendi", json!({}));
    let fast = ws.ended("hızlı", 1);
    let slow = ws.ended("yavaş", cpus + 2);
    let at = |e: &Value| e["time"].as_str().unwrap().to_string();
    assert!(
        at(&fast[0]) < at(&slow[0]),
        "#hızlı waited for a warm step: {} vs {}",
        at(&fast[0]),
        at(&slow[0])
    );
    assert!(ws.events("warm.started").len() <= cpus);
}

#[test]
fn a_changed_file_gets_a_fresh_copy() {
    let mut ws = Ws::new(json!({}));
    ws.py("kim", PID);
    ws.flow("sor", "çay.istendi", json!([{ "#kim": { "warm": true } }]));
    ws.start(&[]);
    ws.emit("çay.istendi", json!({}));
    ws.ended("sor", 1);
    std::thread::sleep(Duration::from_millis(20));
    ws.py(
        "kim",
        "import os, json\nprint(json.dumps({\"pid\": os.getpid(), \"yeni\": True}))",
    );
    ws.emit("çay.istendi", json!({}));
    ws.ended("sor", 2);
    let o = ws.outputs("sor");
    assert_eq!(o[1]["yeni"], json!(true), "the new file ran: {o:?}");
    assert_ne!(o[0]["pid"], o[1]["pid"]);
    assert!(
        ws.events("warm.ended")
            .iter()
            .any(|e| e["why"] == "changed")
    );
}

#[test]
fn an_idle_copy_stops() {
    let mut ws = Ws::new(json!({}));
    ws.py("kim", PID);
    ws.flow("sor", "çay.istendi", json!([{ "#kim": { "warm": true } }]));
    ws.start(&[("ITERO_TEST_WARM_IDLE", "1")]);
    ws.emit("çay.istendi", json!({}));
    ws.ended("sor", 1);
    let pid = ws.outputs("sor")[0]["pid"].as_i64().unwrap();
    ws.wait_for(
        |l| {
            l.iter()
                .any(|e| e["event"] == "warm.ended" && e["why"] == "idle")
        },
        "the idle copy to stop",
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(!alive(pid), "the runner {pid} is still there");
}

#[test]
fn no_copy_outlives_its_daemon() {
    let mut ws = Ws::new(json!({}));
    ws.py("kim", PID);
    ws.flow("sor", "çay.istendi", json!([{ "#kim": { "warm": true } }]));
    ws.start(&[]);
    ws.emit("çay.istendi", json!({}));
    ws.ended("sor", 1);
    let runner = ws.outputs("sor")[0]["pid"].as_i64().unwrap();
    let copy = ws.events("warm.started")[0]["copy"].as_i64().unwrap();
    let mut d = ws.daemon.take().unwrap();
    // SAFETY: SIGKILL to the daemon this test started
    unsafe { libc::kill(d.id() as i32, libc::SIGKILL) };
    let _ = d.wait();
    let end = Instant::now() + Duration::from_secs(3);
    while (alive(copy) || alive(runner)) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !alive(copy) && !alive(runner),
        "left: copy {copy}, runner {runner}"
    );
}

#[test]
fn a_lost_warm_step_runs_again_once_its_copy_is_gone() {
    let mut ws = Ws::new(json!({ "limits": { "quiet": "1s" } }));
    ws.py(
        "uyku",
        "import os, time, pathlib\np = pathlib.Path('kez')\nn = int(p.read_text()) if p.exists() else 0\np.write_text(str(n + 1))\nif n == 0:\n    pathlib.Path('ilk').write_text(str(os.getpid()))\n    time.sleep(30)\nprint('uyandı')",
    );
    ws.flow("uyu", "uyu.istendi", json!([{ "#uyku": { "warm": true } }]));
    ws.start(&[]);
    ws.emit("uyu.istendi", json!({}));
    let ends = ws.ended("uyu", 1);
    assert_eq!(ends[0]["status"], "done", "{ends:#?}");
    let first: i64 = std::fs::read_to_string(ws.path("ilk"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        !alive(first),
        "the first try's runner {first} is still there"
    );
    assert_eq!(std::fs::read_to_string(ws.path("kez")).unwrap(), "2");
}

#[test]
fn a_warm_step_of_a_non_python_action_fails_with_why() {
    let mut ws = Ws::new(json!({}));
    ws.action("kabuk", "#!/bin/sh", "echo çay");
    ws.flow(
        "sor",
        "çay.istendi",
        json!([{ "#kabuk": { "warm": true } }]),
    );
    ws.start(&[]);
    ws.emit("çay.istendi", json!({}));
    let ends = ws.ended("sor", 1);
    assert_eq!(ends[0]["status"], "error");
    let failed = ws.events("step.failed");
    assert!(
        failed[0]["why"]
            .as_str()
            .unwrap()
            .contains("warm: only Python actions can be warm: its first line runs sh"),
        "{failed:#?}"
    );
}
