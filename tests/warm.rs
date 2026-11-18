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

/// A warm worker on a socket: what it prints, line by line.
struct Worker {
    child: std::process::Child,
    sock: Option<std::os::unix::net::UnixStream>,
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
}

impl Worker {
    fn start() -> Worker {
        use std::os::unix::process::CommandExt;
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["worker", "--warm"])
            .stdin(std::process::Stdio::from(std::os::fd::OwnedFd::from(
                theirs,
            )))
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        use std::io::BufRead;
        Worker {
            child,
            sock: Some(ours),
            lines: std::io::BufReader::new(out).lines(),
        }
    }

    /// Sends `step` with a lock of its own; its `done` line.
    fn step(&mut self, step: &StepRun) -> Value {
        let lock = tempfile::tempfile().unwrap();
        itero::engine::handover::send_framed(
            self.sock.as_ref().unwrap(),
            serde_json::to_string(step).unwrap().as_bytes(),
            std::os::fd::AsFd::as_fd(&lock),
        )
        .unwrap();
        let done = self
            .lines
            .by_ref()
            .map(Result::unwrap)
            .find(|l| l.starts_with("done "))
            .expect("no done line");
        serde_json::from_str(&done[5..]).unwrap()
    }
}

#[test]
fn a_warm_worker_takes_steps_until_its_socket_closes() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Worker::start();
    let v = w.step(&py_step(d.path(), ACTION, json!({"what": "ok"})));
    assert_eq!(
        v["answer"],
        json!({"outcome": {"done": {"ok": "ok"}}}),
        "{v}"
    );
    assert_eq!(v["keep"], json!(true));
    let v = w.step(&py_step(d.path(), ACTION, json!({"what": "raise"})));
    assert!(
        v["answer"]["error"]
            .as_str()
            .unwrap()
            .contains("KeyError: 'ğ'"),
        "{v}"
    );
    assert_eq!(v["keep"], json!(true));
    w.sock = None;
    assert!(w.child.wait().unwrap().success());
}

#[test]
fn a_process_left_by_a_step_ends_the_copy_with_it() {
    let d = tempfile::tempdir().unwrap();
    let body = "import subprocess\np = subprocess.Popen(['sleep', '30'])\nopen('kalan', 'w').write(str(p.pid))\nprint('ılık')";
    let mut w = Worker::start();
    let v = w.step(&py_step(d.path(), body, json!({})));
    assert_eq!(v["answer"], json!({"outcome": {"done": "ılık"}}), "{v}");
    assert_eq!(v["keep"], json!(false));
    assert_eq!(v["why"], json!("a process still runs"));
    let _ = w.child.wait();
    let left: i32 = std::fs::read_to_string(d.path().join("kalan"))
        .unwrap()
        .parse()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    // SAFETY: kill with signal 0 only asks whether the process is there
    let alive = unsafe { libc::kill(left, 0) } == 0;
    assert!(!alive, "sleep {left} was left running");
}

#[test]
fn a_warm_worker_whose_action_cannot_be_warm_says_why_and_ends() {
    let d = tempfile::tempdir().unwrap();
    let mut step = py_step(d.path(), "print('çay')", json!({}));
    std::fs::write(&step.file, "#!/bin/sh\necho çay\n").unwrap();
    step.env.clear();
    let mut w = Worker::start();
    let v = w.step(&step);
    assert_eq!(
        v["answer"],
        json!({"error": "warm: only Python actions can be warm: its first line runs sh"})
    );
    assert_eq!(v["keep"], json!(false));
    assert!(w.child.wait().unwrap().success());
}

/// A uv script's step at `dir`: python-slugify, the arg `title` read from
/// the environment.
fn uv_step(dir: &Path, title: &str, deps: &str) -> StepRun {
    let body = format!(
        "# /// script\n# requires-python = \">=3.10\"\n# dependencies = [{deps}]\n# ///\nimport json, os, sys\nfrom slugify import slugify\nprint(json.dumps({{\"slug\": slugify(os.environ[\"title\"]), \"python\": sys.executable}}))"
    );
    let mut s = py_step(dir, &body, json!({ "title": title }));
    let text = std::fs::read_to_string(&s.file).unwrap();
    std::fs::write(
        &s.file,
        text.replacen(
            "#!/usr/bin/env python3",
            "#!/usr/bin/env -S uv run --script",
            1,
        ),
    )
    .unwrap();
    s.workflow = "kısa-ad".into();
    s
}

#[test]
fn a_uv_script_runs_warm_in_its_own_environment() {
    let d = tempfile::tempdir().unwrap();
    let deps = "\"python-slugify>=8\"";
    let mut r = Runner::start(&uv_step(d.path(), "ılık şeftali", deps)).unwrap();
    for (title, slug) in [
        ("ılık şeftali", "ilik-seftali"),
        ("güzel üzüm", "guzel-uzum"),
    ] {
        let e = r.request(&uv_step(d.path(), title, deps), &|_| ());
        let Ok(Outcome::Done(v)) = e.outcome else {
            panic!("{:?}", e.outcome)
        };
        assert_eq!(v["slug"], json!(slug));
        let python = v["python"].as_str().unwrap();
        assert!(
            python.contains("environments"),
            "not uv's environment: {python}"
        );
    }
}

#[test]
fn a_python_project_runs_warm_in_its_venv() {
    let d = tempfile::tempdir().unwrap();
    file(
        d.path(),
        "pyproject.toml.tmp",
        "[project]\nname = \"cicek\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\ndependencies = [\"python-slugify>=8\"]\n",
    );
    let app = d.path().join("actions/py/çiçek-app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::rename(
        d.path().join("pyproject.toml.tmp"),
        app.join("pyproject.toml"),
    )
    .unwrap();
    let mut s = py_step(
        d.path(),
        "import json, os, sys\nfrom slugify import slugify\nprint(json.dumps({\"slug\": slugify(os.environ[\"title\"]), \"python\": sys.executable}))",
        json!({ "title": "çok iyi" }),
    );
    let text = std::fs::read_to_string(&s.file).unwrap();
    std::fs::write(
        &s.file,
        text.replacen(
            "#!/usr/bin/env python3",
            "#!/usr/bin/env -S uv run --project actions/py/çiçek-app python",
            1,
        ),
    )
    .unwrap();
    s.workflow = "proje".into();
    let mut r = Runner::start(&s).unwrap();
    let e = r.request(&s, &|_| ());
    let Ok(Outcome::Done(v)) = e.outcome else {
        panic!("{:?}", e.outcome)
    };
    assert_eq!(v["slug"], json!("cok-iyi"));
    // on macOS /var is /private/var: both resolved
    let parent = |p: &Path| p.parent().unwrap().canonicalize().unwrap();
    assert_eq!(
        parent(Path::new(v["python"].as_str().unwrap())),
        app.join(".venv/bin").canonicalize().unwrap(),
        "not the project's environment"
    );
}

#[test]
fn a_uv_sync_that_fails_is_an_error_with_uvs_why() {
    let d = tempfile::tempdir().unwrap();
    let s = uv_step(d.path(), "ağaç", "\"itero-yok-boyle-bir-paket-ğüş==0.0.1\"");
    let Err(why) = Runner::start(&s) else {
        panic!("it started")
    };
    assert!(why.starts_with("warm: uv sync: "), "{why}");
    assert!(
        why.len() > "warm: uv sync: ".len(),
        "uv's own why is missing: {why}"
    );
}
