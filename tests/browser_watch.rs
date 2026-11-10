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
    let steps = json!([{ "#notify": { "args": { "text": "$event.what" } } }]);
    workspace_with(d, port, kind, script, args, steps)
}

/// `workspace`, its workflow doing `steps`.
fn workspace_with(
    d: &Path,
    port: u16,
    kind: &str,
    script: &str,
    args: Value,
    steps: Value,
) -> std::path::PathBuf {
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
            "steps": steps
        })
        .to_string(),
    )
    .unwrap();
    root
}

fn daemon(root: &Path, d: &Path) -> Daemon {
    Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["ws", "daemon"])
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

/// A window from the daemon at `api`: the response, held, keeps it open;
/// its first line says which.
fn open(api: &str, body: Value) -> (impl std::io::BufRead, Value) {
    use std::io::BufRead;
    let r = client()
        .post(format!("{api}/tabs"))
        .json(&body)
        .send()
        .unwrap();
    assert_eq!(r.status(), 201);
    let mut held = std::io::BufReader::new(r);
    let mut line = String::new();
    held.read_line(&mut line).unwrap();
    let tab: Value = serde_json::from_str(&line).unwrap();
    (held, tab)
}

/// The daemon's browser API, as the connector of the workspace at `root`
/// wrote it to `seen`, once it ran.
fn api_of(seen: &Path) -> String {
    // the shell makes the file, then writes in it
    until("the connector ran", 20, || {
        std::fs::read_to_string(seen).is_ok_and(|t| !t.is_empty())
    });
    std::fs::read_to_string(seen).unwrap()
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
    let api = api_of(&seen);
    assert_eq!(api, format!("http://127.0.0.1:{port}/browser"));

    let (held, tab) = open(&api, json!({ "headless": true }));
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

    // given back, by ending the connection: closed
    drop(held);
    until("the window closed", 10, || {
        !pages(&chrome).contains(&id.to_string())
    });

    // one held open goes with the daemon, and its Chrome too
    let (_held, _) = open(&api, json!({}));
    stop::stop(&mut daemon.0);
    assert!(
        client()
            .get(format!("http://{chrome}/json/version"))
            .send()
            .is_err(),
        "Chrome stopped with the daemon"
    );
}

/// A watch that dies with no word (`kill -9`, a crash): its window is
/// closed all the same, since it lives as long as the watch's connection.
#[test]
fn a_killed_watchs_window_is_closed() {
    let d = tempfile::tempdir().unwrap();
    let w = web::serve(vec![web::html("/liste", LISTE)]);
    let seen = d.path().join("api.txt");
    let root = workspace(
        d.path(),
        free_port(),
        "sayfa.changed",
        "#!/bin/sh\nprintf '%s' \"$ITERO_BROWSER_API\" > \"$OUT\"\nexec cat >/dev/null\n",
        json!({ "OUT": seen.display().to_string() }),
    );
    let _daemon = daemon(&root, d.path());
    let api = api_of(&seen);
    let mut watch = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args([
            "browser",
            "watch",
            &w.url("/liste"),
            "--navigated",
            "--headless",
        ])
        .env("ITERO_BROWSER_API", &api)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _stdin = watch.stdin.take();
    drop(browser::watched(&d.path().join("durum"), "/liste"));
    let active = d
        .path()
        .join("durum/itero/browser/headless/DevToolsActivePort");
    let port = std::fs::read_to_string(active).unwrap();
    let chrome = format!("127.0.0.1:{}", port.lines().next().unwrap().trim());
    let liste = || {
        let list: Value = client()
            .get(format!("http://{chrome}/json/list"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .any(|t| t["url"].as_str().unwrap_or_default().ends_with("/liste"))
    };
    assert!(liste());
    // SAFETY: a plain kill of our own child
    unsafe { libc::kill(watch.id() as i32, libc::SIGKILL) };
    watch.wait().unwrap();
    until("the window closed", 10, || !liste());
}

#[path = "common/web.rs"]
mod web;

/// A list the test changes, through a connection of its own.
const LISTE: &str = r#"<!doctype html><title>Liste</title>
<ul aria-label="Liste"><li>Bir</li></ul>
<script>window.add = t => { const li = document.createElement('li'); li.textContent = t; document.querySelector('ul').append(li); };</script>"#;

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
    let mut page = browser::watched(&d.path().join("durum"), "/liste");
    page.eval("add('İki')").unwrap();
    until("the first change", 20, || {
        events(&root, "sayfa.changed").len() == 1
    });
    // three in a burst: one event, after a second of quiet
    page.eval("add('Üç'); add('Dört'); add('Beş'); 1").unwrap();
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
    let mut page = browser::watched(&d.path().join("durum"), "/haber");
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

#[test]
fn a_step_acts_on_the_watched_window() {
    let d = tempfile::tempdir().unwrap();
    let w = web::serve(vec![web::html("/liste", LISTE)]);
    let root = workspace_with(
        d.path(),
        free_port(),
        "sayfa.changed",
        "#!/bin/sh\nexec itero browser watch \"$URL\" --changed Liste --headless\n",
        json!({ "URL": w.url("/liste") }),
        json!([{ "#browser": { "args": { "read": "text", "in": "Liste", "tab": "$event.tab" } } }]),
    );
    let _daemon = daemon(&root, d.path());
    let mut page = browser::watched(&d.path().join("durum"), "/liste");
    page.eval("add('İki')").unwrap();
    let ws = itero::workspace::find(&root).unwrap();
    let ran = || {
        itero::engine::journal::Journal::open(&ws)
            .unwrap()
            .runs()
            .unwrap()
            .into_iter()
            .filter(|r| r.workflow == "izle" && r.runtime.ended.is_some())
            .collect::<Vec<_>>()
    };
    until("the run", 30, || !ran().is_empty());
    let run = &ran()[0];
    let state = serde_json::to_string(&run.runtime).unwrap();
    assert_eq!(
        itero::engine::steps::status(&run.runtime),
        "done",
        "{state}"
    );
    // the page the connector watches, read where it is
    assert!(state.contains("Bir\\nİki"), "{state}");
    // no window of the run's own was opened
    let active = d
        .path()
        .join("durum/itero/browser/headless/DevToolsActivePort");
    let port = std::fs::read_to_string(active).unwrap();
    let list: Value = client()
        .get(format!(
            "http://127.0.0.1:{}/json/list",
            port.lines().next().unwrap().trim()
        ))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let pages = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["type"] == "page")
        .count();
    // Chrome's first page and the watched one
    assert_eq!(pages, 2, "{list}");
}

#[path = "common/browser.rs"]
mod browser;
#[path = "common/chrome.rs"]
mod test_chrome;

#[test]
fn steps_on_one_watched_window_take_turns() {
    let state = tempfile::tempdir().unwrap();
    // SAFETY: the only test of this binary that runs steps in-process
    unsafe { std::env::set_var("XDG_STATE_HOME", state.path()) };
    let c = test_chrome::start();
    let t = browser::tab(c.port);
    let w = web::serve(vec![web::html("/liste", LISTE)]);
    browser::step(&t, json!({ "open": w.url("/liste") })).unwrap();
    // each waits a second for what never comes
    let slow = json!({ "wait": "Yok", "timeout": 1, "tab": t.ws });
    let started = Instant::now();
    std::thread::scope(|s| {
        let a = s.spawn(|| browser::step(&t, slow.clone()));
        let b = s.spawn(|| browser::step(&t, slow.clone()));
        assert!(a.join().unwrap().is_err());
        assert!(b.join().unwrap().is_err());
    });
    // each gives up just short of its second: at once they would end
    // together, in turn one after the other
    assert!(
        started.elapsed() >= Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn tab_is_a_windows_address() {
    let e = itero::browser::step::check(&json!({ "read": "text", "tab": "http://x" })).unwrap_err();
    assert!(
        e.contains("tab is a window's address (ws://...), as $event.tab gives it"),
        "{e}"
    );
}

#[test]
fn a_page_that_changes_only_when_reloaded_is_reloaded_and_compared() {
    let d = tempfile::tempdir().unwrap();
    // each load counts one more visit: a static page, new on each reload
    let w = web::serve(vec![web::html(
        "/sayaç",
        r#"<!doctype html><title>Sayaç</title><p id="ziyaret"></p>
<script>const n = Number(localStorage.n || 0) + 1; localStorage.n = n;
document.getElementById('ziyaret').textContent = 'Ziyaret ' + n;</script>"#,
    )]);
    let root = workspace(
        d.path(),
        free_port(),
        "sayac.changed",
        "#!/bin/sh\nexec itero browser watch \"$URL\" --changed '{\"css\": \"#ziyaret\"}' --navigated --reload 2 --headless\n",
        json!({ "URL": w.url("/sayaç") }),
    );
    let _daemon = daemon(&root, d.path());
    until("two reloads seen", 30, || {
        events(&root, "sayac.changed").len() >= 2
    });
    let got = events(&root, "sayac.changed");
    // its own reloads are not news: changes only
    assert!(got.iter().all(|e| e["what"] == "changed"), "{got:?}");
    assert_eq!(got[0]["previous"], "Ziyaret 1", "{got:?}");
    assert_eq!(got[0]["text"], "Ziyaret 2", "{got:?}");
    assert_eq!(got[1]["previous"], "Ziyaret 2", "{got:?}");
    assert_eq!(got[1]["text"], "Ziyaret 3", "{got:?}");
}

#[test]
fn reload_is_whole_seconds() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args([
            "browser",
            "watch",
            "https://ışık.example/",
            "--navigated",
            "--reload",
            "0",
        ])
        .env("ITERO_BROWSER_API", "http://127.0.0.1:9/browser")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("--reload is whole seconds, at least 1"), "{e}");
}
