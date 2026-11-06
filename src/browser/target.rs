//! What a step names on a page: a link, button or field by the name a
//! person sees (the accessibility tree's), else an element showing that
//! text; or an element by CSS selector. And where to put the mouse on it.
use super::cdp::Cdp;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Name(String),
    Css(String),
}

impl Target {
    /// A string, a name; or `{"css": "selector"}`.
    pub fn parse(v: &Value) -> Result<Target, String> {
        match v {
            Value::String(s) if s.trim().is_empty() => Err("a target is empty".into()),
            Value::String(s) => Ok(Target::Name(s.trim().into())),
            Value::Object(m) => {
                if m.keys().any(|k| k != "css") {
                    return Err(format!(
                        "a target object has only css, not {}",
                        Value::Object(m.clone())
                    ));
                }
                match m.get("css") {
                    Some(Value::String(s)) if !s.trim().is_empty() => {
                        Ok(Target::Css(s.trim().into()))
                    }
                    _ => Err("a target's css is a text: a CSS selector".into()),
                }
            }
            other => Err(format!(
                "a target is a name or {{\"css\": \"selector\"}}, not {other}"
            )),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Target::Name(n) => format!("\"{n}\""),
            Target::Css(c) => format!("css {c}"),
        }
    }
}

/// What was found: how many matched, the first in reading order, and its
/// text.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub count: usize,
    pub backend: i64,
    pub text: String,
}

/// The roles a name is looked for first: what a person clicks or types in.
const ACTIVE: [&str; 13] = [
    "link",
    "button",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "menuitem",
    "tab",
    "option",
    "listbox",
    "slider",
];

/// The target on the page now, or `None`. Never waits.
pub fn find(cdp: &mut Cdp, t: &Target) -> Result<Option<Found>, String> {
    let doc = cdp.send("DOM.getDocument", json!({ "depth": 0 }))?;
    let root = doc["root"]["nodeId"].clone();
    match t {
        Target::Css(sel) => {
            let r = cdp
                .send(
                    "DOM.querySelectorAll",
                    json!({ "nodeId": root, "selector": sel }),
                )
                .map_err(|_| format!("not a CSS selector: {sel}"))?;
            let ids = r["nodeIds"].as_array().cloned().unwrap_or_default();
            let Some(first) = ids.first() else {
                return Ok(None);
            };
            let d = cdp.send("DOM.describeNode", json!({ "nodeId": first }))?;
            let backend = d["node"]["backendNodeId"].as_i64().ok_or("no node")?;
            let text = text_of(cdp, backend)?;
            Ok(Some(Found {
                count: ids.len(),
                backend,
                text,
            }))
        }
        Target::Name(name) => {
            let r = cdp.send(
                "Accessibility.queryAXTree",
                json!({ "nodeId": root, "accessibleName": name }),
            )?;
            let nodes: Vec<Value> = r["nodes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|n| n["ignored"].as_bool() != Some(true) && n["backendDOMNodeId"].is_i64())
                .collect();
            let role = |n: &Value| n["role"]["value"].as_str().unwrap_or_default().to_string();
            let active: Vec<&Value> = nodes
                .iter()
                .filter(|n| ACTIVE.contains(&role(n).as_str()))
                .collect();
            let other: Vec<&Value> = nodes
                .iter()
                .filter(|n| {
                    !ACTIVE.contains(&role(n).as_str())
                        && role(n) != "StaticText"
                        && role(n) != "InlineTextBox"
                })
                .collect();
            let text: Vec<&Value> = nodes.iter().filter(|n| role(n) == "StaticText").collect();
            let (tier, of_text) = if !active.is_empty() {
                (active, false)
            } else if !other.is_empty() {
                (other, false)
            } else {
                (text, true)
            };
            let Some(first) = tier.first() else {
                return Ok(None);
            };
            let mut backend = first["backendDOMNodeId"].as_i64().expect("filtered");
            if of_text {
                // a text: the element showing it
                backend = element_of(cdp, backend)?;
            }
            Ok(Some(Found {
                count: tier.len(),
                backend,
                text: name.clone(),
            }))
        }
    }
}

fn object(cdp: &mut Cdp, backend: i64) -> Result<String, String> {
    let r = cdp.send("DOM.resolveNode", json!({ "backendNodeId": backend }))?;
    r["object"]["objectId"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "the element is gone".into())
}

fn call(
    cdp: &mut Cdp,
    backend: i64,
    f: &str,
    args: Value,
    by_value: bool,
) -> Result<Value, String> {
    let id = object(cdp, backend)?;
    let r = cdp.send(
        "Runtime.callFunctionOn",
        json!({ "objectId": id, "functionDeclaration": f, "arguments": args, "returnByValue": by_value }),
    )?;
    if let Some(x) = r.get("exceptionDetails") {
        return Err(x["exception"]["description"]
            .as_str()
            .unwrap_or("an exception")
            .to_string());
    }
    Ok(r["result"].clone())
}

fn element_of(cdp: &mut Cdp, backend: i64) -> Result<i64, String> {
    let r = call(
        cdp,
        backend,
        "function(){return this.nodeType===1?this:this.parentElement}",
        json!([]),
        false,
    )?;
    let id = r["objectId"].as_str().ok_or("no element shows that text")?;
    let d = cdp.send("DOM.describeNode", json!({ "objectId": id }))?;
    d["node"]["backendNodeId"]
        .as_i64()
        .ok_or_else(|| "no element shows that text".into())
}

fn text_of(cdp: &mut Cdp, backend: i64) -> Result<String, String> {
    let r = call(
        cdp,
        backend,
        "function(){return (this.innerText ?? this.textContent ?? '').split(/\\s+/).join(' ').trim()}",
        json!([]),
        true,
    )?;
    Ok(r["value"].as_str().unwrap_or_default().to_string())
}

/// How long a page is given to settle after scrolling.
const SETTLE: Duration = Duration::from_millis(300);

/// Where to click `backend`: scrolled into view, measured after the page
/// settled, and checked to be what lies at that point (itself, inside
/// it, or a link to the same place laid over it); else what covers it.
pub fn point(cdp: &mut Cdp, backend: i64) -> Result<(f64, f64), String> {
    cdp.send(
        "DOM.scrollIntoViewIfNeeded",
        json!({ "backendNodeId": backend }),
    )
    .map_err(|_| "it is not shown on the page".to_string())?;
    std::thread::sleep(SETTLE);
    let m = cdp
        .send("DOM.getBoxModel", json!({ "backendNodeId": backend }))
        .map_err(|_| "it is not shown on the page".to_string())?;
    let q: Vec<f64> = m["model"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    if q.len() != 8 {
        return Err("it has no box on the page".into());
    }
    let x = (q[0] + q[2] + q[4] + q[6]) / 4.0;
    let y = (q[1] + q[3] + q[5] + q[7]) / 4.0;
    let over = call(
        cdp,
        backend,
        "function(x,y){const h=document.elementFromPoint(x,y);if(!h)return 'nothing';\
         if(this===h||this.contains(h))return '';\
         const a=this.closest&&this.closest('a[href]'),b=h.closest('a[href]');\
         if(a&&b&&a.href===b.href)return '';\
         return h.tagName.toLowerCase()+(h.id?'#'+h.id:'')+(typeof h.className==='string'&&h.className?'.'+h.className.split(' ')[0]:'')}",
        json!([{ "value": x }, { "value": y }]),
        true,
    )?;
    match over["value"].as_str() {
        Some("") => Ok((x, y)),
        Some(what) => Err(format!("{what} lies over it")),
        None => Err("cannot tell what lies over it".into()),
    }
}
