//! Webhooks: `itero webhook` takes checked requests from outside and saves
//! them as `itero.webhook.received`.
#![cfg(unix)]
use serde_json::json;
use std::path::Path;
use std::process::Output;

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
