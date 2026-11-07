//! A tab's page: going to a URL and waiting for it, the mouse and keys
//! as a person uses them, a tab a click opens, a screenshot.
use super::cdp::{Cdp, Tab};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a page may keep loading after it is "loaded" before it is
/// read anyway: some never go quiet.
const QUIET: Duration = Duration::from_secs(2);

/// How long after a click or a key a page is given to start going
/// somewhere else.
const MAYBE_GOES: Duration = Duration::from_millis(700);

/// Turns on the events the page's waits read.
pub fn prepare(cdp: &mut Cdp) -> Result<(), String> {
    cdp.send("Page.enable", json!({}))?;
    cdp.send("Page.setLifecycleEventsEnabled", json!({ "enabled": true }))?;
    cdp.send("DOM.enable", json!({}))?;
    Ok(())
}

/// Goes to `url` and waits until it has loaded, or changed page in place
/// as single-page sites do, then until its network is almost quiet (at
/// most `QUIET`).
pub fn load(cdp: &mut Cdp, url: &str, until: Instant) -> Result<(), String> {
    cdp.events();
    let r = cdp.send("Page.navigate", json!({ "url": url }))?;
    if let Some(why) = r["errorText"].as_str().filter(|w| !w.is_empty()) {
        return Err(format!("{url}: {why}"));
    }
    match r["loaderId"].as_str() {
        Some(loader) if !loader.is_empty() => settle(cdp, loader, until),
        // the same document: no load follows
        _ => Ok(()),
    }
}

fn settle(cdp: &mut Cdp, loader: &str, until: Instant) -> Result<(), String> {
    let lifecycle = |name: &'static str| {
        let loader = loader.to_string();
        move |e: &Value| {
            e["method"] == "Page.lifecycleEvent"
                && e["params"]["name"] == name
                && e["params"]["loaderId"] == loader.as_str()
        }
    };
    let loaded = lifecycle("load");
    cdp.wait_event(until, &mut |e| loaded(e))
        .map_err(|_| "the page did not finish loading".to_string())?;
    let idle = lifecycle("networkAlmostIdle");
    let quiet = (Instant::now() + QUIET).min(until);
    // past it, the page is read as it is
    let _ = cdp.wait_event(quiet, &mut |e| idle(e));
    Ok(())
}

/// After a click or a key: when the page starts going somewhere within
/// `MAYBE_GOES`, waits for it as `load` does.
pub fn follow(cdp: &mut Cdp, until: Instant) -> Result<(), String> {
    let tree = cdp.send("Page.getFrameTree", json!({}))?;
    let main = tree["frameTree"]["frame"]["id"].clone();
    let soon = (Instant::now() + MAYBE_GOES).min(until);
    // the page's own frame, not an ad's or a widget's
    let went = cdp.wait_event(soon, &mut |e| {
        matches!(
            e["method"].as_str(),
            Some(
                "Page.frameStartedNavigating"
                    | "Page.frameStartedLoading"
                    | "Page.navigatedWithinDocument"
            )
        ) && e["params"]["frameId"] == main
    });
    let Ok(e) = went else {
        return Ok(());
    };
    if e["method"] == "Page.navigatedWithinDocument" {
        return Ok(());
    }
    // it started: a new document, whose loader comes with its navigation;
    // or the same one changed in place (pushState), with no load to come
    let nav = cdp
        .wait_event(until, &mut |e| {
            (e["method"] == "Page.frameNavigated" && e["params"]["frame"]["id"] == main)
                || (e["method"] == "Page.navigatedWithinDocument" && e["params"]["frameId"] == main)
        })
        .map_err(|_| "the page did not finish loading".to_string())?;
    if nav["method"] == "Page.navigatedWithinDocument" {
        return Ok(());
    }
    let loader = nav["params"]["frame"]["loaderId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    settle(cdp, &loader, until)
}

fn mouse(cdp: &mut Cdp, kind: &str, x: f64, y: f64) -> Result<(), String> {
    let mut p = json!({ "type": kind, "x": x, "y": y });
    if kind != "mouseMoved" {
        p["button"] = json!("left");
        p["clickCount"] = json!(1);
    }
    cdp.send("Input.dispatchMouseEvent", p).map(|_| ())
}

/// Waits, until `until`, for the page to have loaded: one that is going
/// elsewhere has no document to ask for a moment, and is asked again.
pub fn ready(cdp: &mut Cdp, until: Instant) -> Result<(), String> {
    let loaded = "new Promise(r => document.readyState === 'complete' ? r(true) : addEventListener('load', () => r(true)))";
    loop {
        match cdp.eval(loaded) {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() > until => {
                return Err(format!("the page did not finish loading: {e}"));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// The mouse to (x, y), then a left click: as a person's, not a script's.
/// Events kept from before are dropped: `follow` reads the click's own.
pub fn click(cdp: &mut Cdp, x: f64, y: f64) -> Result<(), String> {
    cdp.events();
    mouse(cdp, "mouseMoved", x, y)?;
    mouse(cdp, "mousePressed", x, y)?;
    mouse(cdp, "mouseReleased", x, y)
}

/// The mouse to (x, y), no click.
pub fn hover(cdp: &mut Cdp, x: f64, y: f64) -> Result<(), String> {
    mouse(cdp, "mouseMoved", x, y)
}

/// Types `text` where the focus is.
pub fn type_text(cdp: &mut Cdp, text: &str) -> Result<(), String> {
    cdp.send("Input.insertText", json!({ "text": text }))
        .map(|_| ())
}

/// The keys `key` takes: name, code, Windows key code, the text it types.
pub const KEYS: [(&str, &str, i64, &str); 12] = [
    ("Enter", "Enter", 13, "\r"),
    ("Escape", "Escape", 27, ""),
    ("Tab", "Tab", 9, ""),
    ("Backspace", "Backspace", 8, ""),
    ("ArrowUp", "ArrowUp", 38, ""),
    ("ArrowDown", "ArrowDown", 40, ""),
    ("ArrowLeft", "ArrowLeft", 37, ""),
    ("ArrowRight", "ArrowRight", 39, ""),
    ("PageUp", "PageUp", 33, ""),
    ("PageDown", "PageDown", 34, ""),
    ("Home", "Home", 36, ""),
    ("End", "End", 35, ""),
];

/// Presses `name`, one of `KEYS`. Events kept from before are dropped,
/// as by `click`.
pub fn key(cdp: &mut Cdp, name: &str) -> Result<(), String> {
    cdp.events();
    let (key, code, vk, text) = KEYS
        .iter()
        .find(|k| k.0 == name)
        .ok_or_else(|| format!("no key {name}"))?;
    let mut down =
        json!({ "type": "keyDown", "key": key, "code": code, "windowsVirtualKeyCode": vk });
    if !text.is_empty() {
        down["text"] = json!(text);
    }
    cdp.send("Input.dispatchKeyEvent", down)?;
    cdp.send(
        "Input.dispatchKeyEvent",
        json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk }),
    )
    .map(|_| ())
}

/// The browser's own connection, beside a tab's, and the tab's id: both
/// from the tab's address (`ws://127.0.0.1:<port>/devtools/page/<id>`).
fn browser(tab_ws: &str) -> Result<(Cdp, String), String> {
    let port = tab_ws
        .strip_prefix("ws://")
        .and_then(|r| r.split('/').next())
        .and_then(|h| h.rsplit(':').next())
        .and_then(|p| p.parse().ok())
        .ok_or("not a tab's address")?;
    let id = tab_ws.rsplit('/').next().unwrap_or_default().to_string();
    Ok((super::cdp::browser(port)?, id))
}

/// The page tabs open now, by id.
pub fn tabs(tab_ws: &str) -> Result<Vec<String>, String> {
    let (mut b, _) = browser(tab_ws)?;
    let r = b.send("Target.getTargets", json!({}))?;
    Ok(r["targetInfos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "page")
        .filter_map(|t| t["targetId"].as_str().map(String::from))
        .collect())
}

/// A tab the page of `tab_ws` opened that was not in `before`, within a
/// moment.
pub fn opened(tab_ws: &str, before: &[String]) -> Result<Option<Tab>, String> {
    let (mut b, me) = browser(tab_ws)?;
    let end = Instant::now() + MAYBE_GOES;
    loop {
        let r = b.send("Target.getTargets", json!({}))?;
        let new = r["targetInfos"].as_array().into_iter().flatten().find(|t| {
            t["type"] == "page"
                && t["openerId"] == me.as_str()
                && !before.iter().any(|id| t["targetId"] == id.as_str())
        });
        if let Some(t) = new {
            let id = t["targetId"].as_str().unwrap_or_default().to_string();
            let host = tab_ws
                .strip_prefix("ws://")
                .and_then(|r| r.split('/').next())
                .unwrap_or_default();
            return Ok(Some(Tab {
                ws: format!("ws://{host}/devtools/page/{id}"),
                id,
            }));
        }
        if Instant::now() > end {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The page as a PNG, to `path`.
pub fn screenshot(cdp: &mut Cdp, path: &Path) -> Result<(), String> {
    use base64::Engine;
    let r = cdp.send("Page.captureScreenshot", json!({ "format": "png" }))?;
    let data = r["data"].as_str().ok_or("no screenshot")?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, png).map_err(|e| format!("{}: {e}", path.display()))
}

/// The page's address and title.
pub fn about(cdp: &mut Cdp) -> Result<(String, String), String> {
    let v = cdp.eval("[location.href, document.title]")?;
    Ok((
        v[0].as_str().unwrap_or_default().to_string(),
        v[1].as_str().unwrap_or_default().to_string(),
    ))
}
