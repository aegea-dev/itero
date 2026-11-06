//! #browser steps in a workflow the daemon runs: one tab for the run,
//! closed when it ends; a failed step's screenshot forgotten with its run.
//! Its own test binary: it sets XDG_STATE_HOME for the process.
#![cfg(unix)]
#[path = "common/web.rs"]
mod web;
use itero::browser::Mode;
use itero::browser::keeper::Keeper;
use itero::engine::journal::Journal;
use serde_json::json;
use std::path::Path;
use std::time::{Duration, Instant};

const NOW: &str = "2026-11-07T10:00:00Z";

#[test]
fn a_run_s_steps_share_its_tab_which_closes_when_the_run_ends() {
    let d = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe { std::env::set_var("XDG_STATE_HOME", d.path().join("durum")) };
    let _stop = StopChrome;
    let w = web::serve(vec![web::html(
        "/sepet",
        "<!doctype html><title>Sepet</title><button>Öde</button>",
    )]);
    let root = d.path().join("çalışma");
    std::fs::create_dir_all(root.join("workflows")).unwrap();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    std::fs::write(
        root.join("workflows/sepet.json"),
        json!({
            "on": [{ "event": "sepet.açıldı" }],
            "steps": [
                { "#browser": { "args": { "open": "$event.url", "headless": true } } },
                { "#browser": { "args": { "find": "Öde" } } },
                { "#browser": { "args": { "click": "Yok", "timeout": 1 } } }
            ]
        })
        .to_string(),
    )
    .unwrap();
    let ws = itero::workspace::find(&root).unwrap();
    itero::engine::events::emit(
        &ws,
        "sepet.açıldı",
        Some(json!({ "url": w.url("/sepet") })),
        NOW,
    )
    .unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(
        itero::engine::steps::status(&run.runtime),
        "error",
        "{:?}",
        run.runtime
    );
    // the second step found what the first opened: one tab
    let state = serde_json::to_string(&run.runtime.state).unwrap();
    assert!(state.contains("\"count\":1"), "{state}");
    let why = serde_json::to_string(&run.runtime).unwrap();
    assert!(
        why.contains("\\\"Yok\\\" is not on the page after 1 s"),
        "{why}"
    );
    let shot = root.join(format!(".itero/browser/{}-0.2.png", run.id));
    assert!(shot.is_file(), "{why}");

    // the run ended: its tab is closed
    let port = Keeper::global().port(Mode::Headless).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let list: serde_json::Value = itero::net::client(Duration::from_secs(5))
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/json/list"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        if !list.to_string().contains("/sepet") {
            break;
        }
        assert!(Instant::now() < end, "{list}");
        std::thread::sleep(Duration::from_millis(100));
    }

    // a run forgotten, its screenshot goes with it
    let other = root.join(".itero/browser/0199aaaa-0000-7000-8000-000000000000-1.png");
    std::fs::write(&other, b"x").unwrap();
    let forget = itero::engine::worker::StepRun {
        file: itero::engine::worker::FORGET.into(),
        root: root.clone(),
        workflow: "keep".into(),
        env: Default::default(),
        input: Default::default(),
        memory: root.join(".itero/memory/keep"),
        waiting_since: None,
        quiet: 60,
        most: 60,
        now: NOW.into(),
    };
    itero::engine::worker::run_step(&forget).unwrap();
    assert!(!other.exists());
    assert!(shot.is_file(), "a run still kept keeps its screenshot");
}

/// Stops the keeper's Chrome however the test ends: a panic would leave
/// it running, with no parent.
struct StopChrome;

impl Drop for StopChrome {
    fn drop(&mut self) {
        Keeper::global().stop();
    }
}
