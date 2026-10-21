//! A workflow file in the simple form (`docs/design-simple.md`): `on`,
//! `steps`, and the `success`, `failure` and `error` lists run at the end.
//! Its name is its file name, so the file has none.
use serde_json::{Map, Value};
use std::time::Duration;

/// How long a run may wait for data unless its workflow says.
pub const WAIT: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Flow {
    pub on: On,
    pub steps: Vec<Step>,
    pub success: Vec<Step>,
    pub failure: Vec<Step>,
    pub error: Vec<Step>,
    /// How long a run may wait for data; `None` is `WAIT`.
    pub timeout: Option<Duration>,
}

/// When a workflow or a step runs. Messages and schedules are
/// alternatives that start a run; the rest are conditions that must all
/// hold.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct On {
    /// Message types: `order.placed`.
    pub messages: Vec<String>,
    /// Cron lines: `0 9 * * *`.
    pub schedules: Vec<String>,
    /// State values that must be there: `$order` is `order`.
    pub present: Vec<String>,
    /// Environment variables or secrets that must be defined: `$$API` is
    /// `API`.
    pub defined: Vec<String>,
    /// State values that must be exactly these: `{"$order": 43}`.
    pub equal: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// An action or a workflow, with data for it and when it runs.
    One {
        name: Name,
        data: Map<String, Value>,
        on: Option<On>,
    },
    /// Actions and workflows that run side by side; the next step waits
    /// for all of them.
    Side(Vec<Name>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Name {
    /// `#check-payment`, `#notify@0.2`.
    Action {
        name: String,
        version: Option<String>,
    },
    /// `order-process`.
    Workflow(String),
}

/// Reads a workflow file's text.
pub fn parse(text: &str) -> Result<Flow, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    from_value(&v)
}

/// Reads a workflow already read as JSON: what the manager is given.
pub fn from_value(v: &Value) -> Result<Flow, String> {
    let Value::Object(top) = v else {
        return Err("a workflow is an object: { \"on\": [...], \"steps\": [...] }".into());
    };
    let mut f = Flow::default();
    for (k, v) in top {
        match k.as_str() {
            "on" => f.on = on(v)?,
            "steps" => f.steps = steps(k, v)?,
            "success" => f.success = steps(k, v)?,
            "failure" => f.failure = steps(k, v)?,
            "error" => f.error = steps(k, v)?,
            "timeout" => {
                let t = v.as_str().ok_or("timeout is a duration, like \"2h\"")?;
                f.timeout = Some(humantime::parse_duration(t).map_err(|_| {
                    format!("timeout: a duration is like 30m, 2h or 1d, not \"{t}\"")
                })?);
            }
            other => {
                return Err(format!(
                    "unknown key \"{other}\": a workflow has on, steps, success, failure, error and timeout"
                ));
            }
        }
    }
    if !top.contains_key("steps") {
        return Err("a workflow needs steps".into());
    }
    if f.steps.is_empty() {
        return Err("steps is empty".into());
    }
    Ok(f)
}

/// Whether `on`'s conditions hold over a run's state, `defined` telling
/// which environment variables and secrets are. Its messages and
/// schedules are not conditions: they say what starts a run.
pub fn holds(on: &On, state: &Map<String, Value>, defined: &dyn Fn(&str) -> bool) -> bool {
    on.present.iter().all(|k| state.contains_key(k))
        && on.defined.iter().all(|k| defined(k))
        && on.equal.iter().all(|(k, v)| state.get(k) == Some(v))
}

/// Two or more words joined by `.`: `order.placed`.
pub fn is_message_type(s: &str) -> bool {
    let words: Vec<&str> = s.split('.').collect();
    words.len() >= 2
        && words.iter().all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
}

fn on(v: &Value) -> Result<On, String> {
    let mut o = On::default();
    match v {
        Value::Array(items) => {
            for item in items {
                let s = item
                    .as_str()
                    .ok_or("on lists message types, schedules, $names and $$NAMES, as text")?;
                if let Some(n) = s.strip_prefix("$$") {
                    o.defined.push(word(n, s)?);
                } else if let Some(n) = s.strip_prefix('$') {
                    o.present.push(word(n, s)?);
                } else if s.contains(' ') {
                    crate::engine::schedule::check(s)?;
                    o.schedules.push(s.to_string());
                } else if is_message_type(s) {
                    o.messages.push(s.to_string());
                } else {
                    return Err(format!(
                        "on: \"{s}\" is not a message type (two or more words joined by .), a schedule, a $name or a $$NAME"
                    ));
                }
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                match k.strip_prefix('$') {
                    Some(n) if !n.starts_with('$') => {
                        o.equal.insert(word(n, k)?, v.clone());
                    }
                    _ => {
                        return Err(format!(
                            "on: \"{k}\" is not a $name: an object compares state values"
                        ));
                    }
                }
            }
        }
        _ => return Err("on is a list, or an object of $names and their values".into()),
    }
    Ok(o)
}

fn word(n: &str, whole: &str) -> Result<String, String> {
    if n.is_empty() || n.contains(char::is_whitespace) {
        return Err(format!("on: \"{whole}\" names nothing"));
    }
    Ok(n.to_string())
}

fn steps(key: &str, v: &Value) -> Result<Vec<Step>, String> {
    let items = v.as_array().ok_or(format!("{key} is a list"))?;
    items.iter().map(step).collect()
}

fn step(v: &Value) -> Result<Step, String> {
    match v {
        Value::String(s) => Ok(Step::One {
            name: name(s)?,
            data: Map::new(),
            on: None,
        }),
        Value::Array(items) => {
            let names = items
                .iter()
                .map(|i| {
                    i.as_str()
                        .ok_or(
                            "a list side by side holds names of actions and workflows".to_string(),
                        )
                        .and_then(name)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Step::Side(names))
        }
        Value::Object(m) => {
            let mut it = m.iter();
            let (Some((k, v)), None) = (it.next(), it.next()) else {
                return Err("a step object has one name: { \"#ship\": { ... } }".into());
            };
            let Value::Object(fields) = v else {
                return Err(format!("{k}: its data is an object"));
            };
            let mut data = fields.clone();
            let when = data.remove("on").map(|w| on(&w)).transpose()?;
            Ok(Step::One {
                name: name(k)?,
                data,
                on: when,
            })
        }
        _ => Err("a step is a name, a list side by side, or { name: { data } }".into()),
    }
}

/// `#name` or `#name@1.2` is an action, any other name a workflow.
fn name(s: &str) -> Result<Name, String> {
    let ok = |n: &str| {
        !n.is_empty() && !n.contains(['/', '#', '@', '$']) && !n.contains(char::is_whitespace)
    };
    match s.strip_prefix('#') {
        Some(rest) => {
            let (n, version) = match rest.split_once('@') {
                Some((n, v)) => (n, Some(v)),
                None => (rest, None),
            };
            if !ok(n) {
                return Err(format!("\"{s}\" is not an action's name"));
            }
            if let Some(v) = version
                && (v.is_empty()
                    || !v
                        .split('.')
                        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
            {
                return Err(format!(
                    "\"{s}\": a version is numbers joined by ., like 0.2"
                ));
            }
            Ok(Name::Action {
                name: n.to_string(),
                version: version.map(str::to_string),
            })
        }
        None if ok(s) => Ok(Name::Workflow(s.to_string())),
        None => Err(format!("\"{s}\" is not a workflow's name")),
    }
}

/// A version's numbers, compared as numbers: 0.10 is after 0.2.
pub fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}
