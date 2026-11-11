//! `itero daemon` with `daemon.json`'s `notify`: a workspace's `#notify`
//! goes to the channel, not the desktop. Every command runs with HOME,
//! XDG_CONFIG_HOME and XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
#[path = "common/web.rs"]
mod web;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn command(d: &Path, dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
    c.current_dir(dir)
        .env("HOME", d.join("ev"))
        .env("XDG_CONFIG_HOME", d.join("ayar"))
        .env("XDG_STATE_HOME", d.join("durum"))
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("ITERO_NOTIFY_LOG", d.join("gösterilen.log"))
        .stdin(Stdio::null());
    c
}

#[test]
fn a_workspaces_notify_goes_to_the_machines_channel() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    let w = web::serve(vec![web::file("/kanca", "text/plain", "tamam")]);
    // a workspace whose `bildir`, on `bildirim.istendi`, notifies
    let ws = d.join("projeler/bir");
    std::fs::create_dir_all(ws.join("workflows")).unwrap();
    std::fs::write(ws.join("itero.config.json"), "{}").unwrap();
    std::fs::write(
        ws.join("workflows/bildir.json"),
        json!({ "on": [{ "event": "bildirim.istendi" }],
                "steps": [{ "#notify": { "args": { "title": "Sunucu", "text": "disk dolu" } } }] })
        .to_string(),
    )
    .unwrap();
    let o = command(d, &ws).args(["ws", "add"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let config = d.join("ayar/itero/daemon.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        json!({ "notify": [{ "url": w.url("/kanca") }] }).to_string(),
    )
    .unwrap();
    let _d = Daemon(
        command(d, d)
            .arg("daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    // the workspace running, then the event
    let kept = d.join("durum/itero/port");
    let http = itero::net::client(Duration::from_secs(2)).unwrap();
    until("the workspace to run", || {
        std::fs::read_to_string(&kept)
            .ok()
            .and_then(|p| p.trim().parse::<u16>().ok())
            .and_then(|port| {
                http.get(format!("http://127.0.0.1:{port}/workspaces"))
                    .send()
                    .ok()?
                    .json::<Value>()
                    .ok()
            })
            .is_some_and(|v| {
                v.as_array()
                    .is_some_and(|l| l.iter().any(|w| w["state"] == "running"))
            })
    });
    let o = command(d, &ws)
        .args(["ws", "emit", "bildirim.istendi"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    until("the channel to get it", || {
        !w.bodies.lock().unwrap().is_empty()
    });
    let (path, body) = w.bodies.lock().unwrap()[0].clone();
    assert_eq!(path, "/kanca");
    let b: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(b["title"], "Sunucu");
    assert_eq!(b["text"], "disk dolu");
    assert_eq!(b["workflow"], "bildir");
    assert!(b["workspace"].is_string(), "its name on the list: {b}");
    assert!(
        !d.join("gösterilen.log").exists(),
        "nothing went to the desktop"
    );
}
