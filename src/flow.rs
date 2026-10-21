//! A workflow file in the simple form (`docs/design-simple.md`): `on`,
//! `steps`, and the `success`, `failure` and `error` lists run at the end.
//! Its name is its file name, so the file has none.
use chrono::{DateTime, Utc};
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
    pub meta: Meta,
}

/// What a person says about a workflow: a few keys itero knows, and any
/// others, kept and shown.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Meta {
    pub description: Option<String>,
    /// Nothing starts it before this.
    pub from: Option<DateTime<Utc>>,
    /// Nothing starts it from this on.
    pub until: Option<DateTime<Utc>>,
    /// Nothing starts it.
    pub off: bool,
    /// It looks after the workspace.
    pub internal: bool,
    /// The person's own keys.
    pub other: Map<String, Value>,
}

/// The names itero gives and nobody sets: in a workflow's `meta` and the
/// workspace's `[meta]` they are an error.
pub fn given_by_itero(key: &str) -> Result<(), String> {
    match key {
        "id" | "name" | "created" | "path" | "version" => {
            Err(format!("meta: {key} is given by itero and cannot be set"))
        }
        _ => Ok(()),
    }
}

/// Whether anything may start a workflow at `now`: it is not off, and
/// `now` is within its `from` and `until`.
pub fn active(meta: &Meta, now: DateTime<Utc>) -> bool {
    !meta.off && meta.from.is_none_or(|f| now >= f) && meta.until.is_none_or(|u| now < u)
}

fn meta(v: &Value) -> Result<Meta, String> {
    let Value::Object(m) = v else {
        return Err("meta is an object: { \"description\": \"...\" }".into());
    };
    let flag = |k: &str, v: &Value| v.as_bool().ok_or(format!("meta: {k} is true or false"));
    let mut out = Meta::default();
    for (k, v) in m {
        given_by_itero(k)?;
        match k.as_str() {
            "description" => {
                out.description = Some(v.as_str().ok_or("meta: description is text")?.into())
            }
            "from" => out.from = Some(moment(k, v, false)?),
            "until" => out.until = Some(moment(k, v, true)?),
            "off" => out.off = flag(k, v)?,
            "internal" => out.internal = flag(k, v)?,
            _ => {
                out.other.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(out)
}

/// A date or an RFC 3339 time. A date is a whole day: `from` it starts at
/// its first second, `until` it ends after its last.
fn moment(key: &str, v: &Value, end: bool) -> Result<DateTime<Utc>, String> {
    let wrong = || format!("meta: {key}: a date (2026-11-01) or a time (2026-11-01T09:00:00Z)");
    let s = v.as_str().ok_or_else(wrong)?;
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Utc));
    }
    let day = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| wrong())?;
    let day = if end {
        day.succ_opt().ok_or_else(wrong)?
    } else {
        day
    };
    Ok(day.and_time(chrono::NaiveTime::MIN).and_utc())
}

/// When a workflow or a step runs. Messages and schedules are
/// alternatives that start a run; the rest are conditions that must all
/// hold.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct On {
    /// `{"event": "order.placed"}`: message types.
    pub events: Vec<String>,
    /// `{"schedule": "0 9 * * *"}`: cron lines.
    pub schedules: Vec<String>,
    /// `{"path": "/var/mail/$$USER"}`: files or folders, as written.
    pub paths: Vec<String>,
    /// `{"has": ...}`: `$order` must be in the state, `$$API` defined (an
    /// environment variable or a secret); kept as written.
    pub has: Vec<String>,
    /// `{"$paid": true}`: state values that must be exactly these, by name.
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
            "meta" => f.meta = meta(v)?,
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
                    "unknown key \"{other}\": a workflow has meta, on, steps, success, failure, error and timeout"
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
    on.has.iter().all(|h| match h.strip_prefix("$$") {
        Some(n) => defined(n),
        None => state.contains_key(h.trim_start_matches('$')),
    }) && on.equal.iter().all(|(k, v)| state.get(k) == Some(v))
}

/// Whether anything in `on` starts a run: an event, a schedule or a path.
/// A workflow with none runs only as a step of another.
pub fn starts(on: &On) -> bool {
    !(on.events.is_empty() && on.schedules.is_empty() && on.paths.is_empty())
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

/// `on`: a list of objects, each saying what it is with its key.
fn on(v: &Value) -> Result<On, String> {
    const FORMS: &str = "{\"event\": ...}, {\"schedule\": ...}, {\"path\": ...}, {\"$name\": value} or {\"has\": ...}";
    let Value::Array(items) = v else {
        return Err(format!("on is a list of objects: {FORMS}"));
    };
    let mut o = On::default();
    for item in items {
        let Value::Object(m) = item else {
            return Err(format!("on: {item} is not an object; an entry is {FORMS}"));
        };
        let trigger = ["event", "schedule", "path"]
            .iter()
            .any(|k| m.contains_key(*k));
        if trigger && m.len() > 1 {
            return Err(format!(
                "on: {item} says more than one thing: an event, a schedule or a path is an entry of its own"
            ));
        }
        for (k, v) in m {
            match k.as_str() {
                "event" => {
                    let s = v.as_str().unwrap_or("");
                    if !is_message_type(s) {
                        return Err(format!(
                            "on: event {v} is not a message type (two or more words joined by .)"
                        ));
                    }
                    o.events.push(s.to_string());
                }
                "schedule" => {
                    let s = v.as_str().ok_or("on: a schedule is a cron line, as text")?;
                    crate::engine::schedule::check(s)?;
                    o.schedules.push(s.to_string());
                }
                "path" => match v.as_str() {
                    Some(s) if !s.trim().is_empty() => o.paths.push(s.to_string()),
                    _ => return Err(format!("on: path {v} is not a file or folder's path")),
                },
                "has" => {
                    let names: Vec<&Value> = match v {
                        Value::Array(a) => a.iter().collect(),
                        one => vec![one],
                    };
                    for n in names {
                        let s = n.as_str().ok_or(format!(
                            "on: has takes \"$name\", \"$$NAME\" or a list of them, not {v}"
                        ))?;
                        let bare = s.strip_prefix("$$").or_else(|| s.strip_prefix('$'));
                        match bare {
                            Some(b) => {
                                word(b, s)?;
                                o.has.push(s.to_string());
                            }
                            None => {
                                return Err(format!("on: has \"{s}\" is not a $name or a $$NAME"));
                            }
                        }
                    }
                }
                _ => match k.strip_prefix('$') {
                    Some(n) if !n.starts_with('$') => {
                        o.equal.insert(word(n, k)?, v.clone());
                    }
                    _ => {
                        return Err(format!(
                            "on: \"{k}\" is not one of event, schedule, path, has or a $name"
                        ));
                    }
                },
            }
        }
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

impl Flow {
    /// Every step of every list: steps, success, failure, error.
    fn all_steps(&self) -> impl Iterator<Item = &Step> {
        self.steps
            .iter()
            .chain(&self.success)
            .chain(&self.failure)
            .chain(&self.error)
    }

    /// The actions and workflows it names, each once, in order.
    pub fn uses(&self) -> Vec<Name> {
        let mut out: Vec<Name> = vec![];
        for s in self.all_steps() {
            let names: Vec<&Name> = match s {
                Step::One { name, .. } => vec![name],
                Step::Side(names) => names.iter().collect(),
            };
            for n in names {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }

    /// The `$$` names its conditions check: in its `on` and its steps'.
    pub fn defined_names(&self) -> Vec<String> {
        let names = |on: &On| -> Vec<String> {
            on.has
                .iter()
                .filter_map(|h| h.strip_prefix("$$").map(str::to_string))
                .collect()
        };
        let mut out = names(&self.on);
        for s in self.all_steps() {
            if let Step::One { on: Some(on), .. } = s {
                out.extend(names(on));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The `$$` names in its steps' data: the secrets its actions get.
    pub fn secret_names(&self) -> Vec<String> {
        let mut out = vec![];
        for s in self.all_steps() {
            if let Step::One { data, .. } = s {
                out.extend(data.values().filter_map(secret_ref).map(str::to_string));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// `"$$NAME"`: the name of the environment variable or secret it stands for.
pub fn secret_ref(v: &Value) -> Option<&str> {
    v.as_str()?.strip_prefix("$$").filter(|n| !n.is_empty())
}

/// `"$name"`: the state value it stands for.
pub fn state_ref(v: &Value) -> Option<&str> {
    let s = v.as_str()?;
    if s.starts_with("$$") {
        return None;
    }
    s.strip_prefix('$').filter(|n| !n.is_empty())
}
