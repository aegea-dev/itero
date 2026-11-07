//! The built-in `#browser`: one action on the run's tab in the daemon's
//! Chrome. Its `$output` is always the page (`url`, `title`) and the mode,
//! and what the action found or read.
use super::blockers::{self, Blocker};
use super::cdp::Cdp;
use super::page;
use super::target::{self, Target};
use crate::engine::worker::{Failure, Note, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

/// The args `#browser` reads.
pub const ARGS: [&str; 12] = [
    "open", "wait", "find", "read", "in", "click", "hover", "type", "into", "key", "headless",
    "timeout",
];

/// The actions: one per step.
pub const ACTIONS: [&str; 8] = [
    "open", "wait", "find", "read", "click", "hover", "type", "key",
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
        [] => problems.push(format!(
            "a step does one of {} or key",
            ACTIONS[..ACTIONS.len() - 1].join(", ")
        )),
        [_] => {}
        many => problems.push(format!("one action per step, not {}", many.join(" and "))),
    }
    let is = |k: &str| a.get(k).filter(|v| !later(v));
    if let Some(v) = is("open") {
        match v.as_str() {
            Some(u) if u.starts_with("http://") || u.starts_with("https://") => {}
            _ => problems.push(format!("open is an http or https address, not {v}")),
        }
    }
    for k in ["wait", "find", "click", "hover", "in", "into"] {
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
    if a.contains_key("in") && !a.contains_key("read") {
        problems.push("in goes with read".into());
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

fn fail(why: impl std::fmt::Display) -> Failure {
    Failure::Error(format!("#browser: {why}"))
}

pub fn run(r: &StepRun) -> Result<Outcome, Failure> {
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
    let mut out = Map::new();
    let acted = act(&mut cdp, &a, until, timeout, tab, r, &mut out);
    let mut cdp = match acted {
        Ok(Some(new)) => new,
        Ok(None) => cdp,
        Err(why) => return Err(fail(failed(&mut cdp, why, r))),
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
    match page::screenshot(cdp, &r.root.join(&shown)) {
        Ok(()) => msg.push_str(&format!("; screenshot: {}", shown.display())),
        Err(e) => msg.push_str(&format!("; no screenshot: {e}")),
    }
    msg
}

/// After a page loads: a banner rejected is logged; one that is not is
/// shown to the user, and `$output.blocked` says it may be in the way.
fn blocked(
    cdp: &mut Cdp,
    r: &StepRun,
    until: Instant,
    out: &mut Map<String, Value>,
) -> Result<(), String> {
    let mut kinds = vec![];
    for b in blockers::check(cdp, until)? {
        match b {
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
                let host = url.split('/').nth(2).unwrap_or(&url).to_string();
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
    until: Instant,
    timeout: Duration,
    tab: &str,
    r: &StepRun,
    out: &mut Map<String, Value>,
) -> Result<Option<Cdp>, String> {
    if let Some(url) = a.get("open").and_then(Value::as_str) {
        page::load(cdp, url, until)?;
        blocked(cdp, r, until, out)?;
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
            let _ = next.eval("new Promise(r => document.readyState === 'complete' ? r() : addEventListener('load', () => r()))");
            out.insert("new_tab".into(), json!(true));
            blocked(&mut next, r, until, out)?;
            return Ok(Some(next));
        }
        page::follow(cdp, until)?;
        blocked(cdp, r, until, out)?;
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
