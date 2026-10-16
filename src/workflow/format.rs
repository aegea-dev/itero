//! The workflow file as JSON: what people write, checked key by key (an
//! unknown key is an error, never ignored).
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Every state a run can be in, in order.
    pub states: Vec<String>,
    /// What starts a run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<Trigger>,
    /// Where a run begins.
    pub start: String,
    /// The closing states.
    #[serde(default)]
    pub end: Vec<String>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    /// How many runs may be under way at once, and what happens to a start
    /// beyond that. Without it, any number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
    /// Failed runs in a row that pause the workflow: 5 unless given, 0 never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_after: Option<u32>,
}

/// Failed runs in a row that pause a workflow when it says nothing.
pub const DEFAULT_PAUSE_AFTER: u32 = 5;

impl File {
    pub fn pause_after_or_default(&self) -> u32 {
        self.pause_after.unwrap_or(DEFAULT_PAUSE_AFTER)
    }
}

/// At most `max` runs under way; a start beyond waits its turn (`queue`)
/// or is dropped (`skip`).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Concurrency {
    pub max: u32,
    #[serde(default)]
    pub then: Then,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Then {
    #[default]
    Queue,
    Skip,
}

/// What starts a run: a schedule (cron, in the machine's local time) or an
/// app's event (`on`, its type), one of them, and the arguments the run
/// gets. A run an event starts has the event's data as its input.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub args: Map<String, Value>,
}

/// A move from one state to another.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The event type that triggers it; without, it starts on entering `from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// A condition that must hold for it to be taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// For an event for the workspace: whether it is this run's
    /// (`event.data.thread == run.input.thread`). A transition with a
    /// `match` hears its type's events for the workspace too.
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub r#match: Option<String>,
    /// The chain of actions, run in order.
    #[serde(default, rename = "do", skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_timeout: Option<String>,
    /// Where a failed chain goes instead of `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fail: Option<String>,
}

/// One action in a chain: a name like `json.pick@1`, or an object with
/// arguments and a check. Both forms read into this one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Action {
    pub action: String,
    /// Names its output: `steps.<id>` for the rest of the chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A condition: false skips the action, and `prev` passes through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<Map<String, Value>>,
    /// A condition over `output`; false means the action failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    action: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    with: Option<Map<String, Value>>,
    #[serde(default)]
    expect: Option<String>,
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(action) => Ok(Action {
                action,
                id: None,
                when: None,
                description: String::new(),
                with: None,
                expect: None,
            }),
            v @ Value::Object(_) => {
                let c: Call = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                Ok(Action {
                    action: c.action,
                    id: c.id,
                    when: c.when,
                    description: c.description,
                    with: c.with,
                    expect: c.expect,
                })
            }
            _ => Err(serde::de::Error::custom(
                "an action is a name like \"json.pick@1\", or an object with \"action\"",
            )),
        }
    }
}
