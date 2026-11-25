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
