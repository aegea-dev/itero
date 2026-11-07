//! A page watched by a connector: the daemon gives connectors a browser
//! window of their own (`ITERO_BROWSER_API`).
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use stop::Daemon;

fn until(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !ok() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A workspace on `port` whose connector `kind` runs `script`, started
/// by a workflow `on` it with `args`.
fn workspace(d: &Path, port: u16, kind: &str, script: &str, args: Value) -> std::path::PathBuf {
    let root = d.join("çalışma");
    std::fs::create_dir_all(root.join("workflows")).unwrap();
    std::fs::create_dir_all(root.join("connectors")).unwrap();
    std::fs::write(
        root.join("itero.config.json"),
        json!({ "port": port }).to_string(),
    )
    .unwrap();
    let file = root.join("connectors").join(kind);
    std::fs::write(&file, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        root.join("workflows/izle.json"),
        json!({
            "on": [{ "event": kind, "args": args }],
            "steps": [{ "#notify": { "args": { "text": "$event.text" } } }]
        })
        .to_string(),
    )
    .unwrap();
    root
}

fn daemon(root: &Path, d: &Path) -> Daemon {
    Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(root)
            .env("XDG_STATE_HOME", d.join("durum"))
            .env("ITERO_TEST_BROWSER_HEADLESS", "1")
            .env("ITERO_TEST_BROWSER_OFFLINE", "1")
            .env("ITERO_NOTIFY_LOG", d.join("bildirim.log"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .spawn()
            .unwrap(),
    )
}

fn client() -> reqwest::blocking::Client {
    itero::net::client(Duration::from_secs(30)).unwrap()
}

/// The pages of the Chrome on `port`, by id.
fn pages(port: &str) -> Vec<String> {
    let list: Value = client()
        .get(format!("http://{port}/json/list"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .filter(|t| t["type"] == "page")
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_connector_asks_the_daemon_for_a_window_and_gives_it_back() {
    let d = tempfile::tempdir().unwrap();
    let port = free_port();
    let seen = d.path().join("api.txt");
    let root = workspace(
        d.path(),
        port,
        "sayfa.changed",
        "#!/bin/sh\nprintf '%s' \"$ITERO_BROWSER_API\" > \"$OUT\"\nexec cat >/dev/null\n",
        json!({ "OUT": seen.display().to_string() }),
    );
    let mut daemon = daemon(&root, d.path());
    until("the connector ran", 20, || seen.exists());
    let api = std::fs::read_to_string(&seen).unwrap();
    assert_eq!(api, format!("http://127.0.0.1:{port}/browser"));

    let r = client()
        .post(format!("{api}/tabs"))
        .json(&json!({ "headless": true }))
        .send()
        .unwrap();
    assert_eq!(r.status(), 201, "{:?}", r.text());
    let tab: Value = r.json().unwrap();
    assert_eq!(tab["mode"], "headless", "{tab}");
    let ws = tab["ws"].as_str().unwrap();
    let chrome = ws
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let id = tab["id"].as_str().unwrap();
    assert!(pages(&chrome).contains(&id.to_string()));
    let mut c = itero::browser::cdp::Cdp::connect(ws, Duration::from_secs(5)).unwrap();
    assert_eq!(c.eval("document.visibilityState").unwrap(), "visible");
    drop(c);

    // given back: closed
    let r = client().delete(format!("{api}/tabs/{id}")).send().unwrap();
    assert_eq!(r.status(), 200, "{:?}", r.text());
    until("the window closed", 10, || {
        !pages(&chrome).contains(&id.to_string())
    });
    let r = client().delete(format!("{api}/tabs/{id}")).send().unwrap();
    assert_eq!(r.status(), 404);

    // one left open goes with the daemon, and its Chrome too
    let r = client()
        .post(format!("{api}/tabs"))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(r.status(), 201);
    stop::stop(&mut daemon.0);
    assert!(
        client()
            .get(format!("http://{chrome}/json/version"))
            .send()
            .is_err(),
        "Chrome stopped with the daemon"
    );
}
