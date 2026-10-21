use itero::engine::{Job, Program, worker};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

fn job(action: &str, with: Value, root: PathBuf) -> Job {
    Job {
        action: action.into(),
        with: with.as_object().cloned().unwrap_or_default(),
        key: "r1:e1.2:0".into(),
        input: Value::Null,
        prev: Value::Null,
        run: json!({"id": "r1", "workflow": "akış"})
            .as_object()
            .unwrap()
            .clone(),
        memory: json!({"son": "Gölge"}).as_object().unwrap().clone(),
        args: Map::new(),
        root,
        program: None,
        timeout: 10,
        heartbeat: None,
        prepare: None,
    }
}

/// A one-request HTTP server: answers with JSON naming what it was sent.
fn server() -> (String, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut first = String::new();
        r.read_line(&mut first).unwrap();
        let mut headers = Map::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let (k, v) = line.split_once(": ").unwrap();
            if k.eq_ignore_ascii_case("content-length") {
                length = v.parse().unwrap();
            }
            headers.insert(k.to_lowercase(), json!(v));
        }
        let mut body = vec![0; length];
        r.read_exact(&mut body).unwrap();
        let answer = json!({
            "data": {"name": "Ürün"},
            "line": first.trim_end(),
            "headers": headers,
            "body": String::from_utf8_lossy(&body),
        })
        .to_string();
        write!(s, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).unwrap();
    });
    (url, h)
}

#[test]
fn http_request_sends_and_reads_json() {
    let (url, h) = server();
    let with = json!({"url": format!("{url}/items/12"), "method": "POST", "headers": {"Authorization": "Bearer gizli"}, "json": {"söz": "ışık"}});
    let out = worker::run(&job("http.request@1", with, std::env::temp_dir())).unwrap();
    h.join().unwrap();
    assert_eq!(out["status"], 201);
    assert_eq!(out["json"]["data"]["name"], "Ürün");
    assert_eq!(out["json"]["line"], "POST /items/12 HTTP/1.1");
    assert_eq!(out["json"]["headers"]["authorization"], "Bearer gizli");
    assert_eq!(out["json"]["headers"]["idempotency-key"], "r1:e1.2:0");
    assert_eq!(
        serde_json::from_str::<Value>(out["json"]["body"].as_str().unwrap()).unwrap(),
        json!({"söz": "ışık"})
    );
    assert_eq!(out["headers"]["content-type"], "application/json");
}

#[test]
fn http_request_reads_no_more_than_its_cap() {
    let (url, h) = server();
    let with = json!({"url": format!("{url}/büyük"), "max_bytes": 10});
    let e = worker::run(&job("http.request@1", with, std::env::temp_dir()))
        .unwrap_err()
        .to_string();
    h.join().unwrap();
    assert!(
        e.contains("larger than 10 bytes") && e.contains("max_bytes"),
        "{e}"
    );
}

#[test]
fn http_request_that_cannot_connect_fails() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    drop(l);
    let e = worker::run(&job(
        "http.request@1",
        json!({"url": url}),
        std::env::temp_dir(),
    ))
    .unwrap_err()
    .to_string();
    assert!(e.contains("http.request@1"), "{e}");
}

#[test]
fn command_runs_in_the_workspace_and_reports_its_exit() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("notlar.txt"), "çay\nşeker\n").unwrap();
    let with = json!({"run": ["sh", "-c", "cat notlar.txt; echo hata 1>&2; exit 3"]});
    let out = worker::run(&job("command@1", with, d.path().to_path_buf())).unwrap();
    assert_eq!(
        out,
        json!({"exit": 3, "stdout": "çay\nşeker\n", "stderr": "hata\n", "lines": ["çay", "şeker"]})
    );
    let e = worker::run(&job(
        "command@1",
        json!({"run": ["yok-böyle-bir-program"]}),
        d.path().to_path_buf(),
    ))
    .unwrap_err()
    .to_string();
    assert!(e.contains("yok-böyle-bir-program"), "{e}");
}

#[test]
fn sh_runs_a_pipeline_with_values_only_in_its_environment() {
    let d = tempfile::tempdir().unwrap();
    let with = json!({"run": "printf '%s\\n' \"$KONU\" | wc -l | tr -d ' '",
                      "env": {"KONU": "çay; touch kırık"}});
    let out = worker::run(&job("sh@1", with, d.path().to_path_buf())).unwrap();
    assert_eq!(out["exit"], 0);
    assert_eq!(out["lines"], json!(["1"]));
    assert_eq!(out["json"], json!(1));
    assert!(
        !d.path().join("kırık").exists(),
        "a value never runs as a command"
    );
    let out = worker::run(&job(
        "sh@1",
        json!({"run": "echo \"$SAYI\"; exit 4", "env": {"SAYI": 7}}),
        d.path().to_path_buf(),
    ))
    .unwrap();
    assert_eq!(
        (out["exit"].clone(), out["json"].clone()),
        (json!(4), json!(7))
    );
}

/// Runs a job in `itero worker`, its own process, with `env` set: what
/// notify@1 reads comes from the worker's environment.
fn in_worker(j: &Job, env: &[(&str, &std::ffi::OsStr)]) -> Value {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("worker")
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(j).unwrap().as_bytes())
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn notify_writes_to_its_log_when_one_is_set() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    let j = job(
        "notify@1",
        json!({"title": "Posta", "text": "2 yeni ileti: Çay saati"}),
        d.path().to_path_buf(),
    );
    let v = in_worker(&j, &[("ITERO_NOTIFY_LOG", log.as_os_str())]);
    assert_eq!(v["output"]["shown"], true);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "Posta\t2 yeni ileti: Çay saati\n"
    );
}

/// macOS: Itero.app's notifier (here a stand-in, by ITERO_NOTIFIER).
/// Elsewhere: notify-send (here a stand-in, first on PATH). Either gets
/// the title and text as its last two arguments, never in a script.
/// Homebrew's layout: `bin/itero` a link into the Cellar, `Itero.app` in the
/// formula's `libexec`. The worker follows the link and finds the app.
#[cfg(target_os = "macos")]
#[test]
fn notify_finds_itero_app_in_a_formulas_libexec() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let cellar = d.path().join("Cellar/itero/0.1.0");
    std::fs::create_dir_all(cellar.join("bin")).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_itero"), cellar.join("bin/itero")).unwrap();
    let app = cellar.join("libexec/Itero.app/Contents/MacOS");
    std::fs::create_dir_all(&app).unwrap();
    let got = d.path().join("argümanlar");
    std::fs::write(
        app.join("itero-notifier"),
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\n",
            got.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        app.join("itero-notifier"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::create_dir_all(d.path().join("bin")).unwrap();
    std::os::unix::fs::symlink(cellar.join("bin/itero"), d.path().join("bin/itero")).unwrap();
    let j = job(
        "notify@1",
        json!({"title": "Posta", "text": "1 new: çay"}),
        d.path().to_path_buf(),
    );
    let mut c = std::process::Command::new(d.path().join("bin/itero"))
        .arg("worker")
        .env("HOME", d.path())
        .env_remove("ITERO_NOTIFIER")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&j).unwrap().as_bytes())
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["output"]["by"], "itero-notifier");
    assert_eq!(
        std::fs::read_to_string(&got).unwrap(),
        "Posta\n1 new: çay\n"
    );
}

#[cfg(unix)]
#[test]
fn notify_runs_the_notifier_with_the_title_and_text_as_arguments() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let got = d.path().join("argümanlar");
    let name = if cfg!(target_os = "macos") {
        "itero-notifier"
    } else {
        "notify-send"
    };
    let fake = d.path().join(name);
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\n",
            got.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let j = job(
        "notify@1",
        json!({"title": "Posta; touch kırık", "text": "1 new: $(çay)"}),
        d.path().to_path_buf(),
    );
    let path = format!(
        "{}:{}",
        d.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let v = in_worker(
        &j,
        &[
            ("ITERO_NOTIFIER", fake.as_os_str()),
            ("PATH", std::ffi::OsStr::new(&path)),
        ],
    );
    assert_eq!(v["output"], json!({"shown": true, "by": name}));
    let args = std::fs::read_to_string(&got).unwrap();
    let last: Vec<&str> = args.lines().rev().take(2).collect();
    assert_eq!(last, ["1 new: $(çay)", "Posta; touch kırık"]);
    assert!(!d.path().join("kırık").exists());
}

#[cfg(unix)]
#[test]
fn a_program_gets_the_job_on_stdin_and_prints_its_output() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let script = d.path().join("araç.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nread line\nprintf '{\"got\": %s, \"bölge\": \"%s\"}' \"$line\" \"$BOLGE\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut j = job("arac.run@1", json!({"n": 3}), d.path().to_path_buf());
    j.program = Some(Program {
        folder: d.path().to_path_buf(),
        executable: "./araç.sh".into(),
        arguments: vec![],
        environment: [("BOLGE".to_string(), "eu".to_string())].into(),
        interpreter: None,
    });
    let out = worker::run(&j).unwrap();
    assert_eq!(out["exit"], 0);
    assert_eq!(out["json"]["bölge"], "eu");
    assert_eq!(out["json"]["got"]["with"], json!({"n": 3}));
    assert_eq!(out["json"]["got"]["run"]["id"], "r1");
    assert_eq!(out["json"]["got"]["memory"]["son"], "Gölge");
    assert_eq!(out["json"]["got"]["key"], "r1:e1.2:0");
    std::fs::write(&script, "#!/bin/sh\necho kırık 1>&2\nexit 2\n").unwrap();
    assert!(worker::run(&j).unwrap_err().to_string().contains("kırık"));
    // an error is not a loss; a program killed by a signal is
    assert!(matches!(worker::run(&j), Err(worker::Failure::Error(_))));
    std::fs::write(&script, "#!/bin/sh\necho 'düz metin'\n").unwrap();
    let out = worker::run(&j).unwrap();
    assert_eq!(out["lines"], json!(["düz metin"]));
    assert!(out.get("json").is_none());
    std::fs::write(&script, "#!/bin/sh\nkill -9 $$\n").unwrap();
    let e = worker::run(&j).unwrap_err();
    assert!(matches!(e, worker::Failure::Lost(_)), "{e:?}");
}

#[test]
fn a_template_in_sh_run_is_refused() {
    let flow = r#"{ "name": "akis", "states": ["a", "b"], "start": "a", "end": ["b"],
      "transitions": [ { "from": "a", "to": "b", "do": [
        { "action": "sh@1", "with": { "run": "echo ${args.ad}" } } ] } ] }"#;
    let e = itero::workflow::parse(flow).unwrap_err().to_string();
    assert!(e.contains("put values in env"), "{e}");
    let ok = r#"{ "name": "akis", "states": ["a", "b"], "start": "a", "end": ["b"],
      "transitions": [ { "from": "a", "to": "b", "do": [
        { "action": "sh@1", "with": { "run": "echo \"$AD\"", "env": { "AD": "${args.ad}" } } },
        { "action": "notify@1", "with": { "title": "Başlık", "text": "${prev.stdout}" } } ] } ] }"#;
    itero::workflow::parse(ok).unwrap();
    let bad_env = ok.replace("\"AD\":", "\"ad soyad\":");
    let e = itero::workflow::parse(&bad_env).unwrap_err().to_string();
    assert!(e.contains("ad soyad"), "{e}");
}

// The simple form: one action file per step.

#[cfg(unix)]
mod steps {
    use itero::engine::worker::{self, Failure, Outcome, StepRun};
    use serde_json::{Map, Value, json};
    use std::path::{Path, PathBuf};

    /// An executable shell action in `dir`.
    fn action(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let f = dir.join(name);
        std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        f
    }

    fn step(dir: &Path, file: PathBuf, env: &[(&str, &str)], state: Value) -> StepRun {
        StepRun {
            file,
            root: dir.to_path_buf(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            state: state.as_object().cloned().unwrap_or_default(),
            memory: dir.join(".itero/memory/sipariş"),
            waiting_since: None,
            timeout: 10,
        }
    }

    fn done(o: Result<Outcome, Failure>) -> Map<String, Value> {
        match o {
            Ok(Outcome::Done(m)) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_action_reads_its_values_and_the_state_and_prints_json() {
        let d = tempfile::tempdir().unwrap();
        let f = action(
            d.path(),
            "selam",
            r#"read -r state
printf '{"host": "%s", "state": %s, "where": "%s"}' "$host" "$state" "$(pwd)""#,
        );
        let out = done(worker::run_step(&step(
            d.path(),
            f,
            &[("host", "posta.örnek.test")],
            json!({"sipariş": 43}),
        )));
        assert_eq!(out["host"], "posta.örnek.test");
        assert_eq!(out["state"], json!({"sipariş": 43}));
        let here = std::fs::canonicalize(d.path()).unwrap();
        assert_eq!(out["where"], here.display().to_string());
    }

    #[test]
    fn printing_nothing_is_an_empty_output_and_anything_but_an_object_an_error() {
        let d = tempfile::tempdir().unwrap();
        let quiet = action(d.path(), "sessiz", "exit 0");
        assert_eq!(
            done(worker::run_step(&step(d.path(), quiet, &[], json!({})))),
            Map::new()
        );
        for (name, body) in [("liste", "echo '[1]'"), ("yazı", "echo 'çay hazır'")] {
            let f = action(d.path(), name, body);
            let e = worker::run_step(&step(d.path(), f, &[], json!({}))).unwrap_err();
            assert!(e.to_string().contains("not a JSON object"), "{name}: {e}");
        }
    }

    #[test]
    fn exit_75_is_not_yet_and_keeps_what_it_printed() {
        let d = tempfile::tempdir().unwrap();
        let f = action(d.path(), "ödeme", r#"echo '{"ref": "ödeme-1"}'; exit 75"#);
        match worker::run_step(&step(d.path(), f, &[], json!({}))) {
            Ok(Outcome::NotYet(m)) => assert_eq!(m["ref"], "ödeme-1"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn another_exit_is_an_error_with_the_end_of_stderr() {
        let d = tempfile::tempdir().unwrap();
        let f = action(d.path(), "bozuk", "echo 'bağlantı yok' >&2; exit 3");
        match worker::run_step(&step(d.path(), f, &[], json!({}))) {
            Err(Failure::Error(why)) => assert_eq!(why, "exit 3: bağlantı yok"),
            other => panic!("{other:?}"),
        }
        let missing = d.path().join("yok");
        let e = worker::run_step(&step(d.path(), missing, &[], json!({}))).unwrap_err();
        assert!(e.to_string().contains("cannot run"), "{e}");
    }

    #[test]
    fn memory_is_kept_between_runs_and_waiting_since_is_given() {
        let d = tempfile::tempdir().unwrap();
        let f = action(
            d.path(),
            "sayaç",
            r#"n=$(cat "$ITERO_MEMORY" 2>/dev/null || echo 0)
n=$((n + 1)); echo "$n" > "$ITERO_MEMORY"
printf '{"n": %s, "since": "%s"}' "$n" "$ITERO_WAITING_SINCE""#,
        );
        let s = step(d.path(), f, &[], json!({}));
        assert_eq!(done(worker::run_step(&s))["n"], 1);
        let mut again = s.clone();
        again.waiting_since = Some("2026-10-21T10:00:00Z".into());
        let out = done(worker::run_step(&again));
        assert_eq!(out["n"], 2);
        assert_eq!(out["since"], "2026-10-21T10:00:00Z");
    }
}
