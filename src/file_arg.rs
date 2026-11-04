//! A file given as a step's arg, `{"file": "prompts/sort.md"}`: read when
//! the workflow is, its text in the object's place. Everything after that
//! sees a text: a run keeps the version it started with, `itero ws check`
//! finds a bad one, and no value can pick a file.
use serde_json::Value;
use std::path::{Component, Path};

/// Replaces each file in `workflow`'s steps with its text. One message per
/// file it cannot use; those stay as they are.
pub(crate) fn inline(root: &Path, workflow: &mut Value) -> Vec<String> {
    let mut problems = vec![];
    for key in ["steps", "success", "failure", "error"] {
        if let Some(list) = workflow.get_mut(key) {
            in_steps(root, list, &mut problems);
        }
    }
    problems
}

/// Finds each step's `args`, at any depth of lists, groups and undos, and
/// goes no deeper than that. A step's `on` holds no args.
fn in_steps(root: &Path, node: &mut Value, problems: &mut Vec<String>) {
    match node {
        Value::Array(items) => items.iter_mut().for_each(|i| in_steps(root, i, problems)),
        Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                match key.as_str() {
                    "args" => in_args(root, value, problems),
                    "on" => {}
                    _ => in_steps(root, value, problems),
                }
            }
        }
        _ => {}
    }
}

/// Args are an object, or a list of objects; each value directly in them
/// may be a file.
fn in_args(root: &Path, args: &mut Value, problems: &mut Vec<String>) {
    let mut each = |o: &mut serde_json::Map<String, Value>| {
        for v in o.values_mut() {
            if let Some(path) = path_of(v) {
                match text_of(root, &path) {
                    // the fill reads `\$` as a `$`: the text stays as written
                    Ok(text) => *v = Value::String(text.replace('$', "\\$")),
                    Err(why) => problems.push(why),
                }
            }
        }
    };
    match args {
        Value::Object(o) => each(o),
        Value::Array(items) => items
            .iter_mut()
            .filter_map(Value::as_object_mut)
            .for_each(&mut each),
        _ => {}
    }
}

/// The path, when `v` is an object with only the key `file`, a text.
fn path_of(v: &Value) -> Option<String> {
    let o = v.as_object().filter(|o| o.len() == 1)?;
    o.get("file")?.as_str().map(str::to_string)
}

/// The file's text. It must be inside the workspace, and not hold what the
/// workspace keeps to itself: a workspace someone shares must not be able to
/// send `.env` anywhere.
fn text_of(root: &Path, path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if p.is_absolute()
        || path.starts_with('~')
        || p.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(format!(
            "file {path}: a file is a path inside the workspace"
        ));
    }
    let fail = |e: std::io::Error| format!("file {path}: {e}");
    let real = root.join(p).canonicalize().map_err(fail)?;
    let home = root.canonicalize().map_err(fail)?;
    let Ok(inside) = real.strip_prefix(&home) else {
        return Err(format!("file {path}: a link leading out of the workspace"));
    };
    let named = |p: Option<&std::ffi::OsStr>, name: &str| {
        p.and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
    };
    let first = inside.components().next().map(|c| c.as_os_str());
    if named(first, ".itero") || named(inside.file_name(), ".env") {
        return Err(format!(
            "file {path}: .env and .itero are the workspace's own, not for an arg"
        ));
    }
    std::fs::read_to_string(&real).map_err(fail)
}
