//! A page as text, from Chrome's accessibility tree: what a person sees,
//! in reading order, the same on every site. Text as it reads; `# ` before
//! a heading; `[name]` a link, `[button: name]` a button, `[label: ___]` a
//! field: the names in brackets are what a step's `click` takes.
use serde_json::Value;
use std::collections::HashMap;

/// The text of `nodes`, as `Accessibility.getFullAXTree` gives them.
pub fn from_tree(nodes: &[Value]) -> String {
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|n| Some((n["nodeId"].as_str()?, n)))
        .collect();
    let mut lines = Vec::new();
    if let Some(root) = nodes.first() {
        walk(root, &by_id, &mut lines);
    }
    tidy(lines)
}

/// The text of the part of the page that is DOM node `backend`, or
/// `None` when the tree has no node for it.
pub fn from_subtree(nodes: &[Value], backend: i64) -> Option<String> {
    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|n| Some((n["nodeId"].as_str()?, n)))
        .collect();
    let start = nodes
        .iter()
        .find(|n| n["backendDOMNodeId"].as_i64() == Some(backend))?;
    let mut lines = Vec::new();
    walk(start, &by_id, &mut lines);
    Some(tidy(lines))
}

fn tidy(lines: Vec<String>) -> String {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.is_empty() || out.last() == Some(&l) {
            continue;
        }
        out.push(l);
    }
    out.join("\n")
}

/// What reads as one line, however it is marked up inside.
const ONE_LINE: [&str; 9] = [
    "paragraph",
    "listitem",
    "cell",
    "gridcell",
    "columnheader",
    "rowheader",
    "caption",
    "LabelText",
    "label",
];

fn name(n: &Value) -> String {
    n["name"]["value"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn walk(n: &Value, by_id: &HashMap<&str, &Value>, out: &mut Vec<String>) {
    let children = |out: &mut Vec<String>| {
        for c in n["childIds"].as_array().into_iter().flatten() {
            if let Some(c) = c.as_str().and_then(|id| by_id.get(id)) {
                walk(c, by_id, out);
            }
        }
    };
    if n["ignored"].as_bool() == Some(true) {
        return children(out);
    }
    let named = |shape: &dyn Fn(&str) -> String, out: &mut Vec<String>| {
        let v = name(n);
        if !v.is_empty() {
            out.push(shape(&v));
        }
    };
    let role = n["role"]["value"].as_str().unwrap_or_default();
    if ONE_LINE.contains(&role) {
        // a paragraph's bold word or link is part of its line
        let mut inside = Vec::new();
        children(&mut inside);
        let line: Vec<String> = inside.into_iter().filter(|l| !l.is_empty()).collect();
        out.push(line.join(" "));
        return;
    }
    match role {
        "StaticText" => named(&|v| v.to_string(), out),
        "heading" => named(&|v| format!("# {v}"), out),
        "link" => named(&|v| format!("[{v}]"), out),
        "button" => named(&|v| format!("[button: {v}]"), out),
        "textbox" | "searchbox" | "combobox" => named(&|v| format!("[{v}: ___]"), out),
        "img" | "image" => {}
        _ => children(out),
    }
}
