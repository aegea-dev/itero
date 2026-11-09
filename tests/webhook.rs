//! Webhooks: `itero webhook` takes checked requests from outside and saves
//! them as `itero.webhook.received`.
#![cfg(unix)]
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn itero(d: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(d)
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap()
}

fn file(d: &Path, rel: &str, text: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, text).unwrap();
    if rel.starts_with("actions/") || rel.starts_with("connectors/") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn pass(d: &Path) {
    let now = itero::engine::daemon::stamp(chrono::Utc::now());
    let o = itero(d, &["daemon", "--once", "--now", &now]);
    assert!(o.status.success(), "{o:?}");
}

/// A workspace: `ileri` runs on a webhook, `bekle` waits for `{"sipariş": 43}`.
fn forwarding(d: &Path) {
    file(d, "itero.config.json", "{}");
    file(
        d,
        "workflows/ileri.json",
        r##"{ "on": [{ "event": "itero.webhook.received" }],
             "steps": [{ "#yaz": { "args": { "yol": "$event.path" } } }] }"##,
    );
    file(
        d,
        "actions/yaz",
        "#!/bin/sh\nprintf '%s' \"$yol\" > yazıldı.txt\n",
    );
    file(
        d,
        "workflows/bekle.json",
        r##"{ "on": [{ "event": "sipariş.verildi" }], "steps": ["#bekle"] }"##,
    );
    file(
        d,
        "actions/bekle",
        "#!/bin/sh\nif [ -n \"${event:-}\" ]; then echo uyandı > uyandı.txt; exit 0; fi\necho '{\"sipariş\": 43}'\nexit 75\n",
    );
}

#[test]
fn a_webhook_starts_a_workflow_and_wakes_no_waiting_step() {
    let d = tempfile::tempdir().unwrap();
    forwarding(d.path());
    assert!(
        itero(d.path(), &["ws", "emit", "sipariş.verildi"])
            .status
            .success()
    );
    pass(d.path());
    let ws = itero::workspace::find(d.path()).unwrap();
    let now = itero::engine::daemon::stamp(chrono::Utc::now());
    // a stranger's data, with the very field `bekle` waits on
    let data = json!({ "path": "/sipariş", "sipariş": 43, "headers": {}, "body": {} });
    itero::engine::events::webhook_received(&ws, data.clone(), &now).unwrap();
    pass(d.path());
    assert_eq!(
        std::fs::read_to_string(d.path().join("yazıldı.txt")).unwrap(),
        "/sipariş"
    );
    assert!(
        !d.path().join("uyandı.txt").exists(),
        "a webhook woke a waiting step"
    );
    // the same data as any other event does wake it: what the test guards
    let o = itero(d.path(), &["ws", "emit", "kargo.geldi", &data.to_string()]);
    assert!(o.status.success(), "{o:?}");
    pass(d.path());
    assert!(d.path().join("uyandı.txt").exists());
}

#[test]
fn nothing_local_can_send_a_webhook_event() {
    let d = tempfile::tempdir().unwrap();
    forwarding(d.path());
    let o = itero(d.path(), &["ws", "emit", "itero.webhook.received", "{}"]);
    assert!(!o.status.success(), "{o:?}");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(o.status.success(), "a workflow may listen to it: {o:?}");
}

use itero::connectors::webhook::check::{Check, passes};

const GITHUB_SECRET: &str = "It's a Secret to Everybody";
const GITHUB_SIGNATURE: &str =
    "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";

#[test]
fn hmac_is_github_s_signature_of_the_raw_body() {
    let ok = |given, body: &[u8]| passes(Check::Hmac, GITHUB_SECRET, given, "", body);
    assert!(ok(Some(GITHUB_SIGNATURE), b"Hello, World!"));
    assert!(
        !ok(Some(GITHUB_SIGNATURE), b"Hello, World?"),
        "another body"
    );
    assert!(
        !ok(
            Some(GITHUB_SIGNATURE.trim_start_matches("sha256=")),
            b"Hello, World!"
        ),
        "no prefix"
    );
    assert!(!ok(Some("sha256=zz"), b"Hello, World!"), "not hex");
    assert!(!ok(Some("sha256=şş"), b"Hello, World!"), "not ASCII");
    assert!(!ok(Some("sha256=757"), b"Hello, World!"), "an odd length");
    assert!(!ok(Some("sha256="), b"Hello, World!"), "empty");
    assert!(!ok(None, b"Hello, World!"), "no header");
}

#[test]
fn twilio_signs_the_url_and_the_sorted_fields() {
    let url = "https://example.com/myapp.php?foo=1&bar=2";
    let body = b"To=%2B18005551212&CallSid=CA1234567890ABCDE&Caller=%2B14158675310&Digits=1234&From=%2B14158675310";
    let ok = |given, url| passes(Check::Twilio, "12345", given, url, body);
    assert!(ok(Some("L/OH5YylLD5NRKLltdqwSvS0BnU="), url));
    assert!(
        !ok(
            Some("L/OH5YylLD5NRKLltdqwSvS0BnU="),
            "https://example.com/başka"
        ),
        "another URL"
    );
    assert!(!ok(Some("şifre değil"), url), "not base64");
    assert!(!ok(None, url), "no header");
}

#[test]
fn a_token_is_a_bearer_token_equal_to_the_secret() {
    let secret = "bir-uzun-gizli-değer";
    let ok = |given| passes(Check::Token, secret, given, "", b"");
    assert!(ok(Some("Bearer bir-uzun-gizli-değer")));
    assert!(!ok(Some("Bearer bir-uzun-gizli-değe")), "shorter");
    assert!(!ok(Some("bir-uzun-gizli-değer")), "no Bearer");
    assert!(!ok(None), "no header");
}

#[test]
fn a_check_goes_by_its_name() {
    assert_eq!(Check::named("hmac"), Some(Check::Hmac));
    assert_eq!(Check::named("token"), Some(Check::Token));
    assert_eq!(Check::named("twilio"), Some(Check::Twilio));
    assert_eq!(Check::named("github"), None);
}

use axum::http::{HeaderMap, HeaderValue};
use itero::connectors::webhook::{BadBody, Hook, event_data, hooks};

const LONG: &str = "bir-uzun-gizli-değer";

fn told(given: &[&str], set: &[(&str, &str)]) -> Result<Vec<Hook>, Vec<String>> {
    let given: Vec<String> = given.iter().map(|s| s.to_string()).collect();
    hooks(&given, &|n| {
        set.iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.to_string())
    })
}

#[test]
fn hooks_are_a_path_a_check_and_a_secret_s_name() {
    let Ok(h) = told(
        &["/kargo:hmac:KARGO", "/uygulama:token:UYGULAMA"],
        &[("KARGO", LONG), ("UYGULAMA", LONG)],
    ) else {
        panic!("refused");
    };
    assert_eq!(h[0].path, "/kargo");
    assert_eq!(h[0].check, Check::Hmac);
    assert_eq!(h[0].secret, LONG);
    assert_eq!(h[1].check, Check::Token);
}

#[test]
fn every_problem_with_the_hooks_is_told_at_once_and_no_value() {
    let Err(e) = told(&[], &[]) else {
        panic!("no hook, and it started");
    };
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains("--hook"), "{e:?}");
    let given = [
        "yolsuz",            // not three parts
        "kargo:hmac:KARGO",  // no leading /
        "/a:açık:KARGO",     // no such check
        "/b:hmac:KAR-GO",    // not a secret's name
        "/c:hmac:YOK",       // not set
        "/d:hmac:KISA",      // too short
        "/e?x=1:hmac:KARGO", // a query is no path
        "/f:hmac:KARGO",
        "/f:token:KARGO", // a repeated path
    ];
    let Err(e) = told(&given, &[("KARGO", LONG), ("KISA", "kısa")]) else {
        panic!("all those, and it started");
    };
    assert_eq!(e.len(), 8, "{e:#?}");
    for needle in [
        "yolsuz", "kargo:", "açık", "KAR-GO", "YOK", "KISA", "/e?x", "/f",
    ] {
        assert!(e.iter().any(|p| p.contains(needle)), "{needle}: {e:#?}");
    }
    assert!(!e.join("\n").contains("kısa"), "a value told: {e:#?}");
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.append(*k, HeaderValue::from_str(v).unwrap());
    }
    h
}

#[test]
fn the_event_has_the_path_the_headers_and_the_body() {
    let h = headers(&[
        ("content-type", "application/json; charset=utf-8"),
        ("user-agent", "kargo/1"),
    ]);
    let v = event_data(
        "/kargo",
        Check::Hmac,
        &h,
        r#"{"şehir": "İzmir"}"#.as_bytes(),
    )
    .unwrap();
    assert_eq!(v["path"], "/kargo");
    assert_eq!(v["body"]["şehir"], "İzmir");
    assert_eq!(v["headers"]["user-agent"], "kargo/1");
    assert_eq!(v.as_object().unwrap().len(), 3);
}

#[test]
fn what_proves_a_request_is_never_saved() {
    let h = headers(&[
        ("content-type", "text/plain"),
        ("authorization", "Bearer gizli"),
        ("cookie", "a=b"),
        ("proxy-authorization", "gizli"),
        ("x-hub-signature-256", "sha256=ab"),
        ("x-twilio-signature", "ab"),
    ]);
    for check in [Check::Token, Check::Hmac, Check::Twilio] {
        let v = event_data("/a", check, &h, b"merhaba").unwrap();
        let text = v.to_string();
        assert!(
            !text.contains("gizli") && !text.contains("cookie"),
            "{text}"
        );
        assert!(v["headers"].get(check.header()).is_none(), "{text}");
    }
}

#[test]
fn a_repeated_header_is_joined() {
    let h = headers(&[
        ("content-type", "text/plain"),
        ("x-yol", "a"),
        ("x-yol", "b"),
    ]);
    let v = event_data("/a", Check::Hmac, &h, b"").unwrap();
    assert_eq!(v["headers"]["x-yol"], "a, b");
}

#[test]
fn a_body_is_json_a_form_or_text() {
    let form = headers(&[("content-type", "application/x-www-form-urlencoded")]);
    let v = event_data(
        "/a",
        Check::Twilio,
        &form,
        "From=%2B90&Body=g%C3%BCnayd%C4%B1n&M=1&M=2&M=3".as_bytes(),
    )
    .unwrap();
    assert_eq!(
        v["body"],
        json!({"From": "+90", "Body": "günaydın", "M": ["1", "2", "3"]})
    );
    let text = headers(&[("content-type", "text/plain")]);
    assert_eq!(
        event_data("/a", Check::Hmac, &text, "günaydın".as_bytes()).unwrap()["body"],
        "günaydın"
    );
    assert_eq!(
        event_data("/a", Check::Hmac, &HeaderMap::new(), b"x").unwrap()["body"],
        "x"
    );
    let js = headers(&[("content-type", "application/json")]);
    assert_eq!(
        event_data("/a", Check::Hmac, &js, b"[1]").unwrap()["body"],
        json!([1])
    );
    assert!(matches!(
        event_data("/a", Check::Hmac, &js, b"{bozuk"),
        Err(BadBody::NotJson)
    ));
    assert!(matches!(
        event_data("/a", Check::Hmac, &text, &[0xff, 0xfe]),
        Err(BadBody::NotText)
    ));
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `itero webhook` in `d`, up. Its stdin stays open while this lives.
struct Listener {
    child: Child,
    port: u16,
    /// Its stderr, past the line that told the port.
    err: BufReader<std::process::ChildStderr>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        drop(self.child.stdin.take()); // it ends with its stdin
        let _ = self.child.wait();
    }
}

impl Listener {
    /// Ends it, and what it said on stderr after that line.
    fn said(&mut self) -> String {
        drop(self.child.stdin.take());
        let mut err = String::new();
        self.err.read_to_string(&mut err).unwrap();
        err
    }
}

/// On a port the OS gives: a free port asked for first could be another
/// test's by the time it is bound, and that test's listener answer for this
/// one. What it said before that line goes in `told`, for the tests of it.
fn listen_telling(d: &Path, extra: &[&str], envs: &[(&str, &str)]) -> (Listener, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["webhook", "--port", "0"])
        .args(extra)
        .current_dir(d)
        .env("ITERO_NO_SECRET_STORE", "1")
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut told = String::new();
    let port = loop {
        let mut line = String::new();
        assert!(
            err.read_line(&mut line).unwrap() > 0,
            "never listened: {told}"
        );
        if let Some(rest) = line.split("listening on ").nth(1) {
            let at = rest.split(':').nth(1).unwrap();
            break at.trim().parse().unwrap();
        }
        told.push_str(&line);
    };
    (Listener { child, port, err }, told)
}

fn listen(d: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Listener {
    listen_telling(d, extra, envs).0
}

/// Sends a request as written; its status and body.
fn send(port: u16, raw: &[u8]) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).unwrap_or("0").parse().unwrap_or(0);
    let body = out.split_once("\r\n\r\n").map_or("", |x| x.1).to_string();
    (status, body)
}

fn post(path: &str, headers: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut r = format!(
        "POST {path} HTTP/1.1\r\nHost: webhook.example.com\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        r.push_str(&format!("{k}: {v}\r\n"));
    }
    r.push_str("\r\n");
    r.push_str(body);
    r.into_bytes()
}

fn signed(secret: &str, body: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
    let tag = ring::hmac::sign(&key, body.as_bytes());
    let hex: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

/// The events waiting in the journal, as `(type, data)`.
fn saved(d: &Path) -> Vec<(String, Value)> {
    let o = itero(d, &["ws", "ev", "list", "--json"]);
    assert!(o.status.success(), "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| (e["type"].as_str().unwrap().to_string(), e["data"].clone()))
        .collect()
}

fn hooked(d: &Path) -> Listener {
    file(d, "itero.config.json", "{}");
    file(
        d,
        ".env",
        &format!("KARGO={LONG}\nUYGULAMA={LONG}\nSMS={LONG}\n"),
    );
    listen(
        d,
        &[
            "--hook",
            "/kargo:hmac:KARGO",
            "--hook",
            "/uygulama:token:UYGULAMA",
            "--hook",
            "/sms:twilio:SMS",
        ],
        &[],
    )
}

const JSON_TYPE: (&str, &str) = ("Content-Type", "application/json");

#[test]
fn a_checked_request_is_saved_and_answered_empty() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    let body = r#"{"şehir":"İzmir"}"#;
    let sign = signed(LONG, body);
    let (status, answer) = send(
        l.port,
        &post("/kargo", &[JSON_TYPE, ("X-Hub-Signature-256", &sign)], body),
    );
    assert_eq!((status, answer.as_str()), (200, ""));
    let e = saved(d.path());
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0].0, "itero.webhook.received");
    assert_eq!(e[0].1["path"], "/kargo");
    assert_eq!(e[0].1["body"]["şehir"], "İzmir");
    assert!(e[0].1["headers"].get("x-hub-signature-256").is_none());
    assert_eq!(e[0].1["headers"]["host"], "webhook.example.com");
}

#[test]
fn twilio_is_checked_against_the_address_it_was_sent_to() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), ".env", "SMS=on-altı-karakterli-belirteç\n");
    let l = listen(d.path(), &["--hook", "/sms:twilio:SMS"], &[]);
    let body = "From=%2B905550000000&Body=g%C3%BCnayd%C4%B1n";
    let text = "https://webhook.example.com/sms?a=1Bodygünaydın\
                From+905550000000";
    let key = ring::hmac::Key::new(
        ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        "on-altı-karakterli-belirteç".as_bytes(),
    );
    let tag = ring::hmac::sign(&key, text.as_bytes());
    use base64::Engine;
    let sign = base64::engine::general_purpose::STANDARD.encode(tag.as_ref());
    let form = ("Content-Type", "application/x-www-form-urlencoded");
    let raw = post("/sms?a=1", &[form, ("X-Twilio-Signature", &sign)], body);
    assert_eq!(send(l.port, &raw).0, 200);
    let e = saved(d.path());
    assert_eq!(e[0].1["body"]["Body"], "günaydın");
    // the same signature, sent on to another address
    let moved = String::from_utf8(raw)
        .unwrap()
        .replace("webhook.example.com", "başka.example.com");
    assert_eq!(send(l.port, moved.as_bytes()).0, 401);
    assert_eq!(saved(d.path()).len(), 1);
}

#[test]
fn what_fails_is_refused_and_saves_nothing() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    let body = r#"{"a":1}"#;
    let sign = signed(LONG, body);
    let code = |raw: Vec<u8>| send(l.port, &raw).0;
    assert_eq!(
        code(post("/kargo", &[JSON_TYPE], body)),
        401,
        "no signature"
    );
    assert_eq!(
        code(post(
            "/kargo",
            &[
                JSON_TYPE,
                (
                    "X-Hub-Signature-256",
                    &signed("başka-bir-gizli-değer", body)
                )
            ],
            body
        )),
        401,
        "another secret"
    );
    assert_eq!(
        code(post(
            "/uygulama",
            &[JSON_TYPE, ("Authorization", "Bearer yanlış")],
            body
        )),
        401,
        "a wrong token"
    );
    assert_eq!(
        code(post(
            "/yok",
            &[JSON_TYPE, ("X-Hub-Signature-256", &sign)],
            body
        )),
        404,
        "not listed"
    );
    assert_eq!(
        code(post("/health", &[], "")),
        404,
        "the daemon's route is not here"
    );
    assert_eq!(
        code(post(
            "/kargo",
            &[JSON_TYPE, ("X-Hub-Signature-256", &signed(LONG, "{bozuk"))],
            "{bozuk"
        )),
        400,
        "not JSON"
    );
    assert_eq!(
        code(post(
            "/sms",
            &[JSON_TYPE, ("X-Twilio-Signature", "ab")],
            body
        )),
        400,
        "twilio takes a form"
    );
    assert_eq!(
        code(b"PUT /kargo HTTP/1.1\r\nHost: a\r\nContent-Length: 0\r\n\r\n".to_vec()),
        405
    );
    assert_eq!(
        code(post(
            "/kargo",
            &[("Content-Type", "text/plain"), ("X-Hub-Signature-256", "")],
            ""
        )),
        401,
        "an empty signature"
    );
    assert!(saved(d.path()).is_empty(), "{:?}", saved(d.path()));
}

#[test]
fn a_body_not_text_is_refused_after_its_check() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    // signed by hand: the signature is over these bytes
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, LONG.as_bytes());
    let hex: String = ring::hmac::sign(&key, &[0xff, 0xfe])
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut raw = format!(
        "POST /kargo HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\nX-Hub-Signature-256: sha256={hex}\r\n\r\n"
    )
    .into_bytes();
    raw.extend_from_slice(&[0xff, 0xfe]);
    assert_eq!(send(l.port, &raw).0, 415);
    assert!(saved(d.path()).is_empty());
}

#[test]
fn an_hmac_hook_answers_meta_s_handshake_with_digits_only() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    let get = |q: &str| {
        send(
            l.port,
            format!("GET {q} HTTP/1.1\r\nHost: a\r\n\r\n").as_bytes(),
        )
    };
    assert_eq!(
        get("/kargo?hub.mode=subscribe&hub.verify_token=x&hub.challenge=1158201444"),
        (200, "1158201444".to_string())
    );
    assert_eq!(
        get("/kargo?hub.mode=subscribe&hub.challenge=%3Cscript%3E").0,
        400,
        "not digits"
    );
    assert_eq!(get("/kargo").0, 405, "no handshake");
    assert_eq!(
        get("/uygulama?hub.mode=subscribe&hub.challenge=1").0,
        405,
        "not an hmac hook"
    );
    assert!(saved(d.path()).is_empty());
}

#[test]
fn a_problem_keeps_it_from_starting_and_names_no_value() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), ".env", "KISA=kısa-değer\n");
    let o = itero(
        d.path(),
        &[
            "webhook",
            "--port",
            "1",
            "--hook",
            "/a:hmac:YOK",
            "--hook",
            "/b:hmac:KISA",
        ],
    );
    assert!(!o.status.success(), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("YOK") && err.contains("KISA"), "{err}");
    assert!(!err.contains("kısa-değer"), "{err}");
}

/// A daemon the test started, gone with the test, also when it fails.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = (self.0.kill(), self.0.wait());
    }
}

#[test]
fn two_listeners_run_as_connectors_of_one_daemon() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (free_port(), free_port());
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), ".env", &format!("KARGO={LONG}\n"));
    file(
        d.path(),
        "connectors/webhook/webhook.listen",
        &format!("#!/bin/sh\nexec itero webhook --port {a} --hook /kargo:hmac:KARGO\n"),
    );
    file(
        d.path(),
        "connectors/webhook/webhook.listen-ikinci",
        &format!("#!/bin/sh\nexec itero webhook --port {b} --hook /ikinci:hmac:KARGO\n"),
    );
    file(
        d.path(),
        "workflows/ileri.json",
        r##"{ "on": [{ "event": "itero.webhook.received" }],
             "steps": [{ "#yaz": { "args": { "yol": "$event.path" } } }] }"##,
    );
    file(
        d.path(),
        "actions/yaz",
        "#!/bin/sh\nprintf '%s\\n' \"$yol\" >> yazıldı.txt\n",
    );
    // no API: a webhook wakes the daemon through a file, and an API on a
    // port the OS gives could be given `a` or `b`, free again since
    // free_port let them go; the request would then reach the daemon
    let daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["daemon", "--no-serve"])
            .current_dir(d.path())
            .env("ITERO_NO_SECRET_STORE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for (port, path) in [(a, "/kargo"), (b, "/ikinci")] {
        let end = Instant::now() + Duration::from_secs(20);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < end, "{path} never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        let sign = signed(LONG, "{}");
        let raw = post(path, &[JSON_TYPE, ("X-Hub-Signature-256", &sign)], "{}");
        assert_eq!(send(port, &raw).0, 200);
    }
    let end = Instant::now() + Duration::from_secs(20);
    let written = || std::fs::read_to_string(d.path().join("yazıldı.txt")).unwrap_or_default();
    while written().lines().count() < 2 {
        assert!(Instant::now() < end, "only: {}", written());
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(daemon);
    // the connectors end with their stdin; their ports come free
    let end = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", a)).is_ok() {
        assert!(
            Instant::now() < end,
            "still listening after the daemon went"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_flood_on_one_hook_leaves_the_others_answering() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    // 32 requests that never finish their bodies hold /kargo's places
    let mut held = vec![];
    for _ in 0..32 {
        let mut s = TcpStream::connect(("127.0.0.1", l.port)).unwrap();
        s.write_all(b"POST /kargo HTTP/1.1\r\nHost: a\r\nContent-Length: 100\r\n\r\nbir")
            .unwrap();
        held.push(s);
    }
    std::thread::sleep(Duration::from_millis(500));
    let sign = signed(LONG, "{}");
    let kargo = post("/kargo", &[JSON_TYPE, ("X-Hub-Signature-256", &sign)], "{}");
    assert_eq!(send(l.port, &kargo).0, 503);
    let token = format!("Bearer {LONG}");
    let other = post("/uygulama", &[JSON_TYPE, ("Authorization", &token)], "{}");
    assert_eq!(send(l.port, &other).0, 200, "another hook goes on");
    drop(held);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(send(l.port, &kargo).0, 200, "the places come back");
}

#[test]
fn a_body_over_the_limit_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let l = hooked(d.path());
    let big = "ş".repeat(3 * 1024 * 1024); // past 5 MiB in bytes
    let sign = signed(LONG, &big);
    let raw = post(
        "/kargo",
        &[
            ("Content-Type", "text/plain"),
            ("X-Hub-Signature-256", &sign),
        ],
        &big,
    );
    let mut s = TcpStream::connect(("127.0.0.1", l.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = s.write_all(&raw); // it may close while this writes
    let mut out = vec![];
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out);
    assert!(
        out.starts_with("HTTP/1.1 413"),
        "{}",
        out.lines().next().unwrap_or("")
    );
    assert!(saved(d.path()).is_empty());
}

#[test]
fn a_connection_that_stays_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), ".env", &format!("KARGO={LONG}\n"));
    let l = listen(
        d.path(),
        &["--hook", "/kargo:hmac:KARGO"],
        &[("ITERO_WEBHOOK_LIFE_SECS", "1")],
    );
    for first in [&b""[..], &b"POST /kargo HTTP/1.1\r\nHost:"[..]] {
        let mut s = TcpStream::connect(("127.0.0.1", l.port)).unwrap();
        s.write_all(first).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let began = Instant::now();
        let mut out = vec![];
        let _ = s.read_to_end(&mut out);
        assert!(began.elapsed() < Duration::from_secs(4), "still open");
    }
}

#[test]
fn a_refusal_is_told_once_and_holds_nothing_of_the_stranger_s() {
    let d = tempfile::tempdir().unwrap();
    let mut l = hooked(d.path());
    for _ in 0..50 {
        send(
            l.port,
            &post(
                "/kargo",
                &[("X-Hub-Signature-256", "sha256=gizli-değil")],
                "gövde-metni",
            ),
        );
        send(l.port, &post("/yabancı-yol", &[], ""));
    }
    let err = l.said();
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 2, "{err}");
    for hidden in ["gizli-değil", "gövde-metni", "yabancı-yol"] {
        assert!(!err.contains(hidden), "{hidden}: {err}");
    }
}

#[test]
fn another_address_is_the_user_s_and_is_warned_about() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), ".env", &format!("KARGO={LONG}\n"));
    let (l, told) = listen_telling(
        d.path(),
        &["--host", "0.0.0.0", "--hook", "/kargo:hmac:KARGO"],
        &[],
    );
    let sign = signed(LONG, "{}");
    let raw = post("/kargo", &[JSON_TYPE, ("X-Hub-Signature-256", &sign)], "{}");
    assert_eq!(send(l.port, &raw).0, 200);
    assert!(told.contains("not encrypted"), "{told}");
}
