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

#[path = "common/web.rs"]
mod web;

/// A list the test changes, through a connection of its own.
const LISTE: &str = r#"<!doctype html><title>Liste</title>
<ul aria-label="Liste"><li>Bir</li></ul>
<script>window.add = t => { const li = document.createElement('li'); li.textContent = t; document.querySelector('ul').append(li); };</script>"#;

/// The watched page, from a connection of the test's own, once it has
/// loaded and the watcher has had a moment to look.
fn watched(state: &Path, path: &str) -> itero::browser::cdp::Cdp {
    let active = state.join("itero/browser/headless/DevToolsActivePort");
    until("Chrome started", 30, || active.exists());
    let port = std::fs::read_to_string(&active).unwrap();
    let port = port.lines().next().unwrap().trim().to_string();
    let mut ws = String::new();
    until("the page opened", 30, || {
        let list: Value = client()
            .get(format!("http://127.0.0.1:{port}/json/list"))
            .send()
            .and_then(|r| r.json())
            .unwrap_or(json!([]));
        ws = list
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["url"].as_str().unwrap_or_default().ends_with(path))
            .map(|t| t["webSocketDebuggerUrl"].as_str().unwrap().to_string())
            .unwrap_or_default();
        !ws.is_empty()
    });
    let mut c = itero::browser::cdp::Cdp::connect(&ws, Duration::from_secs(10)).unwrap();
    until("the page loaded", 30, || {
        c.eval("document.readyState").unwrap_or_default() == "complete"
    });
    std::thread::sleep(Duration::from_millis(1500));
    c
}

/// The events of `kind` the daemon took, their data.
fn events(root: &Path, kind: &str) -> Vec<Value> {
    let ws = itero::workspace::find(root).unwrap();
    itero::engine::journal::Journal::open(&ws)
        .unwrap()
        .events(None)
        .unwrap()
        .into_iter()
        .filter(|r| r.event.kind == kind)
        .map(|r| r.event.data)
        .collect()
}

#[test]
fn a_watched_pages_changes_are_a_connectors_events() {
    let d = tempfile::tempdir().unwrap();
    let w = web::serve(vec![web::html("/liste", LISTE)]);
    let root = workspace(
        d.path(),
        free_port(),
        "sayfa.changed",
        "#!/bin/sh\nexec itero browser watch \"$URL\" --changed Liste --navigated --headless\n",
        json!({ "URL": w.url("/liste") }),
    );
    let daemon = daemon(&root, d.path());
    let mut page = watched(&d.path().join("durum"), "/liste");
    page.eval("add('İki')").unwrap();
    until("the first change", 20, || {
        events(&root, "sayfa.changed").len() == 1
    });
    // three in a burst: one event, after a second of quiet
    page.eval(
        "add('Üç'); setTimeout(() => add('Dört'), 200); setTimeout(() => add('Beş'), 400); 1",
    )
    .unwrap();
    until("the burst", 20, || {
        events(&root, "sayfa.changed").len() == 2
    });
    page.eval("history.pushState({}, '', '/liste/2'); document.title = 'İkinci'; 1")
        .unwrap();
    until("the navigation", 20, || {
        events(&root, "sayfa.changed").len() == 3
    });
    std::thread::sleep(Duration::from_secs(2));
    let got = events(&root, "sayfa.changed");
    let what: Vec<&str> = got.iter().map(|e| e["what"].as_str().unwrap()).collect();
    assert_eq!(what, ["changed", "changed", "navigated"], "{got:?}");
    // the second is the burst, once, after a second of quiet
    assert_eq!(got[0]["text"], "Bir\nİki", "{got:?}");
    assert_eq!(got[0]["previous"], "Bir", "{got:?}");
    assert_eq!(got[1]["text"], "Bir\nİki\nÜç\nDört\nBeş", "{got:?}");
    assert_eq!(got[1]["target"], "Liste");
    assert_eq!(got[2]["url"], w.url("/liste/2"));
    assert_eq!(got[2]["title"], "İkinci");
    let tab = got[0]["tab"].as_str().unwrap().to_string();
    assert!(tab.starts_with("ws://127.0.0.1:"), "{tab}");
    assert_eq!(got[0]["url"], w.url("/liste"));

    // the daemon stops: its connector, the window and Chrome go with it
    drop(daemon);
    let chrome = tab
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    assert!(
        client()
            .get(format!("http://{chrome}/json/version"))
            .send()
            .is_err()
    );
}

#[test]
fn watch_runs_only_as_a_connector_of_a_daemon() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["browser", "watch", "https://ışık.example/", "--navigated"])
        .env_remove("ITERO_BROWSER_API")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("runs as a connector"), "{e}");
}

#[test]
fn watch_needs_something_to_watch() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["browser", "watch", "https://ışık.example/"])
        .env("ITERO_BROWSER_API", "http://127.0.0.1:9/browser")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(
        e.contains("--changed, --appeared, --navigated or --responses"),
        "{e}"
    );
}

#[test]
fn what_appears_and_the_json_a_page_loads_are_events_too() {
    let d = tempfile::tempdir().unwrap();
    let w = web::serve(vec![
        web::html(
            "/haber",
            r#"<!doctype html><title>Haberler</title><p>Bekleniyor</p>
<script>window.go = () => { fetch('/api/haber?n=1');
  const p = document.createElement('p'); p.id = 'yeni'; p.textContent = 'Geldi'; document.body.append(p); };</script>"#,
        ),
        web::file("/api/haber", "application/json", r#"{"başlık": "Işık"}"#),
    ]);
    let root = workspace(
        d.path(),
        free_port(),
        "haber.oldu",
        "#!/bin/sh\nexec itero browser watch \"$URL\" --appeared '{\"css\": \"#yeni\"}' --responses /api/haber --headless\n",
        json!({ "URL": w.url("/haber") }),
    );
    let _daemon = daemon(&root, d.path());
    let mut page = watched(&d.path().join("durum"), "/haber");
    page.eval("go(); 1").unwrap();
    until("two events", 20, || events(&root, "haber.oldu").len() == 2);
    let got = events(&root, "haber.oldu");
    let find = |what: &str| {
        got.iter()
            .find(|e| e["what"] == what)
            .unwrap_or_else(|| panic!("{what}: {got:?}"))
    };
    let appeared = find("appeared");
    assert_eq!(appeared["text"], "Geldi");
    assert_eq!(appeared["target"], json!({ "css": "#yeni" }));
    let response = find("response");
    assert_eq!(response["json"], json!({ "başlık": "Işık" }));
    assert_eq!(response["response_url"], w.url("/api/haber?n=1"));
    assert_eq!(response["url"], w.url("/haber"));
}
