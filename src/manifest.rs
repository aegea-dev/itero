//! itero.config.json described once: the types serde reads and schemars
//! turns into the JSON Schema that a config is checked against, and that
//! editors use. Every key is itero's; a person's own go in `metadata`.
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A workspace's name: a DNS label, so it is safe in a URL, a shell and an
/// address.
pub const NAME: &str = r"^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$";
/// A size: a number and KB, MB or GB (of 1024).
pub const SIZE: &str = r"^[0-9]+ ?(KB|MB|GB)$";
/// A duration, as humantime reads it: `30m`, `2h`, `1d`, `1h 30m`.
pub const DURATION: &str = r"^ *[0-9]+ *(ns|nsec|us|usec|ms|msec|s|sec|secs|second|seconds|m|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days|w|week|weeks|M|month|months|y|year|years)( *[0-9]+ *(ns|nsec|us|usec|ms|msec|s|sec|secs|second|seconds|m|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days|w|week|weeks|M|month|months|y|year|years))* *$";
/// A date (a whole day) or an RFC 3339 time.
pub const MOMENT: &str = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}(T.+)?$";

/// itero.config.json: what a workspace is, how it runs, and an entry per
/// workflow, action and connector. Every key is itero's; your own go in
/// `metadata`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "itero.config.json")]
pub struct Manifest {
    /// The schema this file follows.
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    /// The workspace's name: lowercase letters, digits and hyphens, 1 to 63,
    /// a letter or digit at each end.
    #[schemars(regex(pattern = NAME))]
    pub name: Option<String>,
    /// Its version, as you number it.
    pub version: Option<String>,
    /// What it is.
    pub description: Option<String>,
    /// Words to find it by.
    pub tags: Option<Vec<String>>,
    /// The HTTP API's port, on 127.0.0.1; without it, the OS gives one.
    #[schemars(range(min = 1, max = 65535))]
    pub port: Option<u16>,
    /// The folders with its workflows, actions and connectors.
    pub folders: Option<Folders>,
    /// How much it may hold and do at once.
    pub limits: Option<RawLimits>,
    /// An entry per workflow, by its file's name.
    pub workflow: Option<BTreeMap<String, Entry<WorkflowEntry>>>,
    /// An entry per action, by its file's name.
    pub action: Option<BTreeMap<String, Entry<PlainEntry>>>,
    /// An entry per connector, by the event type it sends.
    pub connector: Option<BTreeMap<String, Entry<PlainEntry>>>,
    /// Your own keys: anything, never checked.
    pub metadata: Option<Map<String, Value>>,
}

/// `true`: on, with its defaults; `false`: off; or the entry written out.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "{T}OrSwitch")]
pub enum Entry<T> {
    Switch(bool),
    Written(T),
}

/// A workflow's entry: what it is, and when it may start.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowEntry {
    /// What it does.
    pub description: Option<String>,
    /// Words to find it by.
    pub tags: Option<Vec<String>>,
    /// Nothing starts it.
    #[schemars(extend("default" = false))]
    pub off: Option<bool>,
    /// Nothing starts it before then: a date (its first second) or an RFC 3339 time.
    #[schemars(regex(pattern = MOMENT))]
    pub from: Option<String>,
    /// Nothing starts it from then on: a date (after its last second) or an RFC 3339 time.
    #[schemars(regex(pattern = MOMENT))]
    pub until: Option<String>,
    /// It looks after the workspace: left out of lists, and outside the limits.
    #[schemars(extend("default" = false))]
    pub internal: Option<bool>,
    /// Your own keys: anything, never checked.
    pub metadata: Option<Map<String, Value>>,
}

/// An action's or a connector's entry: what it is.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlainEntry {
    /// What it does.
    pub description: Option<String>,
    /// Words to find it by.
    pub tags: Option<Vec<String>>,
    /// Your own keys: anything, never checked.
    pub metadata: Option<Map<String, Value>>,
}

/// `folders` as written: each a list of folders, relative to the workspace
/// (`~/` is the home folder).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Folders {
    /// The folders with workflows.
    #[serde(default = "default_workflows")]
    pub workflows: Vec<String>,
    /// The folders with actions.
    #[serde(default = "default_actions")]
    pub actions: Vec<String>,
    /// The folders with connectors.
    #[serde(default = "default_connectors")]
    pub connectors: Vec<String>,
}

impl Default for Folders {
    fn default() -> Folders {
        Folders {
            workflows: default_workflows(),
            actions: default_actions(),
            connectors: default_connectors(),
        }
    }
}

fn default_connectors() -> Vec<String> {
    vec!["connectors".into()]
}

fn default_workflows() -> Vec<String> {
    vec!["workflows".into()]
}

fn default_actions() -> Vec<String> {
    vec!["actions".into()]
}

/// `limits` as written: sizes and durations are text, `"1GB"`, `"1d"`.
#[derive(Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct RawLimits {
    /// Workflows in its folders, the internal ones apart.
    #[schemars(range(min = 1), extend("default" = 100))]
    pub workflows: Option<usize>,
    /// Runs under way at once; past it, events wait their turn.
    #[schemars(range(min = 1), extend("default" = 1000))]
    pub runs: Option<usize>,
    /// Steps running at once.
    #[schemars(range(min = 1), extend("default" = 64))]
    pub steps: Option<usize>,
    /// What `.itero/` may take: a size.
    #[schemars(regex(pattern = SIZE), extend("default" = "1GB"))]
    pub space: Option<String>,
    /// How long ended runs and their events are kept: a duration.
    #[schemars(regex(pattern = DURATION), extend("default" = "1d"))]
    pub keep: Option<String>,
    /// The size the log may reach before it is rolled.
    #[schemars(regex(pattern = SIZE), extend("default" = "10MB"))]
    pub log: Option<String>,
    /// Rolled logs kept.
    #[schemars(range(min = 1), extend("default" = 3))]
    pub logs: Option<usize>,
    /// How long a step may say nothing before it is stopped.
    #[schemars(regex(pattern = DURATION), extend("default" = "10m"))]
    pub quiet: Option<String>,
    /// The longest a step may run.
    #[schemars(regex(pattern = DURATION), extend("default" = "1d"))]
    pub most: Option<String>,
}

/// The schema of itero.config.json, as published.
pub fn schema() -> Value {
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .with_transform(schemars::transform::RecursiveTransform(no_null))
        .into_generator()
        .into_root_schema_for::<Manifest>();
    serde_json::to_value(schema).expect("a schema is JSON")
}

/// A key left out takes its default; `null` is the value of none. schemars
/// lets an `Option` be `null`: taken off its `type`, and its `anyOf`'s
/// `{"type": "null"}` branch.
fn no_null(schema: &mut schemars::Schema) {
    let Some(o) = schema.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(types)) = o.get_mut("type") {
        types.retain(|t| t != "null");
        if let [one] = types.as_slice() {
            let one = one.clone();
            o.insert("type".into(), one);
        }
    }
    if let Some(Value::Array(any)) = o.get("anyOf") {
        let null = serde_json::json!({ "type": "null" });
        let kept: Vec<Value> = any.iter().filter(|b| **b != null).cloned().collect();
        if kept.len() < any.len() {
            o.remove("anyOf");
            match kept.as_slice() {
                [Value::Object(one)] => o.extend(one.clone()),
                _ => {
                    o.insert("anyOf".into(), Value::Array(kept));
                }
            }
        }
    }
}

use crate::problem::Problem;
use crate::workspace::MANIFEST;
use jsonschema::error::{TypeKind, ValidationError, ValidationErrorKind};
use jsonschema::paths::LocationSegment;

/// Keys from before, by the entry kind they were in (`""`: the top), and
/// where each went; `<name>` is the entry's name.
const GONE: [(&str, &str, &str); 8] = [
    (
        "workflow",
        "settings",
        "settings are gone: itero.config.json holds no values; give them in .env ($$NAME), in a step's args, or in the event",
    ),
    (
        "action",
        "args",
        "args are gone: an action's args are written in the step that runs it",
    ),
    (
        "action",
        "repeat",
        r##"repeat is gone: repeat goes on the step: { "#<name>": { "repeat": ... } }"##,
    ),
    (
        "action",
        "limits",
        r##"limits are gone: write them on the steps that run it: { "#<name>": { "limits": { "quiet": "1h" } } }"##,
    ),
    (
        "connector",
        "args",
        r#"args are gone: write them in the on entry of each workflow that listens: { "event": "<name>", "args": {...} }"#,
    ),
    ("", "id", "id is given by itero and cannot be set"),
    ("", "created", "created is given by itero and cannot be set"),
    ("", "path", "path is given by itero and cannot be set"),
];

/// A config's JSON, when it parses, and every problem in it, each at its
/// address in the workspace (`""`, `limits`, `workflows/<name>`): what the
/// schema refuses, worded as itero words it, and what a value's own
/// parser refuses that its pattern let through.
pub fn problems(text: &str) -> (Option<Value>, Vec<Problem>) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        // past a syntax error nothing more of the file can be read
        Err(e) => return (None, vec![said("", format!("not JSON: {e}"))]),
    };
    let mut out = vec![];
    for e in validator().iter_errors(&v) {
        out.extend(worded(&e, &v));
    }
    let told: Vec<String> = out.iter().map(|p| p.at.clone()).collect();
    out.extend(read_values(&v, &told));
    (Some(v), out)
}

/// The schema, made once.
fn made() -> &'static Value {
    static S: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    S.get_or_init(schema)
}

/// The schema, compiled once.
fn validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| jsonschema::validator_for(made()).expect("itero's schema compiles"))
}

/// A problem at `at`, its message after the file's name.
fn said(at: &str, message: impl std::fmt::Display) -> Problem {
    Problem::new(at, format!("{MANIFEST}: {message}"))
}

/// An error's place in the file, as names: `["workflow", "gözcü", "from"]`.
fn segments_of(e: &ValidationError) -> Vec<String> {
    e.instance_path()
        .iter()
        .map(|s| match s {
            LocationSegment::Property(p) => p.into_owned(),
            LocationSegment::Index(i) => i.to_string(),
        })
        .collect()
}

/// The address of a place in the workspace: an entry's, `limits`,
/// `folders`, or the workspace itself.
fn address(place: &[String]) -> String {
    match place {
        [kind, name, ..] if ENTRIES.contains(&kind.as_str()) => format!("{kind}s/{name}"),
        [key, ..] if key == "limits" || key == "folders" => key.clone(),
        _ => String::new(),
    }
}

const ENTRIES: [&str; 3] = ["workflow", "action", "connector"];

/// The keys an object of the schema's `$defs` may hold, in its order.
fn keys_of(def: &str) -> String {
    let keys: Vec<String> = made()["$defs"][def]["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    keys.join(", ")
}

/// One of the schema's errors as itero tells it: one problem, or the
/// problems of an entry written out as an object.
fn worded(e: &ValidationError, v: &Value) -> Vec<Problem> {
    let place = segments_of(e);
    let at = address(&place);
    let key = place.last().cloned().unwrap_or_default();
    let one = |m: String| vec![said(&at, m)];
    match e.kind() {
        ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
            .iter()
            .map(|k| said(&at, unknown_key(&place, k)))
            .collect(),
        // an entry: true, false, or an object; told by the object's keys
        ValidationErrorKind::AnyOf { context } if entry(&place) => {
            if pointer(v, &place).is_some_and(Value::is_object) {
                context
                    .iter()
                    .flatten()
                    // the `true`/`false` branch's own "not a boolean" says nothing
                    .filter(|e| {
                        !matches!(e.kind(), ValidationErrorKind::Type { .. })
                            || segments_of(e) != place
                    })
                    .flat_map(|e| worded(e, v))
                    .collect()
            } else {
                let what = match place[0].as_str() {
                    "workflow" => "a workflow",
                    "action" => "an action",
                    _ => "a connector",
                };
                one(format!(
                    r#"{what} entry is true, false or an object: {{ "description": "..." }}"#
                ))
            }
        }
        ValidationErrorKind::Type { .. }
        | ValidationErrorKind::Minimum { .. }
        | ValidationErrorKind::Maximum { .. }
            if place == ["port"] =>
        {
            one("port is a number from 1 to 65535".into())
        }
        ValidationErrorKind::Minimum { limit } => one(format!("{key} is at least {limit}")),
        ValidationErrorKind::Maximum { limit } => one(format!("{key} is at most {limit}")),
        ValidationErrorKind::Type { kind } => one(format!("{key} is {}", expected(kind))),
        ValidationErrorKind::Pattern { .. } => {
            let was = pointer(v, &place)
                .and_then(Value::as_str)
                .unwrap_or_default();
            one(pattern_said(&place, &key, was))
        }
        // a kind itero has no words for: jsonschema's own, never dropped
        _ => one(e.to_string()),
    }
}

/// Whether a place is an entry itself: `["workflow", "gözcü"]`.
fn entry(place: &[String]) -> bool {
    place.len() == 2 && ENTRIES.contains(&place[0].as_str())
}

/// The value at a place in the file.
fn pointer<'v>(v: &'v Value, place: &[String]) -> Option<&'v Value> {
    place.iter().try_fold(v, |v, k| match v {
        Value::Object(m) => m.get(k),
        Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    })
}

/// A key the schema does not have, where it was found: where it went, if
/// it is one from before, else which keys there are.
fn unknown_key(place: &[String], k: &str) -> String {
    let kind = match place {
        [] => "",
        [kind, _] if ENTRIES.contains(&kind.as_str()) => kind.as_str(),
        _ => "-",
    };
    if let Some((_, _, went)) = GONE.iter().find(|(gk, key, _)| *gk == kind && *key == k) {
        let name = place.get(1).map(String::as_str).unwrap_or_default();
        return went.replace("<name>", name);
    }
    match place {
        [] => format!(r#""{k}" is not a key of itero.config.json; your own keys go in "metadata""#),
        [kind, _] if kind == "workflow" => {
            format!(
                r#""{k}" is not a key of a workflow entry: {}"#,
                keys_of("WorkflowEntry")
            )
        }
        [kind, _] if kind == "action" => {
            format!(
                r#""{k}" is not a key of an action entry: {}"#,
                keys_of("PlainEntry")
            )
        }
        [kind, _] if kind == "connector" => {
            format!(
                r#""{k}" is not a key of a connector entry: {}"#,
                keys_of("PlainEntry")
            )
        }
        [key] if key == "limits" => format!(r#""{k}" is not a limit: {}"#, keys_of("RawLimits")),
        [key] if key == "folders" => {
            format!(r#""{k}" is not a key of folders: {}"#, keys_of("Folders"))
        }
        _ => format!(r#""{k}" is not a key here"#),
    }
}

/// What a value of the type the schema wants is, in words.
fn expected(kind: &TypeKind) -> &'static str {
    let name = match kind {
        TypeKind::Single(t) => t.to_string(),
        // the schema gives each key one type
        TypeKind::Multiple(_) => String::new(),
    };
    match name.as_str() {
        "string" => "text",
        "boolean" => "true or false",
        "integer" | "number" => "a number",
        "array" => "a list of text",
        "object" => "an object",
        _ => "of another type",
    }
}

/// A text that does not have the form of its key.
fn pattern_said(place: &[String], key: &str, was: &str) -> String {
    match (place.first().map(String::as_str), key) {
        (Some("name"), _) => {
            "name: lowercase letters, digits and hyphens, 1 to 63, a letter or digit at each end"
                .into()
        }
        (Some("limits"), "space" | "log") => {
            format!(r#"{key}: a size is like 10MB or 1GB, not "{was}""#)
        }
        (Some("limits"), _) => format!(r#"{key}: a duration is like 30m, 2h or 1d, not "{was}""#),
        (_, "from" | "until") => {
            format!("{key}: a date (2026-11-01) or a time (2026-11-01T09:00:00Z)")
        }
        _ => format!(r#"{key} is not of its form: "{was}""#),
    }
}

/// What a value's own parser refuses, where the schema said nothing: a
/// duration of 0s, a size too big to count, a date that is no day.
fn read_values(v: &Value, told: &[String]) -> Vec<Problem> {
    let mut out = vec![];
    let fresh = |at: &str| !told.iter().any(|t| t == at);
    if let Some(Value::Object(limits)) = v.get("limits")
        && fresh("limits")
    {
        for (k, x) in limits {
            let Some(t) = x.as_str() else { continue };
            let read = match k.as_str() {
                "space" | "log" => crate::workspace::size(k, Some(t.into()), 0).err(),
                "keep" | "quiet" | "most" => crate::workspace::duration(k, Some(t.into()), 0).err(),
                _ => None,
            };
            if let Some(e) = read {
                let m = e.to_string();
                out.push(said("limits", m.strip_prefix("limits: ").unwrap_or(&m)));
            }
        }
    }
    if let Some(Value::Object(flows)) = v.get("workflow") {
        for (name, e) in flows {
            let at = format!("workflows/{name}");
            let Value::Object(e) = e else { continue };
            if !fresh(&at) {
                continue;
            }
            for (k, end) in [("from", false), ("until", true)] {
                if let Some(x) = e.get(k)
                    && let Err(m) = crate::flow::moment(k, x, end)
                {
                    out.push(said(&at, m));
                }
            }
        }
    }
    out
}
