//! The built-in `#browser`: one action on the run's tab in the daemon's
//! Chrome. Its `$output` is always the page (`url`, `title`) and the mode,
//! and what the action found or read.
use super::blockers::{self, Blocker};
use super::cdp::Cdp;
use super::target::{self, Target};
use super::{inspect, network, page};
use crate::engine::worker::{Failure, Note, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

/// The args `#browser` reads.
pub const ARGS: [&str; 18] = [
    "open",
    "wait",
    "find",
    "read",
    "in",
    "click",
    "hover",
    "type",
    "into",
    "key",
    "inspect",
    "screenshot",
    "full",
    "scroll",
    "headless",
    "timeout",
    "responses",
    "cookie_values",
];

/// The actions that load something, with which `responses` goes.
const LOADS: [&str; 5] = ["open", "wait", "click", "key", "inspect"];

/// The actions: one per step.
pub const ACTIONS: [&str; 11] = [
    "open",
    "wait",
    "find",
    "read",
    "click",
    "hover",
    "type",
    "key",
    "inspect",
    "screenshot",
    "scroll",
];

/// How long an action waits for a page or a target with no `timeout`.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How often a target not there yet is looked for again.
const AGAIN: Duration = Duration::from_millis(200);

fn later(v: &Value) -> bool {
    // "$output.url" and the like: known only in a run
    v.as_str().is_some_and(|s| s.starts_with('$'))
}

/// Every problem with a step's args, in one message; an args value known
/// only in a run is not judged.
pub fn check(args: &Value) -> Result<(), String> {
    let Some(a) = args.as_object() else {
        return Ok(());
    };
    let mut problems = vec![];
    let actions: Vec<&str> = ACTIONS
        .iter()
        .copied()
        .filter(|k| a.contains_key(*k))
        .collect();
    match actions.as_slice() {
        [] => problems.push(format!("a step does one of {}", or_list(&ACTIONS))),
        [_] => {}
        many => problems.push(format!("one action per step, not {}", many.join(" and "))),
    }
    let is = |k: &str| a.get(k).filter(|v| !later(v));
    for k in ["open", "inspect"] {
        if let Some(v) = is(k) {
            match v.as_str() {
                Some(u) if u.starts_with("http://") || u.starts_with("https://") => {}
                _ => problems.push(format!("{k} is an http or https address, not {v}")),
            }
        }
    }
    if a.contains_key("cookie_values") && !a.contains_key("inspect") {
        problems.push("cookie_values goes with inspect".into());
    }
    if let Some(v) = is("cookie_values")
        && !v.is_boolean()
    {
        problems.push(format!("cookie_values is true or false, not {v}"));
    }
    for k in ["wait", "find", "click", "hover", "in", "into", "scroll"] {
        if let Some(v) = is(k)
            && let Err(e) = Target::parse(v)
        {
            problems.push(format!("{k}: {e}"));
        }
    }
    if let Some(v) = is("read")
        && v != "text"
        && v != "html"
    {
        problems.push(format!("read is \"text\" or \"html\", not {v}"));
    }
    if a.contains_key("in") && !a.contains_key("read") && !a.contains_key("screenshot") {
        problems.push("in goes with read or screenshot".into());
    }
    if let Some(v) = is("screenshot") {
        let ok = v.as_str().is_some_and(|f| {
            let f = f.to_ascii_lowercase();
            [".png", ".jpg", ".jpeg"].iter().any(|e| f.ends_with(e))
        });
        if !ok {
            problems.push(format!("screenshot is a .png, .jpg or .jpeg file, not {v}"));
        }
    }
    if a.contains_key("full") && !a.contains_key("screenshot") {
        problems.push("full goes with screenshot".into());
    }
    if let Some(v) = is("full")
        && !v.is_boolean()
    {
        problems.push(format!("full is true or false, not {v}"));
    }
    if a.contains_key("full") && a.contains_key("in") {
        problems.push("full and in do not go together".into());
    }
    match (a.contains_key("type"), a.contains_key("into")) {
        (true, false) => problems.push("type needs into: the field to type in".into()),
        (false, true) => problems.push("into goes with type".into()),
        _ => {}
    }
    if let Some(v) = is("type")
        && !v.is_string()
    {
        problems.push(format!("type is a text, not {v}"));
    }
    if let Some(v) = is("key") {
        let names: Vec<&str> = page::KEYS.iter().map(|k| k.0).collect();
        if !v.as_str().is_some_and(|k| names.contains(&k)) {
            problems.push(format!("key is one of {}, not {v}", names.join(", ")));
        }
    }
    if a.contains_key("responses") && !LOADS.iter().any(|k| a.contains_key(*k)) {
        problems.push(format!("responses goes with {}", or_list(&LOADS)));
    }
    if let Some(v) = is("responses")
        && !v.as_str().is_some_and(|t| !t.is_empty())
    {
        problems.push(format!("responses is a text a URL contains, not {v}"));
    }
    if let Some(v) = is("headless")
        && !v.is_boolean()
    {
        problems.push(format!("headless is true or false, not {v}"));
    }
    if let Some(v) = is("timeout")
        && !v.as_f64().is_some_and(|s| s > 0.0 && s <= 3600.0)
    {
        problems.push(format!(
            "timeout is seconds, more than 0 and at most 3600, not {v}"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// `a, b, c or d`.
fn or_list(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

fn fail(why: impl std::fmt::Display) -> Failure {
    Failure::Error(format!("#browser: {why}"))
}

/// Runs the step; what it says while it waits for a person goes to `told`,
/// its progress.
pub fn run(r: &StepRun, told: &(dyn Fn(Option<&str>) + Sync)) -> Result<Outcome, Failure> {
    let args = r.input.get("args").cloned().unwrap_or(json!({}));
    check(&args).map_err(fail)?;
    let a = args.as_object().cloned().unwrap_or_default();
    let tab = r
        .env
        .get("ITERO_BROWSER_TAB")
        .ok_or_else(|| fail("no tab: the daemon gives one to the step"))?;
    let timeout = a
        .get("timeout")
        .and_then(Value::as_f64)
        .map(Duration::from_secs_f64)
        .unwrap_or(TIMEOUT);
    let until = Instant::now() + timeout;
    let mut cdp = Cdp::connect(tab, TIMEOUT).map_err(fail)?;
    page::prepare(&mut cdp).map_err(fail)?;
    let wanted = a.get("responses").and_then(Value::as_str);
    // inspect turns the Network events on itself, before its page loads
    if wanted.is_some() && !a.contains_key("inspect") {
        network::start(&mut cdp).map_err(fail)?;
    }
    let mut out = Map::new();
    let acted = act(
        &mut cdp,
        &a,
        Wait { until, timeout },
        tab,
        r,
        told,
        &mut out,
    );
    if let Err(why) = &acted {
        return Err(fail(failed(&mut cdp, why.clone(), r)));
    }
    if let Some(text) = wanted {
        // what this step's page loaded; a tab a click opened is not
        // recorded
        let got = network::responses(&mut cdp, text, until).map_err(fail)?;
        out.insert("responses".into(), json!(got));
    }
    let mut cdp = match acted {
        Ok(Some(new)) => new,
        _ => cdp,
    };
    let (url, title) = page::about(&mut cdp).map_err(fail)?;
    out.insert("url".into(), json!(url));
    out.insert("title".into(), json!(title));
    if let Some(m) = r.env.get("ITERO_BROWSER_MODE") {
        out.insert("mode".into(), json!(m));
    }
    Ok(Outcome::Done(Value::Object(out)))
}

/// Why a step failed, with the page it was on and a screenshot of it.
fn failed(cdp: &mut Cdp, why: String, r: &StepRun) -> String {
    let mut msg = why;
    if let Ok((url, title)) = page::about(cdp) {
        msg.push_str(&format!(", on \"{title}\" ({url})"));
    }
    let name = format!(
        "{}-{}.png",
        r.env.get("ITERO_BROWSER_RUN").map_or("run", String::as_str),
        r.env
            .get("ITERO_BROWSER_STEP")
            .map_or("step", String::as_str)
    );
    let shown = std::path::Path::new(".itero").join("browser").join(name);
    match page::screenshot(cdp, &r.root.join(&shown), page::Shot::View) {
        Ok(()) => msg.push_str(&format!("; screenshot: {}", shown.display())),
        Err(e) => msg.push_str(&format!("; no screenshot: {e}")),
    }
    msg
}

/// How long an action waits: until `until`, its `timeout` from when it
/// started.
#[derive(Clone, Copy)]
struct Wait {
    until: Instant,
    timeout: Duration,
}

/// After a page loads: a banner rejected is logged; one that is not is
/// shown to the user, and `$output.blocked` says it may be in the way. A
/// check sitting in a form is `$output.captcha`. A human check in the way
/// fails a headless step at once; in a window, its page is brought to the
/// front, the user is told, and the step waits for a person to solve it,
/// then looks again.
fn blocked(
    cdp: &mut Cdp,
    r: &StepRun,
    w: Wait,
    told: &(dyn Fn(Option<&str>) + Sync),
    out: &mut Map<String, Value>,
) -> Result<(), String> {
    let mut kinds = vec![];
    for b in blockers::check(cdp, w.until)? {
        match b {
            Blocker::Captcha {
                kind,
                in_the_way: false,
            } => {
                out.insert("captcha".into(), json!(kind));
            }
            Blocker::Captcha { kind, .. } => {
                let (url, _) = page::about(cdp)?;
                let host = host_of(&url);
                if r.env.get("ITERO_BROWSER_MODE").map(String::as_str) != Some("window") {
                    tell(
                        r,
                        format!(
                            "{host} shows a human check ({kind}), which cannot be solved headless"
                        ),
                        &url,
                    )?;
                    return Err(format!(
                        "{host} shows a human check ({kind}): it cannot be solved headless; \
                         give the run's first #browser step headless: false"
                    ));
                }
                cdp.send("Page.bringToFront", json!({}))?;
                tell(
                    r,
                    format!(
                        "{host} shows a human check ({kind}): solve it in the Chrome window, and the run goes on"
                    ),
                    &url,
                )?;
                solved(cdp, &host, r, told)?;
                // what the page shows once it is solved, as after a load
                let again = Wait {
                    until: Instant::now() + w.timeout,
                    ..w
                };
                page::ready(cdp, again.until)?;
                return blocked(cdp, r, again, told, out);
            }
            Blocker::Consent {
                tool,
                cleared: true,
            } => {
                let (url, _) = page::about(cdp)?;
                if let Some(log) = crate::engine::log::Log::of_root(&r.root) {
                    log.write(
                        "browser.consent",
                        json!({ "workflow": r.workflow, "tool": tool, "url": url }),
                    );
                }
            }
            Blocker::Consent { tool, .. } => {
                let (url, _) = page::about(cdp)?;
                let host = host_of(&url);
                let banner = match tool {
                    Some(t) => format!("a {t} cookie banner"),
                    None => "a cookie banner".into(),
                };
                tell(
                    r,
                    format!("{host} shows {banner} itero cannot close: it may be in the way"),
                    &url,
                )?;
                kinds.push("consent");
            }
        }
    }
    if !kinds.is_empty() {
        out.insert("blocked".into(), json!(kinds));
    }
    Ok(())
}

/// The host of `url`, with its port: what the user knows a site by.
fn host_of(url: &str) -> String {
    url.split('/').nth(2).unwrap_or(url).to_string()
}

/// How often the user is told the step still waits for them.
const STILL: Duration = Duration::from_secs(60);

/// Waits for the human check on the page to go, as long as the step may
/// run (the workspace's `limits.most`), saying so in its progress each
/// minute. A page going elsewhere, as when the check is passed, cannot be
/// asked for a moment; one that cannot be asked for a minute has failed.
fn solved(
    cdp: &mut Cdp,
    host: &str,
    r: &StepRun,
    told: &(dyn Fn(Option<&str>) + Sync),
) -> Result<(), String> {
    let started = Instant::now();
    let end = started + Duration::from_secs(r.most);
    let mut said: Option<Instant> = None;
    let mut asked = Instant::now();
    loop {
        if said.is_none_or(|t| t.elapsed() >= STILL) {
            let minutes = started.elapsed().as_secs() / 60;
            told(Some(&format!(
                "waiting for a human check on {host} ({minutes} min)"
            )));
            said = Some(Instant::now());
        }
        match blockers::human_check(cdp) {
            Ok(None) => return Ok(()),
            Ok(Some(_)) => asked = Instant::now(),
            Err(e) if asked.elapsed() > STILL => return Err(e),
            Err(_) => {}
        }
        if Instant::now() > end {
            return Err(format!("nobody solved the human check on {host}"));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// A notification about the page at `url`, from the step's workflow.
fn tell(r: &StepRun, text: String, url: &str) -> Result<(), String> {
    crate::engine::worker::show(&Note {
        title: "itero: #browser".into(),
        subtitle: Some(r.workflow.clone()),
        text,
        url: Some(url.into()).filter(|u: &String| crate::engine::worker::openable(u)),
        ..Note::default()
    })
}

/// The target, waiting for it until `until`.
fn wait_for(
    cdp: &mut Cdp,
    t: &Target,
    until: Instant,
    timeout: Duration,
) -> Result<target::Found, String> {
    loop {
        if let Some(f) = target::find(cdp, t)? {
            return Ok(f);
        }
        if Instant::now() + AGAIN > until {
            return Err(format!(
                "{} is not on the page after {} s",
                t.describe(),
                timeout.as_secs_f64().round()
            ));
        }
        std::thread::sleep(AGAIN);
    }
}

fn target_of(a: &Map<String, Value>, k: &str) -> Result<Target, String> {
    Target::parse(a.get(k).unwrap_or(&Value::Null))
}

/// Does the one action; a new connection when a click went on in a tab
/// it opened.
fn act(
    cdp: &mut Cdp,
    a: &Map<String, Value>,
    w: Wait,
    tab: &str,
    r: &StepRun,
    told: &(dyn Fn(Option<&str>) + Sync),
    out: &mut Map<String, Value>,
) -> Result<Option<Cdp>, String> {
    let Wait { until, timeout } = w;
    if let Some(url) = a.get("open").and_then(Value::as_str) {
        page::load(cdp, url, until)?;
        blocked(cdp, r, w, told, out)?;
    } else if let Some(url) = a.get("inspect").and_then(Value::as_str) {
        inspect::load(cdp, url, until)?;
        blocked(cdp, r, w, told, out)?;
        let values = a.get("cookie_values") == Some(&json!(true));
        out.insert("inspect".into(), inspect::facts(cdp, values)?);
    } else if a.contains_key("wait") {
        wait_for(cdp, &target_of(a, "wait")?, until, timeout)?;
        out.insert("found".into(), json!(true));
    } else if a.contains_key("find") {
        let f = target::find(cdp, &target_of(a, "find")?)?;
        out.insert("found".into(), json!(f.is_some()));
        out.insert("count".into(), json!(f.as_ref().map_or(0, |f| f.count)));
        if let Some(f) = f {
            out.insert("text".into(), json!(f.text));
        }
    } else if let Some(how) = a.get("read").and_then(Value::as_str) {
        let part = match a.get("in") {
            Some(_) => Some(wait_for(cdp, &target_of(a, "in")?, until, timeout)?.backend),
            None => None,
        };
        if how == "html" {
            let html = match part {
                Some(b) => {
                    cdp.send("DOM.getOuterHTML", json!({ "backendNodeId": b }))?["outerHTML"]
                        .clone()
                }
                None => cdp.eval("document.documentElement.outerHTML")?,
            };
            out.insert("html".into(), html);
        } else {
            let tree = cdp.send("Accessibility.getFullAXTree", json!({}))?;
            let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
            let text = match part {
                Some(b) => super::text::from_subtree(&nodes, b)
                    .ok_or("that part of the page has nothing to read")?,
                None => super::text::from_tree(&nodes),
            };
            out.insert("text".into(), json!(text));
        }
    } else if a.contains_key("click") {
        let f = wait_for(cdp, &target_of(a, "click")?, until, timeout)?;
        let (x, y) = target::point(cdp, f.backend).map_err(|e| {
            format!(
                "cannot click {}: {e}",
                target_of(a, "click").expect("checked").describe()
            )
        })?;
        let before = page::tabs(tab)?;
        page::click(cdp, x, y)?;
        if let Some(new) = page::opened(tab, &before)? {
            // the run goes on in the tab the click opened
            let run = r.env.get("ITERO_BROWSER_RUN").ok_or("no run")?;
            let file = super::keeper::followed_file(run).map_err(|e| format!("{e:#}"))?;
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&file, json!({ "id": new.id, "ws": new.ws }).to_string())
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let mut next = Cdp::connect(&new.ws, TIMEOUT)?;
            page::prepare(&mut next)?;
            // what it opened may still be loading
            page::ready(&mut next, until)?;
            out.insert("new_tab".into(), json!(true));
            blocked(&mut next, r, w, told, out)?;
            return Ok(Some(next));
        }
        page::follow(cdp, until)?;
        blocked(cdp, r, w, told, out)?;
    } else if let Some(file) = a.get("screenshot").and_then(Value::as_str) {
        let shot = if a.get("full") == Some(&json!(true)) {
            page::Shot::Full
        } else if a.contains_key("in") {
            page::Shot::Element(wait_for(cdp, &target_of(a, "in")?, until, timeout)?.backend)
        } else {
            page::Shot::View
        };
        // the workspace's, or absolute
        let path = r.root.join(file);
        page::screenshot(cdp, &path, shot)?;
        out.insert("file".into(), json!(path.display().to_string()));
    } else if a.contains_key("scroll") {
        let t = target_of(a, "scroll")?;
        let f = wait_for(cdp, &t, until, timeout)?;
        target::scroll_to(cdp, f.backend)
            .map_err(|e| format!("cannot scroll to {}: {e}", t.describe()))?;
    } else if a.contains_key("hover") {
        let t = target_of(a, "hover")?;
        let f = wait_for(cdp, &t, until, timeout)?;
        let (x, y) = target::point(cdp, f.backend)
            .map_err(|e| format!("cannot hover {}: {e}", t.describe()))?;
        page::hover(cdp, x, y)?;
    } else if let Some(text) = a.get("type").and_then(Value::as_str) {
        let t = target_of(a, "into")?;
        let f = wait_for(cdp, &t, until, timeout)?;
        let (x, y) = target::point(cdp, f.backend)
            .map_err(|e| format!("cannot type into {}: {e}", t.describe()))?;
        page::click(cdp, x, y)?;
        page::type_text(cdp, text)?;
    } else if let Some(k) = a.get("key").and_then(Value::as_str) {
        page::key(cdp, k)?;
        page::follow(cdp, until)?;
    }
    Ok(None)
}
