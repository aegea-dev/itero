//! `itero browser watch`: a page kept open in a window the daemon gives a
//! connector, and a line printed for each thing that happens on it, one
//! event each. Changes are seen by a `MutationObserver` in an isolated
//! world of the page, where its own scripts do not see it, told through a
//! binding only that world has.
use super::blockers::{self, Blocker};
use super::cdp::{Cdp, Tab};
use super::target::Target;
use super::{network, page};
use serde_json::{Map, Value, json};
use std::io::Write;
use std::time::{Duration, Instant};

/// What to watch.
pub struct Options {
    pub url: String,
    pub changed: Vec<String>,
    pub appeared: Vec<String>,
    pub navigated: bool,
    pub responses: Vec<String>,
    /// Seconds between reloads, for a page that changes only when it is
    /// loaded again.
    pub reload: Option<u64>,
    pub headless: bool,
}

/// The isolated world itero watches from, and its binding.
const WORLD: &str = "itero";
const BINDING: &str = "__itero";

/// The quiet a change waits for before it is told: a burst is one event.
const QUIET_MS: u64 = 1000;

/// How long the page is given to load.
const LOAD: Duration = Duration::from_secs(60);

/// A target given on the command line: a name, or `{"css": "..."}`.
fn target(s: &str) -> Result<Value, String> {
    let v = if s.trim_start().starts_with('{') {
        serde_json::from_str(s).map_err(|e| format!("{s}: {e}"))?
    } else {
        json!(s)
    };
    Target::parse(&v).map_err(|e| format!("{s}: {e}"))?;
    Ok(v)
}

/// Every problem with the options, in one message.
fn check(o: &Options) -> Result<(Vec<Value>, Vec<Value>), String> {
    let mut problems = vec![];
    if !(o.url.starts_with("http://") || o.url.starts_with("https://")) {
        problems.push(format!(
            "the page is an http or https address, not {}",
            o.url
        ));
    }
    if o.changed.is_empty() && o.appeared.is_empty() && !o.navigated && o.responses.is_empty() {
        problems.push("watch for one of --changed, --appeared, --navigated or --responses".into());
    }
    if o.reload == Some(0) {
        problems.push("--reload is whole seconds, at least 1".into());
    }
    let mut parse = |all: &[String]| -> Vec<Value> {
        all.iter()
            .filter_map(|t| target(t).map_err(|e| problems.push(e)).ok())
            .collect()
    };
    let (changed, appeared) = (parse(&o.changed), parse(&o.appeared));
    if problems.is_empty() {
        Ok((changed, appeared))
    } else {
        Err(problems.join("; "))
    }
}

/// The script put in every document of the tab, in itero's world: it
/// finds each target as a step does (a name: the element labelled or
/// titled so, else one showing that text; or a CSS selector), and reports
/// what each shows (its text, or null when it is not there) once the page
/// is ready and after each `QUIET_MS` of quiet. What changed is worked out
/// by `Seen`, so it holds across a reload, which starts a new document.
fn script(changed: &[Value], appeared: &[Value]) -> String {
    format!(
        r#"(() => {{
  const changed = {changed}, appeared = {appeared};
  const byName = name => {{
    for (const e of document.querySelectorAll('[aria-label],[title]'))
      if (e.getAttribute('aria-label') === name || e.getAttribute('title') === name) return e;
    for (const e of document.querySelectorAll('a,button,h1,h2,h3,h4,h5,h6,label,legend,th,[role]'))
      if ((e.innerText || '').trim() === name) return e;
    const w = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT);
    while (w.nextNode()) if (w.currentNode.nodeValue.trim() === name) return w.currentNode.parentElement;
    return null;
  }};
  const find = t => typeof t === 'string' ? byName(t) : document.querySelector(t.css);
  const text = e => e ? (e.innerText ?? e.textContent ?? '').trim() : null;
  const look = () => {BINDING}(JSON.stringify({{
    changed: changed.map(t => text(find(t))),
    appeared: appeared.map(t => text(find(t))),
  }}));
  let timer;
  const start = () => {{
    look();
    new MutationObserver(() => {{ clearTimeout(timer); timer = setTimeout(look, {QUIET_MS}); }})
      .observe(document, {{ subtree: true, childList: true, characterData: true, attributes: true }});
  }};
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', start); else start();
}})();"#,
        changed = json!(changed),
        appeared = json!(appeared),
    )
}

/// What the targets showed last, kept across documents: the lines a new
/// report makes.
struct Seen {
    changed: Vec<(Value, Option<String>)>,
    appeared: Vec<(Value, Option<bool>)>,
}

impl Seen {
    fn new(changed: &[Value], appeared: &[Value]) -> Seen {
        Seen {
            changed: changed.iter().map(|t| (t.clone(), None)).collect(),
            appeared: appeared.iter().map(|t| (t.clone(), None)).collect(),
        }
    }

    /// The lines for `report` (`{changed: [text|null], appeared:
    /// [text|null]}`): a text other than the last one known; a target there
    /// that was known not to be. A target not there keeps its last text;
    /// the first report is what there is to compare with.
    fn lines(&mut self, report: &Value) -> Vec<Map<String, Value>> {
        let mut out = vec![];
        for (i, (target, last)) in self.changed.iter_mut().enumerate() {
            let Some(now) = report["changed"][i].as_str() else {
                continue;
            };
            if let Some(before) = last.as_deref()
                && before != now
            {
                out.push(Map::from_iter([
                    ("what".into(), json!("changed")),
                    ("target".into(), target.clone()),
                    ("text".into(), json!(now)),
                    ("previous".into(), json!(before)),
                ]));
            }
            *last = Some(now.to_string());
        }
        for (i, (target, was)) in self.appeared.iter_mut().enumerate() {
            let now = &report["appeared"][i];
            let here = !now.is_null();
            if here && *was == Some(false) {
                out.push(Map::from_iter([
                    ("what".into(), json!("appeared")),
                    ("target".into(), target.clone()),
                    ("text".into(), now.clone()),
                ]));
            }
            *was = Some(here);
        }
        out
    }
}

/// The daemon's browser API, as a connector is given it.
fn api() -> Result<String, String> {
    std::env::var("ITERO_BROWSER_API").map_err(|_| {
        "itero browser watch runs as a connector of a daemon that serves its API \
         (ITERO_BROWSER_API is not set)"
            .to_string()
    })
}

fn http() -> Result<reqwest::blocking::Client, String> {
    crate::net::client(Duration::from_secs(60)).map_err(|e| e.to_string())
}

/// A window of the daemon's Chrome, and the response that keeps it: the
/// window lives as long as the response is held, closed by the daemon
/// when its connection closes, however this process ends.
fn open(api: &str, headless: bool) -> Result<(Tab, impl Sized), String> {
    use std::io::BufRead;
    let body = if headless {
        json!({ "headless": true })
    } else {
        json!({})
    };
    let r = http()?
        .post(format!("{api}/tabs"))
        .json(&body)
        .send()
        .map_err(|e| format!("the daemon at {api}: {e}"))?;
    if !r.status().is_success() {
        let v: Value = r.json().map_err(|e| format!("the daemon said: {e}"))?;
        return Err(format!(
            "the daemon: {}",
            v["error"].as_str().unwrap_or("failed")
        ));
    }
    // its first line; the rest never comes, and is not read
    let mut held = std::io::BufReader::new(r);
    let mut line = String::new();
    held.read_line(&mut line)
        .map_err(|e| format!("the daemon said: {e}"))?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("the daemon said: {e}"))?;
    let tab = Tab {
        id: v["id"].as_str().ok_or("the daemon gave no window")?.into(),
        ws: v["ws"].as_str().ok_or("the daemon gave no window")?.into(),
    };
    Ok((tab, held))
}

/// A line for the connector's event: what happened, and the page.
fn print(cdp: &mut Cdp, tab: &Tab, mut line: Map<String, Value>) -> Result<(), String> {
    let (url, title) = page::about(cdp)?;
    line.insert("url".into(), json!(url));
    line.insert("title".into(), json!(title));
    line.insert("tab".into(), json!(tab.ws));
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", Value::Object(line))
        .and_then(|_| out.flush())
        .map_err(|e| format!("cannot print: {e}"))
}

/// After the page loads: a consent banner rejected, or shown to the user;
/// a human check shown and waited for, nothing printed meanwhile.
fn blocked(cdp: &mut Cdp, from: &str) -> Result<(), String> {
    for b in blockers::check(cdp, Instant::now() + LOAD)? {
        let (url, _) = page::about(cdp)?;
        let host = blockers::host_of(&url);
        match b {
            Blocker::Consent { cleared: true, .. } => {}
            Blocker::Consent { tool, .. } => {
                let banner =
                    tool.map_or("a cookie banner".into(), |t| format!("a {t} cookie banner"));
                blockers::notify(
                    from,
                    format!("{host} shows {banner} itero cannot close: it may be in the way"),
                    &url,
                )?;
            }
            Blocker::Captcha {
                in_the_way: false, ..
            } => {}
            Blocker::Captcha { kind, .. } => {
                let _ = cdp.send("Page.bringToFront", json!({}));
                blockers::notify(
                    from,
                    format!(
                        "{host} shows a human check ({kind}): solve it in the Chrome window, and watching goes on"
                    ),
                    &url,
                )?;
                let forever = Instant::now() + Duration::from_secs(365 * 24 * 3600);
                blockers::solved(cdp, &host, forever, &|said| {
                    if let Some(s) = said {
                        eprintln!("itero: {s}");
                    }
                })?;
                page::ready(cdp, Instant::now() + LOAD)?;
                return blocked(cdp, from);
            }
        }
    }
    Ok(())
}

/// Watches until the window closes (an error) or the process ends; the
/// window goes with the process, however it ends. `opened` is called once
/// the window is there.
pub fn run(o: Options, opened: impl FnOnce()) -> Result<(), String> {
    let (changed, appeared) = check(&o)?;
    let api = api()?;
    let (tab, _held) = open(&api, o.headless)?;
    opened();
    let mut cdp = Cdp::connect(&tab.ws, Duration::from_secs(30))?;
    page::prepare(&mut cdp)?;
    cdp.send("Runtime.enable", json!({}))?;
    if !changed.is_empty() || !appeared.is_empty() {
        cdp.send(
            "Runtime.addBinding",
            json!({ "name": BINDING, "executionContextName": WORLD }),
        )?;
        cdp.send(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": script(&changed, &appeared), "worldName": WORLD }),
        )?;
    }
    if !o.responses.is_empty() {
        network::start(&mut cdp)?;
    }
    let from = format!("itero browser watch {}", o.url);
    page::load(&mut cdp, &o.url, Instant::now() + LOAD)?;
    blocked(&mut cdp, &from)?;
    let main = cdp.send("Page.getFrameTree", json!({}))?["frameTree"]["frame"]["id"].clone();
    // the page's own load is not news
    cdp.events("Page.");
    let mut loading = false;
    let mut seen = Seen::new(&changed, &appeared);
    let every = o.reload.map(Duration::from_secs);
    let mut next_reload = every.map(|e| Instant::now() + e);
    // a load itero asked for (`--reload`) is not news
    let mut reloading = false;
    // the JSON responses asked for, by request id, until they have loaded
    let mut wanted: Vec<(String, String)> = vec![];
    loop {
        if let Some(at) = next_reload
            && Instant::now() >= at
            && !loading
        {
            cdp.send("Page.reload", json!({}))?;
            reloading = true;
            next_reload = every.map(|e| Instant::now() + e);
        }
        let e = match cdp.wait_event(Instant::now() + Duration::from_secs(1), &mut |_| true) {
            Ok(e) => e,
            Err(why) if why == "timed out" => continue,
            Err(why) => return Err(format!("the watched window: {why}")),
        };
        let p = &e["params"];
        match e["method"].as_str().unwrap_or_default() {
            "Runtime.bindingCalled" if p["name"] == BINDING => {
                let report: Value = serde_json::from_str(p["payload"].as_str().unwrap_or("{}"))
                    .map_err(|e| format!("the page's watcher said: {e}"))?;
                for line in seen.lines(&report) {
                    print(&mut cdp, &tab, line)?;
                }
            }
            "Page.frameNavigated"
                if p["frame"]["id"] == main && p["frame"]["parentId"].is_null() =>
            {
                loading = true;
            }
            "Page.loadEventFired" if loading => {
                loading = false;
                blocked(&mut cdp, &from)?;
                if o.navigated && !std::mem::take(&mut reloading) {
                    print(
                        &mut cdp,
                        &tab,
                        Map::from_iter([("what".into(), json!("navigated"))]),
                    )?;
                }
            }
            "Page.navigatedWithinDocument" if o.navigated && p["frameId"] == main => {
                print(
                    &mut cdp,
                    &tab,
                    Map::from_iter([("what".into(), json!("navigated"))]),
                )?;
            }
            "Network.responseReceived" => {
                let url = p["response"]["url"].as_str().unwrap_or_default();
                let mime = p["response"]["mimeType"].as_str().unwrap_or_default();
                if network::is_json(mime) && o.responses.iter().any(|t| network::matches(url, t)) {
                    wanted.push((
                        p["requestId"].as_str().unwrap_or_default().to_string(),
                        url.to_string(),
                    ));
                }
            }
            "Network.loadingFinished" => {
                let id = p["requestId"].as_str().unwrap_or_default();
                let Some(at) = wanted.iter().position(|w| w.0 == id) else {
                    continue;
                };
                let (_, from) = wanted.remove(at);
                let mut line = Map::new();
                line.insert("what".into(), json!("response"));
                line.insert("response_url".into(), json!(from));
                match network::json_body(&mut cdp, id) {
                    Ok(v) => line.insert("json".into(), v),
                    Err(e) => line.insert("error".into(), json!(e)),
                };
                print(&mut cdp, &tab, line)?;
            }
            "Inspector.detached" | "Target.targetDestroyed" => {
                return Err("the watched window was closed".into());
            }
            _ => {}
        }
    }
}
