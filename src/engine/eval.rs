//! Evaluating CEL expressions over a run's JSON values, and filling in
//! `${...}` templates in an action's arguments. `${env.NAME}` is left as it
//! is: a secret, filled in by the daemon when a worker starts.
use cel_interpreter::{Context, Program};
use serde_json::{Map, Value};

/// What expressions see.
pub struct Vars<'a> {
    pub run: &'a Map<String, Value>,
    pub memory: &'a Map<String, Value>,
    pub args: &'a Map<String, Value>,
    pub input: &'a Value,
    pub prev: &'a Value,
    /// The outputs of the chain's actions with an `id`.
    pub steps: &'a Map<String, Value>,
    /// The action's output, for `expect`.
    pub output: Option<&'a Value>,
    /// The event a transition is offered, `{id, type, data}`, for `match`.
    pub event: Option<&'a Value>,
}

/// A JSON value as CEL sees it. Whole numbers are signed integers (CEL's
/// `int`), so `args.tries + 1` works; only one too big for that is `uint`.
fn cel(v: &Value) -> cel_interpreter::Value {
    use cel_interpreter::Value as C;
    match v {
        Value::Null => C::Null,
        Value::Bool(b) => C::Bool(*b),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => C::Int(i),
            (None, Some(u)) => C::UInt(u),
            _ => C::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Value::String(s) => C::String(s.clone().into()),
        Value::Array(a) => C::List(a.iter().map(cel).collect::<Vec<_>>().into()),
        Value::Object(o) => o
            .iter()
            .map(|(k, x)| (k.clone(), cel(x)))
            .collect::<std::collections::HashMap<String, C>>()
            .into(),
    }
}

fn object(m: &Map<String, Value>) -> cel_interpreter::Value {
    cel(&Value::Object(m.clone()))
}

/// Evaluates an expression to a JSON value.
pub fn value(src: &str, v: &Vars) -> Result<Value, String> {
    let program = Program::compile(src).map_err(|e| format!("bad expression `{src}`: {e}"))?;
    let mut c = Context::default();
    c.add_variable_from_value("run", object(v.run));
    c.add_variable_from_value("memory", object(v.memory));
    c.add_variable_from_value("args", object(v.args));
    c.add_variable_from_value("input", cel(v.input));
    c.add_variable_from_value("prev", cel(v.prev));
    c.add_variable_from_value("steps", object(v.steps));
    if let Some(o) = v.output {
        c.add_variable_from_value("output", cel(o));
    }
    if let Some(e) = v.event {
        c.add_variable_from_value("event", cel(e));
    }
    let result = program.execute(&c).map_err(|e| format!("`{src}`: {e}"))?;
    result
        .json()
        .map_err(|e| format!("`{src}`: the result is not JSON: {e:?}"))
}

/// Evaluates a condition: it must be true or false.
pub fn truth(src: &str, v: &Vars) -> Result<bool, String> {
    match value(src, v)? {
        Value::Bool(b) => Ok(b),
        other => Err(format!("`{src}` must be true or false, not {other}")),
    }
}

/// A piece of a string: text, or a `${...}` template (with its `${` and `}`).
pub enum Piece<'a> {
    Text(&'a str),
    Expr(&'a str),
}

/// A string cut into its text and its `${...}` templates, in order.
pub fn pieces(s: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        if i > 0 {
            out.push(Piece::Text(&rest[..i]));
        }
        let after = &rest[i + 2..];
        let j = after
            .find('}')
            .ok_or_else(|| format!("unclosed `${{` in `{s}`"))?;
        out.push(Piece::Expr(&rest[i..i + 2 + j + 1]));
        rest = &after[j + 1..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    Ok(out)
}

fn inner(expr: &str) -> &str {
    expr[2..expr.len() - 1].trim()
}

/// The secret a `${env.NAME}` template names; none for any other template.
pub fn secret_name(expr: &str) -> Option<&str> {
    inner(expr).strip_prefix("env.").map(str::trim)
}

fn is_secret(expr: &str) -> bool {
    secret_name(expr).is_some()
}

fn render_text(s: &str, v: &Vars) -> Result<Value, String> {
    let ps = pieces(s)?;
    if let [Piece::Expr(e)] = ps.as_slice()
        && !is_secret(e)
    {
        return value(inner(e), v);
    }
    let mut out = String::new();
    for p in ps {
        match p {
            Piece::Text(t) => out += t,
            Piece::Expr(e) if is_secret(e) => out += e,
            Piece::Expr(e) => match value(inner(e), v)? {
                Value::String(t) => out += &t,
                other => out += &other.to_string(),
            },
        }
    }
    Ok(Value::String(out))
}

/// Fills in every `${...}` in a value's strings. A string that is one
/// `${expr}` becomes the expression's value, keeping its type; otherwise
/// values are written into the text.
pub fn render(value: &Value, v: &Vars) -> Result<Value, String> {
    Ok(match value {
        Value::String(s) => render_text(s, v)?,
        Value::Array(a) => Value::Array(a.iter().map(|x| render(x, v)).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| Ok((k.clone(), render(x, v)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}
