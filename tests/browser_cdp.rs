//! The CDP client against a real headless Chrome.
#[path = "common/chrome.rs"]
mod test_chrome;
use itero::browser::cdp::{self, Cdp};
use serde_json::json;
use std::time::{Duration, Instant};

#[test]
fn commands_events_and_errors_on_one_tab() {
    let c = test_chrome::start();
    let tab = cdp::new_window(c.port, "about:blank").unwrap();
    let mut t = Cdp::connect(&tab.ws, Duration::from_secs(10)).unwrap();
    t.send("Page.enable", json!({})).unwrap();
    t.send(
        "Page.navigate",
        json!({ "url": "data:text/html;charset=utf-8,<title>ışık</title><p>Merhaba</p>" }),
    )
    .unwrap();
    let loaded = t
        .wait_event(Instant::now() + Duration::from_secs(10), &mut |e| {
            e["method"] == "Page.loadEventFired"
        })
        .unwrap();
    assert_eq!(loaded["method"], "Page.loadEventFired");
    assert_eq!(t.eval("document.title").unwrap(), json!("ışık"));
    assert_eq!(t.eval("Promise.resolve(40 + 2)").unwrap(), json!(42));
    let e = t.eval("yok.yok").unwrap_err();
    assert!(e.contains("ReferenceError"), "{e}");
    let e = t.send("Yok.yok", json!({})).unwrap_err();
    assert!(e.starts_with("Yok.yok: "), "{e}");
    cdp::close_tab(c.port, &tab.id).unwrap();
}

#[test]
fn waiting_for_an_event_that_never_comes_times_out() {
    let c = test_chrome::start();
    let tab = cdp::new_window(c.port, "about:blank").unwrap();
    let mut t = Cdp::connect(&tab.ws, Duration::from_secs(10)).unwrap();
    let started = Instant::now();
    let e = t
        .wait_event(started + Duration::from_millis(300), &mut |e| {
            e["method"] == "Page.yok"
        })
        .unwrap_err();
    assert_eq!(e, "timed out");
    assert!(started.elapsed() < Duration::from_secs(2));
    // still usable after a timeout
    assert_eq!(t.eval("1 + 1").unwrap(), json!(2));
}
