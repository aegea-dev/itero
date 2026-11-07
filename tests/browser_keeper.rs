//! The daemon's keeper of Chrome, headless. Its own test binary: it sets
//! XDG_STATE_HOME (the profiles) and the idle time for the process.
#![cfg(unix)]
use itero::browser::Mode;
use itero::browser::keeper::Keeper;
use itero::engine::journal::Journal;
use itero::workspace::Workspace;
use std::time::{Duration, Instant};

fn kinds(ws: &Workspace) -> Vec<String> {
    Journal::open(ws)
        .unwrap()
        .events(None)
        .unwrap()
        .into_iter()
        .map(|r| r.event.kind)
        .filter(|k| k.starts_with("itero.browser."))
        .collect()
}

fn pages(port: u16) -> Vec<String> {
    let v: serde_json::Value = itero::net::client(Duration::from_secs(5))
        .unwrap()
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .unwrap()
        .json()
        .unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .filter(|t| t["type"] == "page")
        .map(|t| {
            t["webSocketDebuggerUrl"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_tab_per_run_closed_when_the_run_is_gone_and_chrome_stopped_when_idle() {
    let d = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("XDG_STATE_HOME", d.path().join("durum"));
        std::env::set_var("ITERO_TEST_BROWSER_IDLE_SECS", "2");
    }
    let root = d.path().join("çalışma");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(&root).unwrap();
    let k = Keeper::global();
    let _stop = StopChrome;

    let a = k.tab_for_run(&ws, "koşu-1", Some(true)).unwrap();
    assert_eq!(a.mode, Mode::Headless);
    assert_eq!(k.tab_for_run(&ws, "koşu-1", None).unwrap(), a);
    let e = k.tab_for_run(&ws, "koşu-1", Some(false)).unwrap_err();
    assert!(e.contains("headless") && e.contains("window"), "{e}");
    let b = k.tab_for_run(&ws, "koşu-2", Some(true)).unwrap();
    assert_ne!(a.ws, b.ws);
    let port = k.port(Mode::Headless).unwrap();
    assert!(d.path().join("durum/itero/browser/headless").is_dir());
    let open = pages(port);
    assert!(open.contains(&a.ws) && open.contains(&b.ws), "{open:?}");

    // the daemon ends a run: its tab goes, the other stays
    k.end_run("koşu-1");
    // Chrome answers the close before the tab is gone
    until("koşu-1's tab closed", || !pages(port).contains(&a.ws));
    assert!(pages(port).contains(&b.ws));

    // a run the journal does not know (gone, or ended unseen): its tab is
    // closed by the keeper's own look
    until("koşu-2's tab closed", || !pages(port).contains(&b.ws));
    assert_eq!(k.health().unwrap()["tabs"], 0);

    // no tab for the idle time: Chrome is stopped and says so
    until("Chrome stopped", || k.port(Mode::Headless).is_none());
    until("its event saved", || {
        kinds(&ws).contains(&"itero.browser.stopped".to_string())
    });
    assert_eq!(kinds(&ws)[0], "itero.browser.started");
    assert!(
        itero::net::client(Duration::from_secs(2))
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/json/version"))
            .send()
            .is_err()
    );
}

/// Stops the keeper's Chrome however the test ends.
struct StopChrome;

impl Drop for StopChrome {
    fn drop(&mut self) {
        Keeper::global().stop();
    }
}
