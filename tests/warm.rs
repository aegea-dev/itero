//! Warm actions: which Python, a file's identity, the runner.
#![cfg(unix)]
use itero::engine::warm::{self, Python};
use std::path::Path;

fn file(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let f = dir.join(name);
    std::fs::write(&f, text).unwrap();
    f
}

#[test]
fn which_python_comes_from_the_first_line() {
    assert_eq!(
        warm::python("#!/usr/bin/env python3").unwrap(),
        Python::Plain("python3".into())
    );
    assert_eq!(
        warm::python("#!/usr/bin/python3.12").unwrap(),
        Python::Plain("/usr/bin/python3.12".into())
    );
    assert_eq!(
        warm::python("#!/usr/bin/env -S uv run --script").unwrap(),
        Python::UvScript
    );
    assert_eq!(
        warm::python("#!/usr/bin/env -S uv run --project actions/py/güzel python").unwrap(),
        Python::UvProject("actions/py/güzel".into())
    );
    for (first, runs) in [
        ("#!/bin/sh", "sh"),
        ("#!/usr/bin/env node", "node"),
        ("#!/usr/bin/env -S bun --install=force", "bun"),
    ] {
        assert_eq!(
            warm::python(first).unwrap_err(),
            format!("warm: only Python actions can be warm: its first line runs {runs}")
        );
    }
}

#[test]
fn a_changed_file_has_another_identity() {
    let d = tempfile::tempdir().unwrap();
    let f = file(d.path(), "şeftali", "#!/usr/bin/env python3\n");
    let before = warm::identity(&f).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&f, "#!/usr/bin/env python3\nprint('ılık')\n").unwrap();
    assert_ne!(before, warm::identity(&f).unwrap());
}

#[test]
fn copies_are_at_most_the_cpus_and_the_steps() {
    let cpus = std::thread::available_parallelism().unwrap().get();
    assert_eq!(warm::most_copies(1), 1);
    assert_eq!(warm::most_copies(10_000), cpus);
}

use itero::engine::warm::runner::{End, Runner};
use itero::engine::worker::{Failure, Outcome, StepRun};
use serde_json::{Value, json};

/// A step of the Python action `body`, at `dir`: its args as environment
/// variables and on stdin, as the daemon gives them.
fn py_step(dir: &Path, body: &str, args: Value) -> StepRun {
    let f = file(dir, "eylem", &format!("#!/usr/bin/env python3\n{body}\n"));
    let mut input = serde_json::Map::new();
    input.insert("args".into(), args.clone());
    let env = args
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().map_or(v.to_string(), str::to_string)))
        .collect();
    StepRun {
        file: f,
        root: dir.to_path_buf(),
        workflow: "sipariş".into(),
        env,
        input,
        memory: dir.join(".itero/memory/sipariş"),
        waiting_since: None,
        quiet: 600,
        most: 86_400,
        now: String::new(),
    }
}

const ACTION: &str = r#"
import json, os, sys, subprocess, threading, time
a = json.load(sys.stdin)["args"]
w = a["what"]
if w == "raise": {"a": 1}["ğ"]
if w == "notyet": print(json.dumps({"payment_ref": "ödeme-1"})); sys.exit(75)
if w == "exit2": print("disk dolu", file=sys.stderr); sys.exit(2)
if w == "text": sys.exit("çok iyi değil")
if w == "child": subprocess.run(["echo", "çocuk"])
if w == "thread": threading.Thread(target=lambda: time.sleep(5), daemon=True).start()
if w == "osexit": os._exit(3)
if w == "where":
    print(json.dumps({"cwd": os.getcwd(), "argv": sys.argv, "env": os.environ.get("what"),
                      "left": os.environ.get("ILIK"), "memory": os.environ.get("ITERO_MEMORY"),
                      "path0": sys.path[0], "buffer": sys.stdin.buffer.read().decode()}))
    os.environ["ILIK"] = "şeftali"; os.chdir("/"); sys.exit(0)
print(json.dumps({"ok": w}))
"#;

fn ask(r: &mut Runner, dir: &Path, what: &str) -> End {
    r.request(&py_step(dir, ACTION, json!({ "what": what })), &|_| ())
}

#[test]
fn a_warm_step_ends_as_a_fresh_process_would() {
    let d = tempfile::tempdir().unwrap();
    let mut r = Runner::start(&py_step(d.path(), ACTION, json!({"what": "ok"}))).unwrap();
    let e = ask(&mut r, d.path(), "ok");
    assert_eq!(e.outcome, Ok(Outcome::Done(json!({"ok": "ok"}))));
    assert!(e.keep);
    let e = ask(&mut r, d.path(), "raise");
    assert!(
        matches!(&e.outcome, Err(Failure::Error(w)) if w.contains("KeyError: 'ğ'")),
        "{:?}",
        e.outcome
    );
    assert!(e.keep, "an exception ends the step, not the copy");
    assert_eq!(
        ask(&mut r, d.path(), "notyet").outcome,
        Ok(Outcome::NotYet(json!({"payment_ref": "ödeme-1"})))
    );
    let e = ask(&mut r, d.path(), "exit2");
    assert!(
        matches!(&e.outcome, Err(Failure::Error(w)) if w.contains("disk dolu")),
        "{:?}",
        e.outcome
    );
    let e = ask(&mut r, d.path(), "text");
    assert!(
        matches!(&e.outcome, Err(Failure::Error(w)) if w.contains("çok iyi değil")),
        "{:?}",
        e.outcome
    );
    // the subprocess wrote to its inherited stdout: part of the output,
    // text as a whole (two lines), not JSON
    assert_eq!(
        ask(&mut r, d.path(), "child").outcome,
        Ok(Outcome::Done(json!("çocuk\n{\"ok\": \"child\"}")))
    );
    assert_eq!(
        ask(&mut r, d.path(), "ok").outcome,
        Ok(Outcome::Done(json!({"ok": "ok"})))
    );
}

#[test]
fn each_request_starts_from_the_steps_own_environment_folder_and_argv() {
    let d = tempfile::tempdir().unwrap();
    let mut r = Runner::start(&py_step(d.path(), ACTION, json!({"what": "where"}))).unwrap();
    for _ in 0..2 {
        let e = ask(&mut r, d.path(), "where");
        let Ok(Outcome::Done(v)) = e.outcome else {
            panic!("{:?}", e.outcome)
        };
        assert_eq!(v["cwd"], json!(d.path().canonicalize().unwrap()));
        assert_eq!(v["argv"], json!([d.path().join("eylem")]));
        assert_eq!(v["env"], json!("where"));
        assert_eq!(
            v["left"],
            Value::Null,
            "the last request's environment is gone"
        );
        assert_eq!(v["memory"], json!(d.path().join(".itero/memory/sipariş")));
        assert_eq!(v["path0"], json!(d.path()));
        assert_eq!(
            v["buffer"],
            json!(""),
            "json.load read the line; the buffer is there"
        );
    }
}

#[test]
fn a_copy_left_with_a_thread_or_ended_is_not_kept() {
    let d = tempfile::tempdir().unwrap();
    let mut r = Runner::start(&py_step(d.path(), ACTION, json!({"what": "thread"}))).unwrap();
    let e = ask(&mut r, d.path(), "thread");
    assert_eq!(e.outcome, Ok(Outcome::Done(json!({"ok": "thread"}))));
    assert!(!e.keep);
    assert_eq!(e.why_not.as_deref(), Some("a thread still runs"));
    let mut r = Runner::start(&py_step(d.path(), ACTION, json!({"what": "osexit"}))).unwrap();
    let e = ask(&mut r, d.path(), "osexit");
    assert!(
        matches!(&e.outcome, Err(Failure::Error(_))),
        "{:?}",
        e.outcome
    );
    assert!(!e.keep);
}

#[test]
fn output_that_looks_like_a_mark_is_kept_whole() {
    let d = tempfile::tempdir().unwrap();
    let body = "print('itero-mark-0000000000000000')\nprint('güzel üzüm')";
    let mut r = Runner::start(&py_step(d.path(), body, json!({}))).unwrap();
    let e = r.request(&py_step(d.path(), body, json!({})), &|_| ());
    assert_eq!(
        e.outcome,
        Ok(Outcome::Done(json!(
            "itero-mark-0000000000000000\ngüzel üzüm"
        )))
    );
}
