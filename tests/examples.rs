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
        let out = String::from_utf8(itero(&ws, &["ws", "check"]).stdout).unwrap();
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
            "ws",
            "emit",
            "order.placed",
            r#"{"order": 43, "email": "ayşe@örnek.test"}"#,
        ],
    );
    itero(&ws, &["daemon", "--once"]);
    let runs = String::from_utf8(itero(&ws, &["ws", "wf", "runs"]).stdout).unwrap();
    assert!(runs.contains("waiting"), "{runs}");
    itero(
        &ws,
        &[
            "ws",
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
    // its sites are its step's arg, a JSON list in $sites as a run gives
    // it; a port nothing listens on is down, without the network
    let flow: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.join("workflows/sites.json")).unwrap())
            .unwrap();
    assert!(
        flow["steps"][0]["#check-sites"]["args"]["sites"].is_array(),
        "{flow}"
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
    // it reads the step before's result, its arg now, from its stdin
    let changed = |now: &str| {
        let input = format!(r#"{{"args": {{"now": {now}}}}}"#);
        run_action(&ws, "changed", &[("ITERO_MEMORY", memory)], &input)
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

/// A daemon the test started, gone with the test, also when it fails.
struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = (self.0.kill(), self.0.wait());
    }
}

/// The workspace's runs of `workflow`.
fn runs_of(ws: &Path, workflow: &str) -> Vec<serde_json::Value> {
    let o = itero(ws, &["ws", "wf", "runs", "--json"]);
    let runs: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let all = runs.as_array().unwrap().iter();
    all.filter(|r| r["workflow"] == workflow).cloned().collect()
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !done() {
        assert!(std::time::Instant::now() < end, "never: {what}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn the_webhook_example_forwards_a_reply_to_the_step_waiting_for_it() {
    use std::io::{Read, Write};
    let d = tempfile::tempdir().unwrap();
    let ws = copy("webhook", d.path());
    std::fs::write(ws.join(".env"), "APP_TOKEN=bir-uzun-gizli-değer\n").unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    // the example's connector, on a free port
    let script = ws.join("connectors/webhook/webhook.listen");
    let text = std::fs::read_to_string(&script)
        .unwrap()
        .replace("8788", &port.to_string());
    std::fs::write(&script, text).unwrap();
    let _daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NO_SECRET_STORE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until("it listens", || {
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    itero(
        &ws,
        &["ws", "emit", "app.ask", r#"{"to": "+905550000000"}"#],
    );
    until("the step waits", || {
        runs_of(&ws, "ask").iter().any(|r| r["status"] == "waiting")
    });
    let send = |body: &str| {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let raw = format!(
            "POST /app HTTP/1.1\r\nHost: a\r\nAuthorization: Bearer bir-uzun-gizli-değer\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    };
    let reply = r#"{"id": "m1", "from": "+905550000000", "text": "günaydın"}"#;
    let forwarded = |n: usize| {
        let forward = runs_of(&ws, "forward");
        forward.len() == n && forward.iter().all(|r| r["status"] == "done")
    };
    send(reply);
    until("the first delivery's run", || forwarded(1));
    until("the step has its reply", || {
        runs_of(&ws, "ask").iter().any(|r| r["status"] == "done")
    });
    send(reply); // a service sends again when a delivery failed
    until("the second delivery's run", || forwarded(2));
    let log = std::fs::read_to_string(ws.join(".itero/itero.log")).unwrap();
    let sent = log
        .lines()
        .filter(|l| l.contains(r#""type":"app.message.received""#))
        .count();
    assert_eq!(sent, 1, "the id was seen: {log}");
}

/// `body`, signed as GitHub signs it, to `port`: its status.
fn github_delivery(port: u16, secret: &str, event: &str, id: &str, body: &str) -> u16 {
    use std::io::{Read, Write};
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let hex: String = ring::hmac::sign(&key, body.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let raw = format!(
        "POST /github HTTP/1.1\r\nHost: a\r\nContent-Type: application/json\r\nX-GitHub-Event: {event}\r\nX-GitHub-Delivery: {id}\r\nX-Hub-Signature-256: sha256={hex}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split(' ').nth(1).unwrap().parse().unwrap()
}

#[test]
fn the_github_webhook_example_tells_of_a_push_once() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("github-webhook", d.path());
    let secret = "bir-uzun-gizli-değer";
    std::fs::write(ws.join(".env"), format!("GITHUB_WEBHOOK_SECRET={secret}\n")).unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let script = ws.join("connectors/webhook/webhook.listen");
    let text = std::fs::read_to_string(&script)
        .unwrap()
        .replace("8788", &port.to_string());
    std::fs::write(&script, text).unwrap();
    let told = ws.join("bildirim.log");
    let _daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFY_LOG", &told)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until("it listens", || {
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });
    // what GitHub sends when the hook is made: nothing to tell
    assert_eq!(
        github_delivery(port, secret, "ping", "d-0", r#"{"zen": "sade"}"#),
        200
    );
    until("the ping's run", || {
        runs_of(&ws, "forward")
            .iter()
            .any(|r| r["status"] == "done")
    });
    // a commit's message is a stranger's text: a $$NAME or $event in it stays text
    let push = r#"{"ref": "refs/heads/ana", "repository": {"full_name": "örnek/depo"},
                   "pusher": {"name": "kişi"},
                   "head_commit": {"message": "düzeltme $$GITHUB_WEBHOOK_SECRET $event.ref"}}"#;
    assert_eq!(github_delivery(port, secret, "push", "d-1", push), 200);
    until("the push is told", || told.is_file());
    // GitHub's Redeliver sends the same delivery again
    assert_eq!(github_delivery(port, secret, "push", "d-1", push), 200);
    until("every delivery's run", || {
        let forward = runs_of(&ws, "forward");
        forward.len() == 3 && forward.iter().all(|r| r["status"] == "done")
    });
    // signed with something else: nothing comes of it
    assert_eq!(
        github_delivery(port, "başka-bir-gizli-değer", "push", "d-2", push),
        401
    );
    until("the told workflow ended", || {
        !runs_of(&ws, "tell").is_empty()
            && runs_of(&ws, "tell").iter().all(|r| r["status"] == "done")
    });
    let told = std::fs::read_to_string(&told).unwrap();
    assert_eq!(told.lines().count(), 1, "{told}");
    assert!(told.starts_with("örnek/depo\t"), "{told}");
    assert!(
        told.contains("ana, by kişi: düzeltme $$GITHUB_WEBHOOK_SECRET $event.ref"),
        "{told}"
    );
    assert!(!told.contains(secret), "{told}");
    assert_eq!(runs_of(&ws, "tell").len(), 1);
}

/// A stand-in for an HTTP server: answers each request, in turn, with the
/// next of `answers` (a status and a body); what it was sent, its head and
/// body, comes back through the handle.
fn http_server(
    answers: Vec<(u16, &'static str)>,
) -> (u16, std::thread::JoinHandle<Vec<(String, String)>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut got = Vec::new();
        listener.set_nonblocking(true).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        for (status, body) in answers {
            // none in 30s: what came so far, for the test to fail on
            let s = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() > end {
                            return got;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            s.set_nonblocking(false).unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push_str(&line);
            }
            let len = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut sent = vec![0; len];
            r.read_exact(&mut sent).unwrap();
            let mut w = s;
            write!(
                w,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            got.push((head, String::from_utf8(sent).unwrap()));
        }
        got
    });
    (port, handle)
}

/// An example's action run by hand, its env given: whether it succeeded,
/// its stdout and its stderr.
fn try_action(ws: &Path, name: &str, env: &[(&str, &str)]) -> (bool, String, String) {
    let o = std::process::Command::new(ws.join("actions").join(name))
        .current_dir(ws)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

#[test]
fn the_messages_push_sends_to_its_topic_and_says_why_a_server_refused() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("messages", d.path());
    let (port, server) = http_server(vec![
        (200, r#"{"id":"m1","event":"message"}"#),
        (
            400,
            r#"{"code":40007,"error":"invalid priority parameter"}"#,
        ),
    ]);
    let url = format!("http://127.0.0.1:{port}");
    let env = [
        ("server", url.as_str()),
        ("to", "itero-uzun-rastgele-konu"),
        ("subject", "Uyarı: disk"),
        ("text", "Disk dolu: \"yüzde 95\"\nikinci satır"),
        ("token", "tk_gizli"),
    ];
    let (ok, out, err) = try_action(&ws, "push", &env);
    assert!(ok, "{err}");
    assert_eq!(
        out.trim(),
        r#"{"id":"m1","event":"message"}"#,
        "its answer is the output"
    );
    let (ok, _, err) = try_action(&ws, "push", &env);
    assert!(!ok, "a refusal fails the step");
    assert!(
        err.contains("invalid priority parameter"),
        "and says why: {err}"
    );
    let got = server.join().unwrap();
    assert_eq!(got.len(), 2, "{got:?}");
    let (head, body) = &got[0];
    assert!(head.starts_with("POST / HTTP/1.1"), "{head}");
    assert!(head.contains("Authorization: Bearer tk_gizli"), "{head}");
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        sent,
        serde_json::json!({
            "topic": "itero-uzun-rastgele-konu",
            "title": "Uyarı: disk",
            "message": "Disk dolu: \"yüzde 95\"\nikinci satır"
        })
    );
}

#[test]
fn the_messages_actions_name_every_arg_they_miss_at_once() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("messages", d.path());
    for (name, says) in [
        ("send-mail", "send-mail: no host user to text"),
        ("push", "push: no to text"),
    ] {
        let (ok, _, err) = try_action(&ws, name, &[]);
        assert!(!ok && err.contains(says), "#{name}: {err}");
    }
    if cfg!(target_os = "macos") {
        let (ok, _, err) = try_action(&ws, "imessage", &[("service", "Faks")]);
        assert!(!ok && err.contains("imessage: no to text"), "{err}");
        let (ok, _, err) = try_action(
            &ws,
            "imessage",
            &[("to", "a"), ("text", "b"), ("service", "Faks")],
        );
        assert!(
            !ok && err.contains("service is iMessage or SMS, not Faks"),
            "{err}"
        );
    } else {
        let (ok, _, err) = try_action(&ws, "imessage", &[("to", "a"), ("text", "b")]);
        assert!(!ok && err.contains("Messages is on macOS only"), "{err}");
    }
}

#[test]
fn the_messages_alert_goes_the_way_its_event_names() {
    let d = tempfile::tempdir().unwrap();
    let ws = copy("messages", d.path());
    let (port, server) = http_server(vec![(200, r#"{"id":"m1"}"#)]);
    std::fs::write(ws.join(".env"), "NTFY_TOPIC=itero-uzun-rastgele-konu\n").unwrap();
    // the push step on the stand-in server
    let file = ws.join("workflows/alert.json");
    let mut flow: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    flow["steps"][1]["#push"]["args"]["server"] = format!("http://127.0.0.1:{port}").into();
    std::fs::write(&file, flow.to_string()).unwrap();
    itero(
        &ws,
        &[
            "ws",
            "emit",
            "alert.send",
            r#"{"via": "push", "subject": "Yedek", "text": "Gece yedeği bitti."}"#,
        ],
    );
    itero(&ws, &["daemon", "--once"]);
    let got = server.join().unwrap();
    assert_eq!(got.len(), 1, "one push: {:?}", runs_of(&ws, "alert"));
    let sent: serde_json::Value = serde_json::from_str(&got[0].1).unwrap();
    assert_eq!(
        sent,
        serde_json::json!({"topic": "itero-uzun-rastgele-konu", "title": "Yedek", "message": "Gece yedeği bitti."})
    );
    let runs = runs_of(&ws, "alert");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(
        runs[0]["status"], "done",
        "mail and iMessage were not its way: {runs:?}"
    );
}
