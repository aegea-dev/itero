//! A workflow file in the simple form (`docs/design-simple.md`): `on`,
//! `steps`, and the `success`, `failure` and `error` lists run at the end.
//! Its name is its file name, so the file has none.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    /// What it is: its entry in itero.config.json, not its file.
    pub meta: Meta,
    /// Its entry's settings: a run's state starts with them.
    pub settings: Map<String, Value>,
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

/// The names itero gives and nobody sets: in a workflow's entry they are
/// an error.
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

/// A workflow's entry in itero.config.json: what it is, and its settings.
pub fn entry(v: &Value) -> Result<(Meta, Map<String, Value>), String> {
    let Value::Object(m) = v else {
        return Err("an entry is an object: { \"description\": \"...\" }".into());
    };
    let flag = |k: &str, v: &Value| v.as_bool().ok_or(format!("{k} is true or false"));
    let mut out = Meta::default();
    let mut settings = Map::new();
    for (k, v) in m {
        given_by_itero(k)?;
        match k.as_str() {
            "description" => {
                out.description = Some(v.as_str().ok_or("description is text")?.into())
            }
            "settings" => match v {
                Value::Object(s) => settings = s.clone(),
                _ => return Err("settings are an object: { \"name\": value }".into()),
            },
            "from" => out.from = Some(moment(k, v, false)?),
            "until" => out.until = Some(moment(k, v, true)?),
            "off" => out.off = flag(k, v)?,
            "internal" => out.internal = flag(k, v)?,
            _ => {
                out.other.insert(k.clone(), v.clone());
            }
        }
    }
    Ok((out, settings))
}

/// A date or an RFC 3339 time. A date is a whole day: `from` it starts at
/// its first second, `until` it ends after its last.
fn moment(key: &str, v: &Value, end: bool) -> Result<DateTime<Utc>, String> {
    let wrong = || format!("{key}: a date (2026-11-01) or a time (2026-11-01T09:00:00Z)");
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
    /// An action or a workflow, with its args, when it runs, and what
    /// takes it back when the run breaks after it finished.
    One {
        name: Name,
        /// An object, or a `"$name"` that is one when the step starts.
        args: Value,
        on: Option<Box<On>>,
        undo: Option<Name>,
        /// Its args reach its action only, not the run's state.
        strict: bool,
    },
    /// Actions and workflows that run side by side; the next step waits as
    /// the group says.
    Side(Group),
}

/// Steps side by side, how the next step waits for them, and when the
/// group breaks. A bare list is `Wait::All` with no `fail`.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub members: Vec<Member>,
    pub wait: Wait,
    /// `None`: the wait's own rule says when the group breaks.
    pub fail: Option<Fail>,
}

/// A step of a group: an action or a workflow, its args and when it runs,
/// and the label the group knows it by.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub name: Name,
    /// As a step's: an object, a list of objects, or a `"$name"`.
    pub args: Value,
    pub on: Option<Box<On>>,
    pub strict: bool,
    /// `#name` for an action's step, the name for a workflow's, as
    /// [`Name::label`]; with `as`, that name the same way: `#mirror-1`.
    pub label: String,
}

impl Member {
    /// A step of a group written as its name alone.
    pub fn bare(name: Name) -> Member {
        Member {
            label: name.label(),
            name,
            args: Value::Object(Map::new()),
            on: None,
            strict: false,
        }
    }
}

/// When the group ends: the key of its object.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    /// Every step ended well.
    #[default]
    All,
    /// The first step ended.
    Race,
    /// The first step ended well.
    Any,
    /// Some of the steps ended well.
    Some(Need),
    /// Every step ended.
    Settled,
}

/// What `some` needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// This many steps ended well.
    Count(usize),
    /// These steps ended well, as the labels `#a` and `workflow`.
    Names(Vec<String>),
}

/// When the group breaks, replacing its wait's own rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fail {
    /// One step broke.
    Any,
    /// Every step broke.
    All,
    Never,
    /// One of these steps broke, as the labels `#a` and `workflow`.
    Steps(Vec<String>),
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

impl Name {
    /// `#name` for an action, whatever its version, the name for a
    /// workflow: how a step is told apart in a group.
    pub fn label(&self) -> String {
        match self {
            Name::Action { name, .. } => format!("#{name}"),
            Name::Workflow(w) => w.clone(),
        }
    }
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
            // what a workflow is lives in itero.config.json; a version
            // pinned before (and a built-in one) still carries a meta, read
            // there, not here
            "meta" => {}
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
        None => lookup(state, h.trim_start_matches('$')).is_some(),
    }) && on.equal.iter().all(|(k, v)| lookup(state, k) == Some(v))
}

/// A state value by name, or one field of it: `output.count`.
pub fn lookup<'s>(state: &'s Map<String, Value>, name: &str) -> Option<&'s Value> {
    if let Some(v) = state.get(name) {
        return Some(v);
    }
    let (n, field) = name.split_once('.')?;
    state.get(n)?.as_object()?.get(field)
}

/// A value of args filled from the state. A whole `"$name"` or
/// `"$name.field"` is the state's value, with its type, null when it has
/// none; a whole secret, `"$$NAME"`, stays as written, for the action
/// alone. Inside text each name is filled ([`pieces`]); texts in lists and
/// objects are filled too. A secret inside text is refused, and so is a
/// state value holding one.
pub fn fill(v: &Value, state: &Map<String, Value>) -> Result<Value, String> {
    match v {
        Value::String(_) if secret_ref(v).is_some() => Ok(v.clone()),
        Value::String(text) => match state_ref(v) {
            // "$file.txt", file not an object: the dot is text
            Some(n)
                if !state.contains_key(n)
                    && n.split_once('.').is_some_and(|(base, _)| {
                        state
                            .get(base)
                            .is_some_and(|b| !b.is_object() && !b.is_null())
                    }) =>
            {
                fill_text(text, state).map(Value::String)
            }
            Some(n) => Ok(lookup(state, n).cloned().unwrap_or(Value::Null)),
            None => fill_text(text, state).map(Value::String),
        },
        Value::Array(items) => items
            .iter()
            .map(|i| fill(i, state))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Value::Object(o) => o
            .iter()
            .map(|(k, i)| Ok((k.clone(), fill(i, state)?)))
            .collect::<Result<_, String>>()
            .map(Value::Object),
        other => Ok(other.clone()),
    }
}

/// The state names a value of args uses, whole or inside its texts, at
/// any depth.
pub fn names_in(v: &Value) -> Vec<String> {
    match v {
        Value::String(_) if secret_ref(v).is_some() => vec![],
        Value::String(_) if state_ref(v).is_some() => {
            state_ref(v).map(str::to_string).into_iter().collect()
        }
        Value::String(text) => pieces(text)
            .into_iter()
            .filter_map(|p| match p {
                Piece::Name { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(names_in).collect(),
        Value::Object(o) => o.values().flat_map(names_in).collect(),
        _ => vec![],
    }
}

/// A piece of a text with names in it.
#[derive(Debug, PartialEq)]
pub enum Piece<'t> {
    Text(String),
    /// `$name`, or `$name.field`: a field when the value is an object that
    /// has it, else the dot and the rest are text.
    Name {
        name: &'t str,
        field: Option<&'t str>,
    },
    /// `$$NAME`, which no text may hold.
    Secret(&'t str),
}

/// A text cut into its pieces: `\$` is a dollar sign, and a `$` that
/// starts no name is itself.
pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let mut plain = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['\\', '$']) {
        plain.push_str(&rest[..i]);
        let at = &rest[i..];
        if let Some(after) = at.strip_prefix("\\$") {
            plain.push('$');
            rest = after;
            continue;
        }
        if let Some(after) = at.strip_prefix('\\') {
            plain.push('\\');
            rest = after;
            continue;
        }
        let after = &at[1..];
        let secret = after.strip_prefix('$').map(|s| (s, name_len(s)));
        if let Some((s, n)) = secret
            && n > 0
        {
            out.push(Piece::Text(std::mem::take(&mut plain)));
            out.push(Piece::Secret(&s[..n]));
            rest = &s[n..];
            continue;
        }
        let n = name_len(after);
        if n == 0 {
            plain.push('$');
            rest = after;
            continue;
        }
        let (name, mut tail) = after.split_at(n);
        let mut field = None;
        if let Some(f) = tail.strip_prefix('.')
            && field_len(f) > 0
        {
            field = Some(&f[..field_len(f)]);
            tail = &f[field_len(f)..];
        }
        out.push(Piece::Text(std::mem::take(&mut plain)));
        out.push(Piece::Name { name, field });
        rest = tail;
    }
    plain.push_str(rest);
    out.push(Piece::Text(plain));
    out.retain(|p| *p != Piece::Text(String::new()));
    out
}

fn fill_text(text: &str, state: &Map<String, Value>) -> Result<String, String> {
    let refused = |what: String| format!("a secret is a whole value only: {what} inside text");
    let mut out = String::new();
    for p in pieces(text) {
        match p {
            Piece::Text(t) => out.push_str(&t),
            Piece::Secret(n) => return Err(refused(format!("$${n}"))),
            Piece::Name { name, field } => {
                let whole = state.get(name);
                let (v, rest) = match field {
                    Some(f) => match whole.and_then(Value::as_object).and_then(|o| o.get(f)) {
                        Some(v) => (Some(v), String::new()),
                        None => (whole, format!(".{f}")),
                    },
                    None => (whole, String::new()),
                };
                match v {
                    None | Some(Value::Null) => {}
                    Some(v) if secret_ref(v).is_some() => {
                        return Err(refused(format!(
                            "${name}, which holds {}",
                            v.as_str().unwrap_or("")
                        )));
                    }
                    Some(Value::String(t)) => out.push_str(t),
                    Some(other) => out.push_str(&other.to_string()),
                }
                out.push_str(&rest);
            }
        }
    }
    Ok(out)
}

/// The length of the name `s` starts with: a letter or `_`, then letters,
/// digits or `_`; 0 when it starts with none.
fn name_len(s: &str) -> usize {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_alphabetic() || c == '_' => {}
        _ => return 0,
    }
    chars
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(s.len(), |(i, _)| i)
}

/// The length of the field `s` starts with: a name, or `#` and an action's
/// name (letters, digits, `_` or `-`), which is how steps side by side
/// keep a result that is no object: `$output.#size`. 0 when it starts with
/// neither.
fn field_len(s: &str) -> usize {
    let Some(rest) = s.strip_prefix('#') else {
        return name_len(s);
    };
    let n = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    if n == 0 { 0 } else { 1 + n }
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
            args: Value::Object(Map::new()),
            on: None,
            undo: None,
            strict: false,
        }),
        Value::Array(items) => Ok(Step::Side(Group {
            members: side_members(items, "a list side by side")?,
            wait: Wait::All,
            fail: None,
        })),
        Value::Object(m) if is_group(m) => group(v, m),
        Value::Object(m) => {
            let w = written(m)?;
            if w.as_name.is_some() {
                return Err(format!(
                    "{}: as names a step side by side, in a group: {{ \"any\": [{{ \"#ask\": {{ \"as\": \"ask-1\" }} }}, ...] }}",
                    w.key
                ));
            }
            Ok(Step::One {
                name: w.name,
                args: w.args,
                on: w.on.map(Box::new),
                undo: w.undo,
                strict: w.strict,
            })
        }
        _ => Err("a step is a name, a list side by side, or { name: { data } }".into()),
    }
}

/// A step's args as written, checked: an object, a list of objects, or a
/// `"$name"` that is one (`"$output"`). An item of a list is direct, each
/// key an arg given as it is, or a pair `{"title": name, "text": text}`,
/// the arg `name` formatted as text. A name may be given once; `output` is
/// itero's.
pub(crate) fn read_args(a: &Value) -> Result<Value, String> {
    let given = |name: &str, seen: &mut Vec<String>| -> Result<(), String> {
        if name == "output" {
            return Err("args: output is given by itero, the step before's result".into());
        }
        if seen.iter().any(|s| s == name) {
            return Err(format!("args: {name} is given twice"));
        }
        seen.push(name.to_string());
        Ok(())
    };
    // a secret inside text: with no state, only that is refused
    let no_secret = |v: &Value| {
        fill(v, &Map::new())
            .map(drop)
            .map_err(|why| format!("args: {why}"))
    };
    match a {
        Value::Object(o) => {
            let mut seen = vec![];
            for (k, v) in o {
                given(k, &mut seen)?;
                no_secret(v)?;
            }
        }
        Value::Array(items) => {
            let mut seen = vec![];
            for item in items {
                let Value::Object(o) = item else {
                    return Err(format!(
                        "args: each item of the list is an object, like {{\"url\": \"$url1\"}}, not {item}"
                    ));
                };
                match pair(o) {
                    Some((title, text)) => {
                        let Some(name) = title.as_str().filter(|n| !n.is_empty()) else {
                            return Err(format!("args: the title of a pair names an arg: {item}"));
                        };
                        let Value::String(_) = text else {
                            return Err(format!("args: the text of a pair is text: {item}"));
                        };
                        if secret_ref(text).is_some() {
                            return Err(format!(
                                "args: a secret goes as a direct arg, {{\"{name}\": {text}}}, not formatted as text"
                            ));
                        }
                        given(name, &mut seen)?;
                        no_secret(text)?;
                    }
                    None => {
                        for (k, v) in o {
                            given(k, &mut seen)?;
                            no_secret(v)?;
                        }
                    }
                }
            }
        }
        a if state_ref(a).is_some() => {}
        _ => {
            return Err(
                "args is an object, a list of objects, or a \"$name\" that is one, like \"$output\""
                    .into(),
            );
        }
    }
    Ok(a.clone())
}

/// An item of a list of args that is a pair, `{"title": name, "text": text}`:
/// its two values.
fn pair(item: &Map<String, Value>) -> Option<(&Value, &Value)> {
    if item.len() != 2 {
        return None;
    }
    Some((item.get("title")?, item.get("text")?))
}

/// A step's args filled from the state, as one object. An object's values
/// and a list's direct items are filled as [`fill`] does: a whole `"$name"`
/// keeps its type. A pair's text is always text. A whole `"$name"` is the
/// state value, which must be an object.
pub fn fill_args(args: &Value, state: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    match args {
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| Ok((k.clone(), fill(v, state)?)))
            .collect(),
        Value::Array(items) => {
            let mut out = Map::new();
            for item in items {
                let Value::Object(o) = item else {
                    return Err(format!(
                        "args: each item of the list is an object, not {item}"
                    ));
                };
                match pair(o) {
                    Some((Value::String(name), Value::String(text))) => {
                        out.insert(name.clone(), Value::String(fill_text(text, state)?));
                    }
                    _ => {
                        for (k, v) in o {
                            out.insert(k.clone(), fill(v, state)?);
                        }
                    }
                }
            }
            Ok(out)
        }
        whole => match fill(whole, state)? {
            Value::Object(o) => Ok(o),
            other => Err(format!("args {whole} is not an object: {}", brief(&other))),
        },
    }
}

/// A value, shortened for a message.
fn brief(v: &Value) -> String {
    let t = v.to_string();
    match t.char_indices().nth(60) {
        Some((i, _)) => format!("{}...", &t[..i]),
        None => t,
    }
}

/// A step written as an object, `{ "#ship": { "args": ..., "on": ... } }`,
/// read: what a step of a list and a step of a group share.
struct Written {
    /// As written, for the errors: `#ship`.
    key: String,
    name: Name,
    args: Value,
    on: Option<On>,
    undo: Option<Name>,
    strict: bool,
    /// Its `as`, the name a group knows it by.
    as_name: Option<String>,
}

fn written(m: &Map<String, Value>) -> Result<Written, String> {
    let mut it = m.iter();
    let (Some((k, v)), None) = (it.next(), it.next()) else {
        return Err("a step object has one name: { \"#ship\": { ... } }".into());
    };
    let Value::Object(fields) = v else {
        return Err(format!(
            "{k}: a step is an object: {{ \"args\": {{ ... }} }}"
        ));
    };
    let mut fields = fields.clone();
    let when = fields.remove("on").map(|w| on(&w)).transpose()?;
    if let Some(w) = &when {
        let starts = [
            (!w.events.is_empty(), "an event"),
            (!w.schedules.is_empty(), "a schedule"),
            (!w.paths.is_empty(), "a path"),
        ];
        if let Some((_, what)) = starts.iter().find(|(given, _)| *given) {
            return Err(format!(
                "{k}: a step's on holds conditions ({{\"$name\": value}}, {{\"has\": ...}}), not {what}: an event, a schedule or a path starts a workflow"
            ));
        }
    }
    let undo = fields
        .remove("undo")
        .map(|u| match u.as_str() {
            Some(s) => name(s).map_err(|e| format!("{k}: undo: {e}")),
            None => Err(format!(
                "{k}: undo is an action or a workflow, like \"#cluster-delete\""
            )),
        })
        .transpose()?;
    let args = match fields.remove("args") {
        None => Value::Object(Map::new()),
        Some(a) => read_args(&a).map_err(|why| format!("{k}: {why}"))?,
    };
    let strict = match fields.remove("strict") {
        None => false,
        Some(Value::Bool(b)) => b,
        Some(_) => return Err(format!("{k}: strict is true or false")),
    };
    let as_name = match fields.remove("as") {
        None => None,
        Some(Value::String(a)) if plain(&a) => Some(a),
        Some(a) => {
            return Err(format!(
                "{k}: as names the step in its group, without # or spaces, like \"mirror-1\", not {a}"
            ));
        }
    };
    if let Some(other) = fields.keys().next() {
        return Err(format!(
            "{k}: \"{other}\" is not one of args, on, undo, strict, as: a step's values go under args"
        ));
    }
    let name = name(k)?;
    if strict && matches!(name, Name::Workflow(_)) {
        return Err(format!("{k}: strict is for an action's step"));
    }
    Ok(Written {
        key: k.clone(),
        name,
        args,
        on: when,
        undo,
        strict,
        as_name,
    })
}

/// The steps of a list side by side, each a name or a step object; `what`
/// says whose list it is.
fn side_members(items: &[Value], what: &str) -> Result<Vec<Member>, String> {
    items
        .iter()
        .map(|i| match i {
            Value::String(s) => name(s).map(Member::bare),
            Value::Object(m) if !is_group(m) => {
                let w = written(m)?;
                if w.undo.is_some() {
                    return Err(format!(
                        "{}: undo is for a step of a list, not one side by side",
                        w.key
                    ));
                }
                let label = match (&w.as_name, &w.name) {
                    (Some(a), Name::Action { .. }) => format!("#{a}"),
                    (Some(a), Name::Workflow(_)) => a.clone(),
                    (None, n) => n.label(),
                };
                Ok(Member {
                    name: w.name,
                    args: w.args,
                    on: w.on.map(Box::new),
                    strict: w.strict,
                    label,
                })
            }
            Value::Object(_) => Err(format!(
                "a group inside a group goes in a workflow of its own, used here by its name: {i}"
            )),
            _ => Err(format!(
                "a step of {what} is a name or an object, like {{ \"#ask\": {{ \"args\": ... }} }}, not {i}"
            )),
        })
        .collect()
}

const WAITS: [&str; 5] = ["all", "race", "any", "some", "settled"];

/// Whether a step object is a group: it holds one of the group's keys,
/// unless it is one step named like a key (`{ "all": { ... } }`, a
/// workflow called `all`).
fn is_group(m: &Map<String, Value>) -> bool {
    let one_step = m.len() == 1 && m.values().all(Value::is_object);
    !one_step
        && m.keys().any(|k| {
            WAITS.contains(&k.as_str()) || ["fail", "count", "names"].contains(&k.as_str())
        })
}

/// `{ "race": ["#a", "#b"], "fail": ... }`: steps side by side that say
/// how to wait. Every error shows the step as written.
fn group(v: &Value, m: &Map<String, Value>) -> Result<Step, String> {
    let bad = |why: String| format!("{v}: {why}");
    let keys: Vec<&str> = m
        .keys()
        .map(String::as_str)
        .filter(|k| WAITS.contains(k))
        .collect();
    let key = match keys.as_slice() {
        [one] => *one,
        [] => {
            return Err(bad(
                "a group holds one of all, race, any, some or settled, with a list of steps".into(),
            ));
        }
        several => {
            return Err(bad(format!(
                "a group holds one of all, race, any, some or settled, not both {}",
                several.join(" and ")
            )));
        }
    };
    if let Some(other) = m
        .keys()
        .find(|k| !WAITS.contains(&k.as_str()) && !["fail", "count", "names"].contains(&k.as_str()))
    {
        return Err(bad(format!(
            "\"{other}\" is not one of {key}, fail, count, names"
        )));
    }
    let items = match &m[key] {
        Value::Array(items) if items.len() >= 2 => items,
        _ => {
            return Err(bad(format!(
                "{key} is a list of two or more steps, like [\"#a\", \"#b\"]"
            )));
        }
    };
    let members = side_members(items, key).map_err(&bad)?;
    let labels: Vec<String> = members.iter().map(|m| m.label.clone()).collect();
    // a step is told apart by its label, whatever its version
    for (i, l) in labels.iter().enumerate() {
        if labels[..i].contains(l) {
            return Err(bad(format!(
                "{l} is in the list twice: give each \"as\", a name of its own, to tell them apart"
            )));
        }
    }
    // a list of names that must be steps of this group
    let steps_of = |what: &str, v: &Value| -> Result<Vec<String>, String> {
        let list: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            one => vec![one],
        };
        let mut out = vec![];
        for n in list {
            let s = n.as_str().unwrap_or("");
            if !labels.iter().any(|l| l == s) {
                return Err(bad(format!(
                    "{what}: {n} is not one of the steps in the list"
                )));
            }
            out.push(s.to_string());
        }
        Ok(out)
    };
    let wait = match key {
        "all" => Wait::All,
        "race" => Wait::Race,
        "any" => Wait::Any,
        "settled" => Wait::Settled,
        _ => match (m.get("count"), m.get("names")) {
            (None, None) => return Err(bad("some needs count or names".into())),
            (Some(_), Some(_)) => return Err(bad("some takes count or names, not both".into())),
            (Some(c), None) => match c.as_u64() {
                Some(n) if (1..=members.len() as u64).contains(&n) => {
                    Wait::Some(Need::Count(n as usize))
                }
                _ => {
                    return Err(bad(format!(
                        "count is a whole number from 1 to {}, the steps in the list",
                        members.len()
                    )));
                }
            },
            (None, Some(n)) => match n {
                Value::Array(a) if !a.is_empty() => Wait::Some(Need::Names(steps_of("names", n)?)),
                _ => {
                    return Err(bad(
                        "names is a list of the steps that must end well, like [\"#a\", \"#c\"]"
                            .into(),
                    ));
                }
            },
        },
    };
    for k in ["count", "names"] {
        if key != "some" && m.contains_key(k) {
            return Err(bad(format!("{k} goes with some")));
        }
    }
    let fail = match m.get("fail") {
        None => None,
        Some(Value::String(s)) if s == "any" => Some(Fail::Any),
        Some(Value::String(s)) if s == "all" => Some(Fail::All),
        Some(Value::String(s)) if s == "never" => Some(Fail::Never),
        Some(f @ (Value::String(_) | Value::Array(_)))
            if f.as_array().is_none_or(|a| !a.is_empty()) =>
        {
            Some(Fail::Steps(steps_of("fail", f)?))
        }
        Some(_) => {
            return Err(bad(
                "fail is \"any\", \"all\", \"never\", a step's name or a list of names".into(),
            ));
        }
    };
    Ok(Step::Side(Group {
        members,
        wait,
        fail,
    }))
}

/// `#name` or `#name@1.2` is an action, any other name a workflow.
/// A name as an action's or a workflow's is written, without its `#` or
/// version: no `/`, `#`, `@`, `$` or space.
fn plain(n: &str) -> bool {
    !n.is_empty() && !n.contains(['/', '#', '@', '$']) && !n.contains(char::is_whitespace)
}

pub fn name(s: &str) -> Result<Name, String> {
    let ok = plain;
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
                Step::One { name, undo, .. } => std::iter::once(name).chain(undo).collect(),
                Step::Side(g) => g.members.iter().map(|m| &m.name).collect(),
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
            match s {
                Step::One { on: Some(on), .. } => out.extend(names(on)),
                Step::Side(g) => {
                    for on in g.members.iter().filter_map(|m| m.on.as_ref()) {
                        out.extend(names(on));
                    }
                }
                Step::One { .. } => {}
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The names of the args its steps give the action `action` (without
    /// `#`), in a list or side by side, each once, sorted.
    pub fn arg_names(&self, action: &str) -> Vec<String> {
        let is = |n: &Name| matches!(n, Name::Action { name, .. } if name == action);
        let mut out = vec![];
        for s in self.all_steps() {
            match s {
                Step::One { name, args, .. } if is(name) => out.extend(arg_names_of(args)),
                Step::Side(g) => {
                    for m in g.members.iter().filter(|m| is(&m.name)) {
                        out.extend(arg_names_of(&m.args));
                    }
                }
                Step::One { .. } => {}
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
            let args: Vec<&Value> = match s {
                Step::One { args, .. } => vec![args],
                Step::Side(g) => g.members.iter().map(|m| &m.args).collect(),
            };
            for a in args {
                out.extend(direct_values(a).filter_map(secret_ref).map(str::to_string));
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// The names of args as written: an object's keys, a list's direct items'
/// keys and its pairs' titles; a whole `"$name"` names none.
fn arg_names_of(args: &Value) -> Vec<String> {
    match args {
        Value::Object(o) => o.keys().cloned().collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_object)
            .flat_map(|o| match pair(o) {
                Some((title, _)) => title.as_str().map(str::to_string).into_iter().collect(),
                None => o.keys().cloned().collect::<Vec<_>>(),
            })
            .collect(),
        _ => vec![],
    }
}

/// The values of args given as they are: an object's, and a list's direct
/// items' (a pair's text is formatted, never a secret).
fn direct_values(args: &Value) -> Box<dyn Iterator<Item = &Value> + '_> {
    match args {
        Value::Object(o) => Box::new(o.values()),
        Value::Array(items) => Box::new(
            items
                .iter()
                .filter_map(Value::as_object)
                .filter(|o| pair(o).is_none())
                .flat_map(|o| o.values()),
        ),
        _ => Box::new(std::iter::empty()),
    }
}

/// `"$$NAME"`, the whole value: the name of the environment variable or
/// secret it stands for.
pub fn secret_ref(v: &Value) -> Option<&str> {
    let n = v.as_str()?.strip_prefix("$$")?;
    (!n.is_empty() && name_len(n) == n.len()).then_some(n)
}

/// `"$name"` or `"$name.field"`, the whole value: the state value it
/// stands for.
pub fn state_ref(v: &Value) -> Option<&str> {
    let s = v.as_str()?.strip_prefix('$')?;
    let n = name_len(s);
    if n == 0 {
        return None;
    }
    match &s[n..] {
        "" => Some(s),
        dot => {
            let f = dot.strip_prefix('.')?;
            (!f.is_empty() && field_len(f) == f.len()).then_some(s)
        }
    }
}
