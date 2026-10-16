//! The actions a workflow can name: itero's built-ins, and the workspace's
//! own (`actions/<name>/action.json`, named `<name>.<target>@<version>`), a
//! function's runtime and entry points, after NGINX Unit's application
//! objects. Running them is the manager's and workers' job; this reads and
//! checks them.
use super::validate::{check_templates, is_word};
use super::{WorkflowError, expr};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The built-in actions at their versions.
pub const BUILT_IN: [&str; 8] = [
    "memory.set@1",
    "json.pick@1",
    "event.emit@1",
    "event.publish@1",
    "http.request@1",
    "command@1",
    "sh@1",
    "notify@1",
];

/// Names a workspace action cannot take: the built-ins' prefixes.
pub const RESERVED: [&str; 7] = ["memory", "json", "event", "http", "command", "sh", "notify"];

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `python@<version>`, `bun@<version>`, `executable` or `container`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Container: the image its program runs in, like
    /// `python:3.13-alpine`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    /// Python: the virtual environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// Python: the requirements file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<String>,
    /// Named entry points.
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The most a job may take: nothing extends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    /// The job's lease: it renews it by touching `ITERO_HEARTBEAT`, or is
    /// killed and run again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat: Option<String>,
}

/// Seconds a job may take when its action sets no `limits.timeout`.
pub const DEFAULT_TIMEOUT: u64 = 60;

/// An entry point: a Python module and callable, a Bun file and export, or
/// a program and its arguments.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
}

/// Reads and checks `<folder>/<name>/action.json`.
pub fn load_file(path: &Path) -> Result<Definition, WorkflowError> {
    let err = |message: String| WorkflowError {
        file: path.display().to_string(),
        at: None,
        message,
    };
    let text = std::fs::read_to_string(path).map_err(|e| err(format!("cannot read it: {e}")))?;
    let d: Definition = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
    validate(path, &d)?;
    Ok(d)
}

fn type_name(kind: &str) -> Option<&str> {
    if kind == "executable" || kind == "container" {
        return Some(kind);
    }
    let (name, version) = kind.split_once('@')?;
    let ok = !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.');
    (ok && (name == "python" || name == "bun")).then_some(name)
}

fn check_target(kind: &str, t: &Target) -> Result<(), String> {
    let has = |o: &Option<String>| o.is_some();
    let (ok, what) = match kind {
        "python" => (
            has(&t.module)
                && has(&t.callable)
                && !has(&t.file)
                && !has(&t.export)
                && !has(&t.executable)
                && t.arguments.is_empty(),
            "a python@ target is a module and a callable",
        ),
        "bun" => (
            has(&t.file)
                && has(&t.export)
                && !has(&t.module)
                && !has(&t.callable)
                && !has(&t.executable)
                && t.arguments.is_empty(),
            "a bun@ target is a file and an export",
        ),
        _ => (
            has(&t.executable)
                && !has(&t.module)
                && !has(&t.callable)
                && !has(&t.file)
                && !has(&t.export),
            "an executable target is an executable and its arguments",
        ),
    };
    if ok { Ok(()) } else { Err(what.into()) }
}

/// Checks a definition; `path` names it, and its folder gives the name.
pub fn validate(path: &Path, d: &Definition) -> Result<(), WorkflowError> {
    let file = path.display().to_string();
    let whole = |message: String| WorkflowError {
        file: file.clone(),
        at: None,
        message,
    };
    let folder = path
        .parent()
        .and_then(|f| f.file_name())
        .and_then(|f| f.to_str())
        .unwrap_or("");
    if d.name != folder {
        return Err(whole(format!(
            "name \"{}\" must match the folder name \"{folder}\"",
            d.name
        )));
    }
    if d.name.contains('.') || d.name.contains('@') {
        return Err(whole(format!("name \"{}\" has no . or @", d.name)));
    }
    if RESERVED.contains(&d.name.as_str()) {
        return Err(whole(format!(
            "name \"{}\" is taken by a built-in action",
            d.name
        )));
    }
    if d.version == 0 {
        return Err(whole("version starts at 1".into()));
    }
    let kind = type_name(&d.kind).ok_or_else(|| {
        whole(format!(
            "type is python@<version>, bun@<version>, executable or container, not \"{}\"",
            d.kind
        ))
    })?;
    if kind != "python" && (d.home.is_some() || d.requirements.is_some()) {
        return Err(whole("home and requirements are for python@ types".into()));
    }
    match (kind, &d.image) {
        ("container", None) => return Err(whole("a container needs image".into())),
        ("container", Some(i)) if i.trim().is_empty() || i.starts_with('-') => {
            return Err(whole(format!("image \"{i}\" is not an image name")));
        }
        ("container", Some(_)) => {}
        (_, Some(_)) => return Err(whole("image is for the container type".into())),
        (_, None) => {}
    }
    if kind == "container" && d.limits.as_ref().is_some_and(|l| l.heartbeat.is_some()) {
        return Err(whole(
            "heartbeat does not reach a container yet: use timeout".into(),
        ));
    }
    if d.targets.is_empty() {
        return Err(whole("targets is empty".into()));
    }
    for (name, t) in &d.targets {
        let at = |message: String| WorkflowError {
            file: file.clone(),
            at: Some(format!("target \"{name}\"")),
            message,
        };
        if !is_word(name) {
            return Err(at("a target name is lowercase letters, digits and -".into()));
        }
        check_target(kind, t).map_err(at)?;
    }
    if let Some(l) = &d.limits {
        let timeout = match l.timeout.as_deref() {
            Some(t) => expr::duration(t).map_err(&whole)?,
            None => std::time::Duration::from_secs(DEFAULT_TIMEOUT),
        };
        if let Some(h) = l.heartbeat.as_deref()
            && expr::duration(h).map_err(&whole)? > timeout
        {
            return Err(whole("heartbeat is longer than timeout".into()));
        }
    }
    for v in d.environment.values() {
        check_templates(v).map_err(&whole)?;
    }
    Ok(())
}

/// A workspace action's name, `<name>.<target>@<version>`, in its parts.
pub fn split(name: &str) -> Option<(&str, &str, &str)> {
    let (what, version) = name.rsplit_once('@')?;
    let (def, target) = what.split_once('.')?;
    Some((def, target, version))
}

/// An environment variable's name: ASCII letters, digits and `_`, not
/// starting with a digit.
fn is_env_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Checks that a built-in has what it needs in `with`. Any other name must
/// read as a workspace action, `<name>.<target>@<version>`; whether there
/// is one is `workspace`'s check, against its actions folders.
pub fn check(name: &str, with: Option<&Map<String, Value>>) -> Result<(), String> {
    let empty = Map::new();
    let with = with.unwrap_or(&empty);
    let text = |k: &str| with.get(k).is_some_and(Value::is_string);
    let only = |keys: &[&str]| match with.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) => Err(format!("{name} takes no with.{k}")),
        None => Ok(()),
    };
    match name {
        "memory.set@1" => {
            if with.is_empty() {
                return Err("memory.set@1 needs at least one key in with".into());
            }
        }
        "json.pick@1" => {
            only(&["path"])?;
            if !text("path") {
                return Err("json.pick@1 needs with.path, like \"json.data\"".into());
            }
        }
        "event.emit@1" => {
            only(&["type", "target", "data"])?;
            if !text("type") || !text("target") {
                return Err("event.emit@1 needs with.type and with.target".into());
            }
        }
        "event.publish@1" => {
            only(&["type", "data"])?;
            if !text("type") {
                return Err("publish needs an event type".into());
            }
            // a type written out is checked now; one from a template when
            // the step runs
            let kind = with.get("type").and_then(Value::as_str).unwrap_or("");
            if !kind.contains("${") {
                if !kind.split('.').all(is_word) {
                    return Err(format!(
                        "publish \"{kind}\" is an event type, like report.ready"
                    ));
                }
                if crate::engine::events::engine_own(kind) {
                    return Err(format!("publish \"{kind}\" is the engine's own"));
                }
            }
            if !with.get("data").is_none_or(Value::is_object) {
                return Err("publish's data is an object".into());
            }
        }
        "http.request@1" => {
            only(&["url", "method", "headers", "json", "body", "max_bytes"])?;
            if !text("url") {
                return Err("http.request@1 needs with.url".into());
            }
            if with.contains_key("json") && with.contains_key("body") {
                return Err("http.request@1 takes with.json or with.body, not both".into());
            }
            if with.get("max_bytes").is_some_and(|v| v.as_u64().is_none()) {
                return Err(
                    "http.request@1 needs with.max_bytes as a whole number of bytes".into(),
                );
            }
        }
        "command@1" => {
            only(&["run"])?;
            let ok = with
                .get("run")
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_string));
            if !ok {
                return Err("command@1 needs with.run, a list of strings".into());
            }
        }
        "sh@1" => {
            only(&["run", "env"])?;
            let run = with.get("run").and_then(Value::as_str).unwrap_or("");
            if run.is_empty() {
                return Err("sh@1 needs with.run, a shell line".into());
            }
            if run.contains("${") {
                return Err("sh@1: a template in run; put values in env and use $NAME".into());
            }
            match with.get("env") {
                None => {}
                Some(Value::Object(env)) => {
                    if let Some(k) = env.keys().find(|k| !is_env_name(k)) {
                        return Err(format!(
                            "sh@1: env name \"{k}\" is letters, digits and _, not starting with a digit"
                        ));
                    }
                }
                Some(_) => return Err("sh@1 needs with.env as an object".into()),
            }
        }
        "notify@1" => {
            only(&["title", "text"])?;
            if !text("title") || !text("text") {
                return Err("notify@1 needs with.title and with.text".into());
            }
        }
        _ => {
            let ok = split(name).is_some_and(|(def, target, version)| {
                is_word(def) && is_word(target) && version.parse::<u32>().is_ok()
            });
            if !ok {
                return Err(format!(
                    "unknown action {name}: a built-in, or <name>.<target>@<version> from an actions folder"
                ));
            }
        }
    }
    Ok(())
}

/// Checks a workspace action's name against the definitions found: it is
/// there, with that target, at that version.
pub fn find<'a>(name: &str, defs: &'a [Definition]) -> Result<&'a Definition, String> {
    let unknown = || format!("unknown action {name}");
    let (def, target, version) = split(name).ok_or_else(unknown)?;
    let d = defs
        .iter()
        .find(|d| d.name == def)
        .ok_or_else(|| format!("no action {def} in the actions folders"))?;
    if !d.targets.contains_key(target) {
        return Err(format!("action {def} has no target {target}"));
    }
    if version != d.version.to_string() {
        return Err(format!(
            "action {def} is at version {}, not {version}",
            d.version
        ));
    }
    Ok(d)
}
