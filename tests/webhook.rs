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
