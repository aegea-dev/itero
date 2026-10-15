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

#[test]
fn notify_writes_to_its_log_when_one_is_set() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    let j = job(
        "notify@1",
        json!({"title": "Posta", "text": "2 yeni ileti: Çay saati"}),
        d.path().to_path_buf(),
    );
    // in its own process: the log is read from the worker's environment
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("worker")
        .env("ITERO_NOTIFY_LOG", &log)
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
    assert_eq!(v["output"]["shown"], true);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "Posta\t2 yeni ileti: Çay saati\n"
    );
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
