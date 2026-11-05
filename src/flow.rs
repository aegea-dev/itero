//! A workflow file in the simple form: `on`,
//! `steps`, and the `success`, `failure` and `error` lists run at the end.
//! Its name is its file name, so the file has none.
use crate::problem::Problem;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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
    /// Its runs go on as rounds: when one ends, the next starts with its
    /// results.
    pub repeat: Option<Repeat>,
    /// What it is: its entry in itero.config.json, not its file.
    pub meta: Meta,
}

/// How long a step or a workflow goes on in rounds, each started as soon
/// as the one before ended, with its results. A round that ends in an
/// error or a failure stops it, and so does the workflow being turned off
/// or being out of its `from`/`until`.
#[derive(Debug, Clone, PartialEq)]
pub enum Repeat {
    /// `true`: until one of those stops it.
    Always,
    /// `3`: that many rounds.
    Times(u64),
    /// `"$output.count"`: that many, read as the first round starts (the
    /// value's path, `output.count`).
    TimesOf(String),
    /// `"<= 2026-12-31"`: while it is still before this, the day's end.
    Until(DateTime<Utc>),
    /// `"> 2026-12-31"`: while it is past this, the day's end.
    After(DateTime<Utc>),
    /// A cron line: a round at each time it names (a step's only).
    Cron(String),
}

/// A `repeat` as written, on a step or (`workflow`) on a workflow.
fn repeat(v: &Value, workflow: bool) -> Result<Repeat, String> {
    const FORMS: &str = "true, a number of rounds, \"$output.count\", \"<= 2026-12-31\", \"> 2026-12-31\" or a cron line";
    match v {
        Value::Bool(true) => Ok(Repeat::Always),
        Value::Number(n) => match n.as_u64() {
            Some(n) if n > 0 => Ok(Repeat::Times(n)),
            _ => Err(format!("repeat {v}: a number of rounds is 1 or more")),
        },
        Value::String(s) => {
            let s = s.trim();
            if let Some(d) = s.strip_prefix("<=") {
                return moment("repeat", &Value::String(d.trim().into()), true).map(Repeat::Until);
            }
            if let Some(d) = s.strip_prefix('>') {
                return moment("repeat", &Value::String(d.trim().into()), true).map(Repeat::After);
            }
            if let Some(p) = s.strip_prefix('$').filter(|p| !p.starts_with('$')) {
                return value_path(p, s)
                    .map(Repeat::TimesOf)
                    .map_err(|e| e.replacen("on:", "repeat:", 1));
            }
            if crate::engine::schedule::check(s).is_ok() {
                if workflow {
                    return Err(format!(
                        "repeat \"{s}\": a workflow's schedule is in on ({{\"schedule\": \"{s}\"}}); its repeat is {FORMS}"
                    ));
                }
                return Ok(Repeat::Cron(s.into()));
            }
            Err(format!("repeat \"{s}\" is not one of {FORMS}"))
        }
        _ => Err(format!("repeat {v} is not one of {FORMS}")),
    }
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

/// A workflow's entry in itero.config.json: what it is. The manifest holds
/// no values to work on.
pub fn entry(v: &Value) -> Result<Meta, String> {
    let Value::Object(m) = v else {
        return Err("an entry is an object: { \"description\": \"...\" }".into());
    };
    let flag = |k: &str, v: &Value| v.as_bool().ok_or(format!("{k} is true or false"));
    let mut out = Meta::default();
    for (k, v) in m {
        given_by_itero(k)?;
        match k.as_str() {
            "description" => {
                out.description = Some(v.as_str().ok_or("description is text")?.into())
            }
            "settings" => {
                return Err(
                    "settings are gone: itero.config.json holds no values; give them in .env ($$NAME), in a step's args, or in the event"
                        .into(),
                );
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
    /// `{"event": "order.placed"}`: message types, each with the args its
    /// connector is given (`"args": {...}`), none when it has none.
    pub events: Vec<Listen>,
    /// `{"schedule": "0 9 * * *"}`: cron lines.
    pub schedules: Vec<String>,
    /// `{"path": "/var/mail/$$USER"}`: files or folders, as written.
    pub paths: Vec<String>,
    /// `{"has": ...}`: `$event.order` must have a value, `$$API` be
    /// defined (a secret, `.env` or the environment); kept as written.
    pub has: Vec<String>,
    /// `{"$event.paid": true}`: values that must be exactly these, by path
    /// (`event.paid`).
    pub equal: Map<String, Value>,
}

/// An event `on` listens for, and the args the connector that sends it is
/// given: one copy of a connector runs for each different set.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Listen {
    #[serde(rename = "event")]
    pub kind: String,
    /// As written: values, or `$$NAME`s looked up when the connector starts.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub args: Map<String, Value>,
}

impl Listen {
    /// An event with no args.
    pub fn of(kind: &str) -> Listen {
        Listen {
            kind: kind.into(),
            args: Map::new(),
        }
    }

    /// Whether an event of `kind` from `source` is for this entry: one a
    /// connector's copy sent (`connector/<type>?<args>`) only when the copy
    /// has this entry's args; one no copy sent (`itero ws emit`, the API)
    /// whatever the args.
    pub fn takes(&self, kind: &str, source: &str) -> bool {
        if self.kind != kind {
            return false;
        }
        match source.strip_prefix("connector/") {
            Some(copy) => copy.split_once('?').map_or("", |(_, key)| key) == self.key(),
            None => true,
        }
    }

    /// The args as written, as JSON with its keys sorted, `""` for none:
    /// what tells one copy of a connector from another.
    pub fn key(&self) -> String {
        if self.args.is_empty() {
            return String::new();
        }
        let sorted: BTreeMap<&String, &Value> = self.args.iter().collect();
        serde_json::to_string(&sorted).unwrap_or_default()
    }
}

/// A step is read once, when its workflow is: its size costs nothing, and
/// boxing its fields would cost an allocation each for no gain.
#[allow(clippy::large_enum_variant)]
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
        /// The undo's args, when it is written as a step: filled when it
        /// runs, `$output` being the result of the step it takes back.
        undo_args: Value,
        /// How the workflow knows it: `#ask-mirror`, `#ask-mirror/2` when its
        /// name is in the file more than once (counted over the whole
        /// file), a workflow's name.
        label: String,
        /// It runs in rounds, each with the one before's results.
        repeat: Option<Repeat>,
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

/// Reads a workflow already read as JSON: what the manager is given. Its
/// error tells every problem, each with its place in the workflow.
pub fn from_value(v: &Value) -> Result<Flow, String> {
    read(v).map_err(|ps| told(&ps))
}

/// Problems as one text: `message (address)`, joined by `; `.
pub fn told(ps: &[Problem]) -> String {
    ps.iter()
        .map(|p| match p.at.as_str() {
            "" => p.message.clone(),
            at => format!("{} ({at})", p.message),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A workflow read as JSON, or every problem it has, each at its address
/// in the workflow: `on/2`, `steps/#ai`, `""` for the whole.
pub fn read(v: &Value) -> Result<Flow, Vec<Problem>> {
    let Value::Object(top) = v else {
        return Err(vec![Problem::new(
            "",
            "a workflow is an object: { \"on\": [...], \"steps\": [...] }",
        )]);
    };
    let mut f = Flow::default();
    let mut ps = vec![];
    let list = |key: &str, v: &Value, ps: &mut Vec<Problem>| -> Vec<Step> {
        steps(key, v).unwrap_or_else(|e| {
            ps.extend(e.into_iter().map(|p| p.under(key)));
            vec![]
        })
    };
    for (k, v) in top {
        match k.as_str() {
            // what a workflow is lives in itero.config.json; a version
            // pinned before (and a built-in one) still carries a meta, read
            // there, not here
            "meta" => {}
            "on" => match on(v) {
                Ok(o) => f.on = o,
                Err(e) => ps.extend(e.into_iter().map(|p| p.under("on"))),
            },
            "steps" => f.steps = list(k, v, &mut ps),
            "success" => f.success = list(k, v, &mut ps),
            "failure" => f.failure = list(k, v, &mut ps),
            "error" => f.error = list(k, v, &mut ps),
            "timeout" => match timeout(v) {
                Ok(t) => f.timeout = Some(t),
                Err(e) => ps.push(Problem::new("timeout", e)),
            },
            "repeat" => match repeat(v, true) {
                Ok(r) => f.repeat = Some(r),
                Err(e) => ps.push(Problem::new("repeat", e)),
            },
            other => ps.push(Problem::new(
                "",
                format!(
                    "unknown key \"{other}\": a workflow has meta, on, steps, success, failure, error, timeout and repeat"
                ),
            )),
        }
    }
    if !top.contains_key("steps") {
        ps.push(Problem::new("", "a workflow needs steps"));
    } else if f.steps.is_empty() && !ps.iter().any(|p| p.at.starts_with("steps")) {
        ps.push(Problem::new("steps", "steps is empty"));
    }
    // the names its steps read, once its steps are all there: a step that
    // could not be read would make every name it gives look missing
    if ps.is_empty() {
        ps = f.number();
    }
    if ps.is_empty() {
        ps = f.check_names();
    }
    if ps.is_empty() { Ok(f) } else { Err(ps) }
}

fn timeout(v: &Value) -> Result<Duration, String> {
    let t = v.as_str().ok_or("timeout is a duration, like \"2h\"")?;
    humantime::parse_duration(t)
        .map_err(|_| format!("timeout: a duration is like 30m, 2h or 1d, not \"{t}\""))
}

/// The name a step's result is kept by, `$steps.<key>`: its `as`, else the
/// action's name without `#`, or the workflow's.
pub fn step_key(label: &str) -> String {
    label.trim_start_matches('#').to_string()
}

/// Where a value comes from: `$event` (the event that started the run),
/// `$output` (the step before), `$steps.<name>` (any step by its name),
/// `$config` (the manifest).
pub const ROOTS: [&str; 5] = ["event", "output", "steps", "config", "count"];

/// Whether `on`'s conditions hold over a run's state, `defined` telling
/// which environment variables and secrets are. Its messages and
/// schedules are not conditions: they say what starts a run.
pub fn holds(on: &On, state: &Map<String, Value>, defined: &dyn Fn(&str) -> bool) -> bool {
    on.has.iter().all(|h| match h.strip_prefix("$$") {
        Some(n) => defined(n),
        None => lookup(state, h.trim_start_matches('$')).is_some_and(|v| !v.is_null()),
    }) && on.equal.iter().all(|(k, v)| lookup(state, k) == Some(v))
}

/// A field by the name a path writes: that name, else one with a `-` where
/// it writes `_`. A `-` ends a name inside text, and names from outside hold
/// them (a webhook's `x-github-event`): `$event.headers.x_github_event`.
fn field<'o>(o: &'o Map<String, Value>, name: &str) -> Option<&'o Value> {
    o.get(name).or_else(|| {
        if !name.contains('_') {
            return None;
        }
        o.iter()
            .find(|(k, _)| k.contains('-') && k.replace('-', "_") == name)
            .map(|(_, v)| v)
    })
}

/// A value by its path from a root: `event.order`, `output.total`,
/// `steps.fetch-rates.price`; fields at any depth.
pub fn lookup<'s>(state: &'s Map<String, Value>, path: &str) -> Option<&'s Value> {
    let mut parts = path.split('.');
    let mut at = state.get(parts.next()?)?;
    for p in parts {
        at = field(at.as_object()?, p)?;
    }
    Some(at)
}

/// How far a path goes: the value at its end; or, when a field is asked
/// of a text or a number, that value and the fields left (`$event.file.txt`
/// with `file` a text is the file's name and `.txt`); else nothing.
enum Found<'s> {
    Whole(&'s Value),
    Short(&'s Value, String),
    Missing,
}

fn find<'s>(state: &'s Map<String, Value>, path: &str) -> Found<'s> {
    let mut parts = path.split('.').peekable();
    let Some(mut at) = parts.next().and_then(|r| state.get(r)) else {
        return Found::Missing;
    };
    // `steps`' first part is a step's name, never a field of a text
    let steps = path.starts_with("steps.");
    let mut first = true;
    while let Some(p) = parts.next() {
        match at.as_object() {
            Some(o) => match field(o, p) {
                Some(v) => at = v,
                None => return Found::Missing,
            },
            None if !(at.is_null() || steps && first) => {
                let rest: Vec<&str> = std::iter::once(p).chain(parts).collect();
                return Found::Short(at, format!(".{}", rest.join(".")));
            }
            None => return Found::Missing,
        }
        first = false;
    }
    Found::Whole(at)
}

/// A value of args filled from the state. A whole `"$event.count"` is the
/// state's value, with its type, null when it has none; a whole secret,
/// `"$$NAME"`, stays as written, for the action alone ([`written_secrets`]
/// says which args those are: a value that looks like one is only text).
/// Inside text each name is filled ([`pieces`]); texts in lists and objects
/// are filled too. A secret written inside text is refused.
pub fn fill(v: &Value, state: &Map<String, Value>) -> Result<Value, String> {
    match v {
        Value::String(_) if secret_ref(v).is_some() => Ok(v.clone()),
        Value::String(text) => match state_ref(v) {
            Some(path) => match find(state, path) {
                Found::Whole(v) => Ok(v.clone()),
                Found::Short(..) => fill_text(text, state).map(Value::String),
                Found::Missing => Ok(Value::Null),
            },
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

/// The paths a value of args reads (`event.order`, `steps.ask.price`),
/// whole or inside its texts, at any depth.
pub fn names_in(v: &Value) -> Vec<String> {
    match v {
        Value::String(_) if secret_ref(v).is_some() => vec![],
        Value::String(_) if state_ref(v).is_some() => {
            state_ref(v).map(str::to_string).into_iter().collect()
        }
        Value::String(text) => pieces(text)
            .into_iter()
            .filter_map(|p| match p {
                Piece::Name(path) => Some(path.to_string()),
                _ => None,
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(names_in).collect(),
        Value::Object(o) => o.values().flat_map(names_in).collect(),
        _ => vec![],
    }
}

/// The `$word`s a value writes that are no value: not one of [`ROOTS`]
/// (`$order`, `$why`), and not a secret.
pub fn unknown_in(v: &Value) -> Vec<String> {
    match v {
        Value::String(text) => pieces(text)
            .into_iter()
            .filter_map(|p| match p {
                Piece::Unknown(w) => Some(w.to_string()),
                _ => None,
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(unknown_in).collect(),
        Value::Object(o) => o.values().flat_map(unknown_in).collect(),
        _ => vec![],
    }
}

/// What to write instead of a `$word` that is no value.
pub fn instead(word: &str) -> String {
    match word.split('.').next().unwrap_or(word) {
        "why" | "failed_step" | "undo_failed" => format!("$output.{word}"),
        _ if word.starts_with("output.#") => {
            format!("$steps.{}", word.trim_start_matches("output.#"))
        }
        _ => format!("$event.{word}"),
    }
}

/// A piece of a text with values in it.
#[derive(Debug, PartialEq)]
pub enum Piece<'t> {
    Text(String),
    /// `$event.order`, `$output`, `$steps.ask.price`, `$config.name`: the
    /// path, without `$`.
    Name(&'t str),
    /// `$$NAME`, which no text may hold.
    Secret(&'t str),
    /// `$order`, `$output.#size`: a word after `$` that is no value, kept as
    /// text; `itero ws check` names what to write instead.
    Unknown(&'t str),
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
        let n = path_len(after);
        // `$output.#size`, the old form of a step's result, is no value
        let old = n == "output".len()
            && after[n..]
                .strip_prefix(".#")
                .is_some_and(|r| step_len(r) > 0);
        if n > 0 && !old {
            out.push(Piece::Text(std::mem::take(&mut plain)));
            out.push(Piece::Name(&after[..n]));
            rest = &after[n..];
            continue;
        }
        let w = old_len(after);
        if w > 0 {
            plain.push('$');
            plain.push_str(&after[..w]);
            out.push(Piece::Unknown(&after[..w]));
            rest = &after[w..];
            continue;
        }
        plain.push('$');
        rest = after;
    }
    plain.push_str(rest);
    out.push(Piece::Text(plain));
    out.retain(|p| *p != Piece::Text(String::new()));
    // an unknown word is kept in the text too: join the texts around it
    let mut joined: Vec<Piece> = vec![];
    for p in out {
        match (joined.last_mut(), p) {
            (Some(Piece::Text(a)), Piece::Text(b)) => a.push_str(&b),
            (_, p) => joined.push(p),
        }
    }
    joined
}

fn fill_text(text: &str, state: &Map<String, Value>) -> Result<String, String> {
    let mut out = String::new();
    for p in pieces(text) {
        match p {
            Piece::Text(t) => out.push_str(&t),
            Piece::Unknown(_) => {}
            Piece::Secret(n) => {
                return Err(format!("a secret is a whole value only: $${n} inside text"));
            }
            Piece::Name(path) => {
                let (v, rest) = match find(state, path) {
                    Found::Whole(v) => (Some(v), String::new()),
                    Found::Short(v, rest) => (Some(v), rest),
                    Found::Missing => (None, String::new()),
                };
                match v {
                    None | Some(Value::Null) => {}
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

/// The length of a step's name `s` starts with, as `#name` has it:
/// letters, digits, `_` or `-`.
fn step_len(s: &str) -> usize {
    let name = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(s.len());
    // and its number, when its name is in the file more than once: `ayna/2`
    match s[name..].strip_prefix('/') {
        Some(rest) => {
            let digits = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            if digits > 0 { name + 1 + digits } else { name }
        }
        None => name,
    }
}

/// The length of the value's path `s` starts with (after `$`): a root of
/// [`ROOTS`], then `.` and a field (a step's name after `steps`), any
/// number of times; 0 when `s` starts with no root.
fn path_len(s: &str) -> usize {
    let root = name_len(s);
    if root == 0 || !ROOTS.contains(&&s[..root]) {
        return 0;
    }
    let steps = &s[..root] == "steps";
    let mut n = root;
    let mut first = true;
    while let Some(after) = s[n..].strip_prefix('.') {
        let f = if steps && first {
            step_len(after)
        } else {
            name_len(after)
        };
        if f == 0 {
            break;
        }
        n += 1 + f;
        first = false;
    }
    n
}

/// The length of a `$word` that is no value (`order`, `why`,
/// `output.#size`, `input.path`), for `itero ws check` to name.
fn old_len(s: &str) -> usize {
    let n = name_len(s);
    if n == 0 {
        return 0;
    }
    match s[n..].strip_prefix('.') {
        Some(after) => {
            let f = after
                .strip_prefix('#')
                .map(|r| 1 + step_len(r))
                .unwrap_or_else(|| name_len(after));
            if f > 0 { n + 1 + f } else { n }
        }
        None => n,
    }
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
fn on(v: &Value) -> Result<On, Vec<Problem>> {
    let Value::Array(items) = v else {
        return Err(vec![Problem::new(
            "",
            format!("on is a list of objects: {ON_FORMS}"),
        )]);
    };
    let mut o = On::default();
    let mut ps = vec![];
    for (i, item) in items.iter().enumerate() {
        if let Err(es) = on_entry(item, &mut o) {
            ps.extend(es.into_iter().map(|e| Problem::new((i + 1).to_string(), e)));
        }
    }
    if ps.is_empty() { Ok(o) } else { Err(ps) }
}

const ON_FORMS: &str = "{\"event\": ...}, {\"schedule\": ...}, {\"path\": ...}, {\"$event.x\": value} or {\"has\": ...}";

/// One entry of `on`, into `o`: every problem it has.
fn on_entry(item: &Value, o: &mut On) -> Result<(), Vec<String>> {
    let Value::Object(m) = item else {
        return Err(vec![format!(
            "on: {item} is not an object; an entry is {ON_FORMS}"
        )]);
    };
    if let Some(args) = m.get("args") {
        if !m.contains_key("event") {
            return Err(vec![format!(
                "on: {item}: args go with an event: they are its connector's ({{\"event\": \"mail.received\", \"args\": {{...}}}})"
            )]);
        }
        let kind = m["event"].as_str().unwrap_or("");
        if m.len() > 2 {
            return Err(vec![format!(
                "on: {item} says more than one thing: an event (with its args), a schedule or a path is an entry of its own"
            )]);
        }
        if !is_message_type(kind) {
            return Err(vec![format!(
                "on: event {} is not a message type (two or more words joined by .)",
                m["event"]
            )]);
        }
        let args = connector_args(args)?;
        o.events.push(Listen {
            kind: kind.into(),
            args,
        });
        return Ok(());
    }
    on_condition(item, m, o).map_err(|e| vec![e])
}

/// A connector's args in an `on` entry: names an environment variable may
/// have (the connector gets them so), values as written or `$$NAME`; a
/// connector has no run, so no `$name` of one. Every problem, each told.
fn connector_args(v: &Value) -> Result<Map<String, Value>, Vec<String>> {
    let Value::Object(args) = v else {
        return Err(vec![format!(
            "on: args is an object: {{\"NAME\": value}}, not {v}"
        )]);
    };
    let mut ps = vec![];
    for (k, v) in args {
        if !env_name(k) {
            ps.push(format!(
                "on: args \"{k}\" is not a name an environment variable can have"
            ));
        }
        if let Some(n) = names_in(v).first().or(unknown_in(v).first()) {
            ps.push(format!(
                "on: args \"{k}\": a connector has no run, so no ${n}; write the value or a $$NAME"
            ));
        }
    }
    if ps.is_empty() {
        Ok(args.clone())
    } else {
        Err(ps)
    }
}

/// A name an environment variable can have: a letter or `_`, then
/// letters, digits or `_`.
pub fn env_name(n: &str) -> bool {
    let mut cs = n.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// An `on` entry with no args: an event, a schedule, a path, or
/// conditions.
fn on_condition(item: &Value, m: &Map<String, Value>, o: &mut On) -> Result<(), String> {
    {
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
                    o.events.push(Listen::of(s));
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
                        match (s.strip_prefix("$$"), s.strip_prefix('$')) {
                            (Some(b), _) => {
                                word(b, s)?;
                                o.has.push(s.to_string());
                            }
                            (None, Some(b)) => {
                                o.has.push(format!("${}", value_path(b, s)?));
                            }
                            _ => {
                                return Err(format!(
                                    "on: has \"{s}\" is not a value ($event.x) or a $$NAME"
                                ));
                            }
                        }
                    }
                }
                _ => match k.strip_prefix('$') {
                    Some(n) if !n.starts_with('$') => {
                        o.equal.insert(value_path(n, k)?, v.clone());
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
    Ok(())
}

/// A condition's value, `event.kind` for `$event.kind`; a `$word` that is
/// no value says what to write instead.
fn value_path(n: &str, whole: &str) -> Result<String, String> {
    if path_len(n) == n.len() {
        return Ok(n.to_string());
    }
    Err(format!("on: \"{whole}\" is no value: write {}", instead(n)))
}

fn word(n: &str, whole: &str) -> Result<String, String> {
    if n.is_empty() || n.contains(char::is_whitespace) {
        return Err(format!("on: \"{whole}\" names nothing"));
    }
    Ok(n.to_string())
}

fn steps(key: &str, v: &Value) -> Result<Vec<Step>, Vec<Problem>> {
    let Some(items) = v.as_array() else {
        return Err(vec![Problem::new("", format!("{key} is a list"))]);
    };
    let (mut out, mut ps) = (vec![], vec![]);
    for (i, (item, label)) in items.iter().zip(step_labels(items)).enumerate() {
        match step(item) {
            Ok(s) => out.push(s),
            Err(e) => ps.extend(placed(e, i, &label)),
        }
    }
    if ps.is_empty() { Ok(out) } else { Err(ps) }
}

/// A step's problems at its label, or, for a step with none (neither a
/// name nor an object), at its list, saying which.
fn placed(e: Vec<Problem>, i: usize, label: &str) -> Vec<Problem> {
    e.into_iter()
        .map(|p| match label {
            "" => Problem::new("", format!("the {} step: {}", ordinal(i + 1), p.message)),
            l => p.under(l),
        })
        .collect()
}

/// 1st, 2nd, 3rd, 4th, 11th, 21st.
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A step's label as written: an action `#ai` (with its version), a
/// workflow's name, a group's wait (`all` for a plain list); `""` for a
/// step that is none of these.
fn raw_label(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(_) => "all".into(),
        Value::Object(m) if is_group(m) => WAITS
            .iter()
            .find(|w| m.contains_key(**w))
            .map_or_else(|| "all".to_string(), |w| w.to_string()),
        Value::Object(m) if m.len() == 1 => m.keys().next().cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

/// `/1`, `/2` added to each label that is in the list more than once.
fn numbered(labels: Vec<String>) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    labels
        .iter()
        .map(|l| {
            if l.is_empty() || labels.iter().filter(|x| *x == l).count() == 1 {
                return l.clone();
            }
            let n = seen.entry(l.clone()).or_default();
            *n += 1;
            format!("{l}/{n}")
        })
        .collect()
}

/// Each step's address in its list: `#ai`, a workflow's name, a group's
/// wait (`all` for a plain list); `/1`, `/2` when one is there more than
/// once.
pub fn step_labels(items: &[Value]) -> Vec<String> {
    numbered(items.iter().map(raw_label).collect())
}

/// Every step and undo of a workflow as written, with its address
/// (`steps/#ai/2`, `steps/all/#b`, `steps/#ship/undo`), what it runs and
/// its args: each step that can be read, even when others in the file
/// cannot, so each is checked on its own.
pub fn addressed(v: &Value) -> Vec<(String, Name, Value)> {
    let mut out = vec![];
    for key in ["steps", "success", "failure", "error"] {
        let Some(items) = v.get(key).and_then(Value::as_array) else {
            continue;
        };
        for (item, label) in items.iter().zip(step_labels(items)) {
            if label.is_empty() {
                continue;
            }
            let at = format!("{key}/{label}");
            let members = match item {
                Value::Array(ms) => Some(ms),
                Value::Object(m) if is_group(m) => WAITS
                    .iter()
                    .find_map(|w| m.get(*w))
                    .and_then(Value::as_array),
                _ => None,
            };
            if let Some(ms) = members {
                for (m, l) in ms.iter().zip(step_labels(ms)) {
                    let read = match m {
                        Value::String(s) => name(s).ok().map(|n| (n, Value::Object(Map::new()))),
                        Value::Object(o) if !is_group(o) => {
                            written(o).ok().map(|w| (w.name, w.args))
                        }
                        _ => None,
                    };
                    if let Some((n, args)) = read.filter(|_| !l.is_empty()) {
                        out.push((format!("{at}/{l}"), n, args));
                    }
                }
            } else if let Ok(Step::One {
                name,
                args,
                undo,
                undo_args,
                ..
            }) = step(item)
            {
                out.push((at.clone(), name, args));
                if let Some(u) = undo {
                    out.push((format!("{at}/undo"), u, undo_args));
                }
            }
        }
    }
    out
}

/// A name as a step writes it: `#ai`, `#ai@2`, `günlük`.
fn written_label(n: &Name) -> String {
    match n {
        Name::Action {
            name,
            version: Some(v),
        } => format!("#{name}@{v}"),
        other => other.label(),
    }
}

fn wait_word(w: &Wait) -> &'static str {
    match w {
        Wait::All => "all",
        Wait::Race => "race",
        Wait::Any => "any",
        Wait::Some(_) => "some",
        Wait::Settled => "settled",
    }
}

/// The addresses of steps already read, as [`step_labels`] gives them for
/// the steps as written.
fn parsed_labels(list: &[Step]) -> Vec<String> {
    numbered(
        list.iter()
            .map(|s| match s {
                Step::One { name, .. } => written_label(name),
                Step::Side(g) => wait_word(&g.wait).to_string(),
            })
            .collect(),
    )
}

fn step(v: &Value) -> Result<Step, Vec<Problem>> {
    let one = |e: String| vec![Problem::new("", e)];
    match v {
        Value::String(s) => {
            let name = name(s).map_err(one)?;
            Ok(Step::One {
                label: name.label(),
                name,
                args: Value::Object(Map::new()),
                on: None,
                undo: None,
                undo_args: Value::Object(Map::new()),
                repeat: None,
            })
        }
        Value::Array(items) => Ok(Step::Side(Group {
            members: side_members(items, "a list side by side")?,
            wait: Wait::All,
            fail: None,
        })),
        Value::Object(m) if is_group(m) => group(v, m),
        Value::Object(m) => {
            let w = written(m)?;
            Ok(Step::One {
                label: w.name.label(),
                name: w.name,
                args: w.args,
                on: w.on.map(Box::new),
                undo: w.undo,
                undo_args: w.undo_args,
                repeat: w.repeat,
            })
        }
        _ => Err(one(format!(
            "a step is a name, a list side by side, or {{ name: {{ data }} }}, not {v}"
        ))),
    }
}

/// A step's args as written, checked: an object, a list of objects, or a
/// `"$name"` that is one (`"$output"`). An item of a list is direct, each
/// key an arg given as it is, or a pair `{"title": name, "text": text}`,
/// the arg `name` formatted as text. A name may be given once; `output` is
/// itero's.
pub(crate) fn read_args(a: &Value) -> Result<Value, String> {
    let given = |name: &str, seen: &mut Vec<String>| -> Result<(), String> {
        if name == "event" {
            return Err(
                "args: event is itero's: a waiting action gets the event that came as $event; name the arg otherwise"
                    .into(),
            );
        }
        if seen.iter().any(|s| s == name) {
            return Err(format!("args: {name} is given twice"));
        }
        seen.push(name.to_string());
        Ok(())
    };
    // a secret inside text, and a `$word` that is no value, are refused
    let no_secret = |v: &Value| {
        if let Some(w) = unknown_in(v).first() {
            return Err(format!("args: ${w} is no value: write {}", instead(w)));
        }
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
    name: Name,
    args: Value,
    on: Option<On>,
    undo: Option<Name>,
    undo_args: Value,
    repeat: Option<Repeat>,
}

fn written(m: &Map<String, Value>) -> Result<Written, Vec<Problem>> {
    let one = |e: &str| vec![Problem::new("", e)];
    let mut it = m.iter();
    let (Some((k, v)), None) = (it.next(), it.next()) else {
        return Err(one("a step object has one name: { \"#ship\": { ... } }"));
    };
    let Value::Object(fields) = v else {
        return Err(one("a step is an object: { \"args\": { ... } }"));
    };
    // each of its fields on its own: every problem told
    let mut ps = vec![];
    let step_name = match name(k) {
        Ok(n) => Some(n),
        Err(e) => {
            ps.push(Problem::new("", e));
            None
        }
    };
    let mut fields = fields.clone();
    let when = match fields.remove("on").map(|w| on(&w)) {
        None => None,
        Some(Err(e)) => {
            ps.extend(e.into_iter().map(|p| Problem::new("", p.message)));
            None
        }
        Some(Ok(w)) => {
            let starts = [
                (!w.events.is_empty(), "an event"),
                (!w.schedules.is_empty(), "a schedule"),
                (!w.paths.is_empty(), "a path"),
            ];
            if let Some((_, what)) = starts.iter().find(|(given, _)| *given) {
                ps.push(Problem::new(
                    "",
                    format!(
                        "a step's on holds conditions ({{\"$name\": value}}, {{\"has\": ...}}), not {what}: an event, a schedule or a path starts a workflow"
                    ),
                ));
            }
            Some(w)
        }
    };
    // a name, or a step with args: `{ "#küme-sil": { "args": { "id": "$output.id" } } }`
    let empty = || Value::Object(Map::new());
    let (undo, undo_args) = match fields.remove("undo") {
        None => (None, empty()),
        Some(Value::String(s)) => match name(&s) {
            Ok(n) => (Some(n), empty()),
            Err(e) => {
                ps.push(Problem::new("undo", e));
                (None, empty())
            }
        },
        Some(Value::Object(u)) if u.len() == 1 => {
            let (n, v) = u.iter().next().expect("one");
            let args = match v {
                Value::Object(f) if f.keys().all(|key| key == "args") => match f.get("args") {
                    Some(a) => read_args(a).unwrap_or_else(|why| {
                        ps.push(Problem::new("undo", why));
                        empty()
                    }),
                    None => empty(),
                },
                _ => {
                    ps.push(Problem::new(
                        "undo",
                        format!(
                            "a step to undo takes args only: {{ \"{n}\": {{ \"args\": {{ ... }} }} }}"
                        ),
                    ));
                    empty()
                }
            };
            match name(n) {
                Ok(n) => (Some(n), args),
                Err(e) => {
                    ps.push(Problem::new("undo", e));
                    (None, args)
                }
            }
        }
        Some(_) => {
            ps.push(Problem::new(
                "undo",
                "undo is an action or a workflow, like \"#cluster-delete\", or one with args",
            ));
            (None, empty())
        }
    };
    let args = match fields.remove("args") {
        None => empty(),
        Some(a) => read_args(&a).unwrap_or_else(|why| {
            ps.push(Problem::new("", why));
            empty()
        }),
    };
    let rounds = match fields.remove("repeat") {
        None => None,
        Some(v) => repeat(&v, false).map_or_else(
            |e| {
                ps.push(Problem::new("", e));
                None
            },
            Some,
        ),
    };
    if fields.remove("strict").is_some() {
        ps.push(Problem::new(
            "",
            "strict is gone: a step's args reach its action and nothing else now",
        ));
    }
    for other in fields.keys() {
        ps.push(Problem::new(
            "",
            format!(
                "\"{other}\" is not one of args, on, undo, repeat: a step's values go under args"
            ),
        ));
    }
    // a workflow's `$event` is data, where a `$$NAME` is only text
    for (at, n, a) in [("", &step_name, &args), ("undo", &undo, &undo_args)] {
        if let Some(Name::Workflow(w)) = n
            && let Some(arg) = written_secrets(a).first()
        {
            ps.push(Problem::new(
                at,
                format!(
                    "args: a secret goes to an action, not to a workflow: write it in the step of {w} that needs it, not as {arg} here"
                ),
            ));
        }
    }
    match step_name {
        Some(name) if ps.is_empty() => Ok(Written {
            name,
            args,
            on: when,
            undo,
            undo_args,
            repeat: rounds,
        }),
        _ => Err(ps),
    }
}

/// The steps of a list side by side, each a name or a step object; `what`
/// says whose list it is. Each one's problems are at its label.
fn side_members(items: &[Value], what: &str) -> Result<Vec<Member>, Vec<Problem>> {
    let one = |e: String| vec![Problem::new("", e)];
    let (mut out, mut ps) = (vec![], vec![]);
    for (i, (item, label)) in items.iter().zip(step_labels(items)).enumerate() {
        let member = match item {
            Value::String(s) => name(s).map(Member::bare).map_err(one),
            Value::Object(m) if !is_group(m) => written(m).and_then(|w| {
                if w.undo.is_some() {
                    return Err(vec![Problem::new(
                        "undo",
                        "undo is for a step of a list, not one side by side",
                    )]);
                }
                if w.repeat.is_some() {
                    return Err(vec![Problem::new(
                        "",
                        "repeat is for a step of a list, not one side by side: put the step in a workflow of its own, repeated, used here by its name",
                    )]);
                }
                let label = w.name.label();
                Ok(Member {
                    name: w.name,
                    args: w.args,
                    on: w.on.map(Box::new),
                    label,
                })
            }),
            Value::Object(_) => Err(one(format!(
                "a group inside a group goes in a workflow of its own, used here by its name: {item}"
            ))),
            _ => Err(one(format!(
                "a step of {what} is a name or an object, like {{ \"#ask\": {{ \"args\": ... }} }}, not {item}"
            ))),
        };
        match member {
            Ok(m) => out.push(m),
            Err(e) => ps.extend(placed(e, i, &label)),
        }
    }
    if ps.is_empty() { Ok(out) } else { Err(ps) }
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
/// how to wait. Every error shows the step as written; its steps' own
/// problems are at their labels.
fn group(v: &Value, m: &Map<String, Value>) -> Result<Step, Vec<Problem>> {
    let bad = |why: String| Problem::new("", format!("{v}: {why}"));
    let keys: Vec<&str> = m
        .keys()
        .map(String::as_str)
        .filter(|k| WAITS.contains(k))
        .collect();
    // its shape first: past a wrong one, nothing inside it is checked
    let key = match keys.as_slice() {
        [one] => *one,
        [] => {
            return Err(vec![bad(
                "a group holds one of all, race, any, some or settled, with a list of steps".into(),
            )]);
        }
        several => {
            return Err(vec![bad(format!(
                "a group holds one of all, race, any, some or settled, not both {}",
                several.join(" and ")
            ))]);
        }
    };
    let items = match &m[key] {
        Value::Array(items) if items.len() >= 2 => items,
        _ => {
            return Err(vec![bad(format!(
                "{key} is a list of two or more steps, like [\"#a\", \"#b\"]"
            ))]);
        }
    };
    let mut ps = vec![];
    for other in m.keys().filter(|k| {
        !WAITS.contains(&k.as_str()) && !["fail", "count", "names"].contains(&k.as_str())
    }) {
        ps.push(bad(format!(
            "\"{other}\" is not one of {key}, fail, count, names"
        )));
    }
    let members = match side_members(items, key) {
        Ok(members) => members,
        Err(e) => {
            // its fail and names name its steps: not checked without them
            ps.extend(e);
            return Err(ps);
        }
    };
    // names of its steps, as written: checked once the file's steps are
    // numbered ([`Flow::number`])
    let steps_of = |what: &str, v: &Value| -> Result<Vec<String>, String> {
        let list: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            one => vec![one],
        };
        list.iter()
            .map(|n| {
                n.as_str()
                    .map(String::from)
                    .ok_or(format!("{what}: {n} is not one of the steps in the list"))
            })
            .collect()
    };
    let wait = match key {
        "all" => Ok(Wait::All),
        "race" => Ok(Wait::Race),
        "any" => Ok(Wait::Any),
        "settled" => Ok(Wait::Settled),
        _ => match (m.get("count"), m.get("names")) {
            (None, None) => Err("some needs count or names".to_string()),
            (Some(_), Some(_)) => Err("some takes count or names, not both".to_string()),
            (Some(c), None) => match c.as_u64() {
                Some(n) if (1..=members.len() as u64).contains(&n) => {
                    Ok(Wait::Some(Need::Count(n as usize)))
                }
                _ => Err(format!(
                    "count is a whole number from 1 to {}, the steps in the list",
                    members.len()
                )),
            },
            (None, Some(n)) => match n {
                Value::Array(a) if !a.is_empty() => {
                    steps_of("names", n).map(|ns| Wait::Some(Need::Names(ns)))
                }
                _ => Err(
                    "names is a list of the steps that must end well, like [\"#a\", \"#c\"]"
                        .to_string(),
                ),
            },
        },
    };
    for k in ["count", "names"] {
        if key != "some" && m.contains_key(k) {
            ps.push(bad(format!("{k} goes with some")));
        }
    }
    let fail = match m.get("fail") {
        None => Ok(None),
        Some(Value::String(s)) if s == "any" => Ok(Some(Fail::Any)),
        Some(Value::String(s)) if s == "all" => Ok(Some(Fail::All)),
        Some(Value::String(s)) if s == "never" => Ok(Some(Fail::Never)),
        Some(f @ (Value::String(_) | Value::Array(_)))
            if f.as_array().is_none_or(|a| !a.is_empty()) =>
        {
            steps_of("fail", f).map(|ns| Some(Fail::Steps(ns)))
        }
        Some(_) => {
            Err("fail is \"any\", \"all\", \"never\", a step's name or a list of names".to_string())
        }
    };
    match (wait, fail) {
        (Ok(wait), Ok(fail)) if ps.is_empty() => Ok(Step::Side(Group {
            members,
            wait,
            fail,
        })),
        (wait, fail) => {
            ps.extend(wait.err().map(bad));
            ps.extend(fail.err().map(bad));
            Err(ps)
        }
    }
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

    /// The names its steps' results are kept by, in every list, as
    /// [`step_key`] gives them.
    pub fn step_keys(&self) -> Vec<String> {
        let mut out = vec![];
        for s in self.all_steps() {
            match s {
                Step::One { label, .. } => out.push(step_key(label)),
                Step::Side(g) => out.extend(g.members.iter().map(|m| step_key(&m.label))),
            }
        }
        out
    }

    /// Numbers each step whose name is in the file more than once, `/1`,
    /// `/2`, counted over the whole file (`steps`, `success`, `failure`,
    /// `error`; a step side by side where it stands), whatever its version;
    /// then reads each group's `fail` and `names` by those numbers, the
    /// name alone the first.
    fn number(&mut self) -> Vec<Problem> {
        let mut counts: std::collections::HashMap<String, usize> = Default::default();
        for (_, list) in self.lists() {
            for s in list {
                match s {
                    Step::One { label, .. } => *counts.entry(label.clone()).or_default() += 1,
                    Step::Side(g) => {
                        for m in &g.members {
                            *counts.entry(m.label.clone()).or_default() += 1;
                        }
                    }
                }
            }
        }
        let mut seen: std::collections::HashMap<String, usize> = Default::default();
        let mut numbered = |l: &mut String| {
            if counts.get(l.as_str()).copied().unwrap_or(0) > 1 {
                let n = seen.entry(l.clone()).or_default();
                *n += 1;
                *l = format!("{l}/{n}");
            }
        };
        for list in [
            &mut self.steps,
            &mut self.success,
            &mut self.failure,
            &mut self.error,
        ] {
            for s in list.iter_mut() {
                match s {
                    Step::One { label, .. } => numbered(label),
                    Step::Side(g) => g.members.iter_mut().for_each(|m| numbered(&mut m.label)),
                }
            }
        }
        let mut ps = vec![];
        for (key, list) in [
            ("steps", &mut self.steps),
            ("success", &mut self.success),
            ("failure", &mut self.failure),
            ("error", &mut self.error),
        ] {
            let at = parsed_labels(list);
            for (s, at) in list.iter_mut().zip(at) {
                let Step::Side(g) = s else {
                    continue;
                };
                let labels: Vec<String> = g.members.iter().map(|m| m.label.clone()).collect();
                let mut read = |what: &str, names: &mut Vec<String>| {
                    for n in names.iter_mut() {
                        let first = format!("{n}/1");
                        if labels.contains(n) {
                        } else if labels.contains(&first) {
                            *n = first;
                        } else {
                            ps.push(Problem::new(
                                format!("{key}/{at}"),
                                format!("{what}: \"{n}\" is not one of the steps in the list"),
                            ));
                        }
                    }
                };
                if let Some(Fail::Steps(names)) = &mut g.fail {
                    read("fail", names);
                }
                if let Wait::Some(Need::Names(names)) = &mut g.wait {
                    read("names", names);
                }
            }
        }
        ps
    }

    /// A `$steps.x` naming no step, or a name two steps have: a problem
    /// at the step (or the `on`) that reads it.
    fn check_names(&self) -> Vec<Problem> {
        let keys = self.step_keys();
        let mut ps = vec![];
        let conditions = |on: &On, read: &mut Vec<String>| {
            read.extend(
                on.has
                    .iter()
                    .filter(|h| !h.starts_with("$$"))
                    .map(|h| h.trim_start_matches('$').to_string()),
            );
            read.extend(on.equal.keys().cloned());
        };
        let told = |at: String, read: Vec<String>, ps: &mut Vec<Problem>| {
            let mut seen = vec![];
            for path in read {
                let Some(rest) = path.strip_prefix("steps.") else {
                    continue;
                };
                let step = rest.split('.').next().unwrap_or(rest).to_string();
                if seen.contains(&step) {
                    continue;
                }
                // the name alone is the first of its number
                let first = format!("{step}/1");
                if keys.iter().any(|k| *k == step || *k == first) {
                    seen.push(step);
                    continue;
                }
                let message = format!(
                    "$steps.{step} names no step of this workflow (its steps: {})",
                    keys.join(", ")
                );
                ps.push(Problem::new(at.clone(), message));
                seen.push(step);
            }
        };
        let mut read = vec![];
        conditions(&self.on, &mut read);
        told("on".into(), read, &mut ps);
        for (key, list) in self.lists() {
            for (s, label) in list.iter().zip(parsed_labels(list)) {
                let parts: Vec<(String, &Value, Option<&On>)> = match s {
                    Step::One { args, on, .. } => {
                        vec![(format!("{key}/{label}"), args, on.as_deref())]
                    }
                    Step::Side(g) => {
                        let names =
                            numbered(g.members.iter().map(|m| written_label(&m.name)).collect());
                        g.members
                            .iter()
                            .zip(names)
                            .map(|(m, n)| (format!("{key}/{label}/{n}"), &m.args, m.on.as_deref()))
                            .collect()
                    }
                };
                for (at, args, on) in parts {
                    let mut read = names_in(args);
                    if let Some(on) = on {
                        conditions(on, &mut read);
                    }
                    told(at, read, &mut ps);
                }
            }
        }
        ps
    }

    /// Its lists of steps by their keys in the file.
    fn lists(&self) -> [(&'static str, &Vec<Step>); 4] {
        [
            ("steps", &self.steps),
            ("success", &self.success),
            ("failure", &self.failure),
            ("error", &self.error),
        ]
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
        let mut out: Vec<String> = self
            .args_of(action)
            .into_iter()
            .flat_map(arg_names_of)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The args of each step of `action`, as written: an object, a list,
    /// or a `"$name"`.
    pub fn args_of(&self, action: &str) -> Vec<&Value> {
        let is = |n: &Name| matches!(n, Name::Action { name, .. } if name == action);
        let mut out = vec![];
        for s in self.all_steps() {
            match s {
                Step::One { name, args, .. } if is(name) => out.push(args),
                Step::Side(g) => {
                    out.extend(g.members.iter().filter(|m| is(&m.name)).map(|m| &m.args))
                }
                Step::One { .. } => {}
            }
        }
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
pub fn arg_names_of(args: &Value) -> Vec<String> {
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

/// The args a step writes as a whole `"$$NAME"`: the only ones that are
/// secrets. Values are data, however they look: what came in an event or a
/// step's result must never be able to name a secret.
pub fn written_secrets(args: &Value) -> Vec<String> {
    let of = |o: &Map<String, Value>| -> Vec<String> {
        o.iter()
            .filter(|(_, v)| secret_ref(v).is_some())
            .map(|(k, _)| k.clone())
            .collect()
    };
    match args {
        Value::Object(o) => of(o),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_object)
            .filter(|o| pair(o).is_none())
            .flat_map(of)
            .collect(),
        _ => vec![],
    }
}

/// `"$$NAME"`, the whole value: the name of the environment variable or
/// secret it stands for.
pub fn secret_ref(v: &Value) -> Option<&str> {
    let n = v.as_str()?.strip_prefix("$$")?;
    (!n.is_empty() && name_len(n) == n.len()).then_some(n)
}

/// `"$event.order"`, `"$output"`, `"$steps.ask.price"`: the whole value,
/// its path from a root, without `$`.
pub fn state_ref(v: &Value) -> Option<&str> {
    let s = v.as_str()?.strip_prefix('$')?;
    let n = path_len(s);
    (n > 0 && n == s.len()).then_some(s)
}
