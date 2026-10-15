//! The built-in actions the manager runs itself (they touch nothing outside
//! the decision), and the checks other built-ins have by default.
use serde_json::{Map, Value};

/// The built-ins that run inside the manager.
pub const PURE: [&str; 3] = ["memory.set@1", "json.pick@1", "event.emit@1"];

pub fn is_pure(action: &str) -> bool {
    PURE.contains(&action)
}

/// The check an action has when it gives no `expect`.
pub fn default_expect(action: &str) -> Option<&'static str> {
    match action {
        "http.request@1" => Some("output.status >= 200 && output.status < 300"),
        "command@1" => Some("output.exit == 0"),
        _ => None,
    }
}

/// What a pure built-in did.
pub struct Done {
    pub output: Value,
    /// Memory changes to hold back until the chain succeeds.
    pub memory: Map<String, Value>,
    /// An event to send when the chain succeeds: type, target, data.
    pub emit: Option<(String, String, Value)>,
}

/// Runs a pure built-in on its filled-in arguments and the previous output.
pub fn run(action: &str, with: &Map<String, Value>, prev: &Value) -> Result<Done, String> {
    let text = |k: &str| {
        with.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("{action} needs with.{k} as text"))
    };
    match action {
        "memory.set@1" => Ok(Done {
            output: Value::Object(with.clone()),
            memory: with.clone(),
            emit: None,
        }),
        "json.pick@1" => {
            let path = text("path")?;
            let mut v = prev;
            for part in path.split('.') {
                v = match v {
                    Value::Object(o) => o.get(part),
                    Value::Array(a) => part.parse::<usize>().ok().and_then(|i| a.get(i)),
                    _ => None,
                }
                .ok_or_else(|| format!("json.pick@1: no {path} in the previous output"))?;
            }
            Ok(Done {
                output: v.clone(),
                memory: Map::new(),
                emit: None,
            })
        }
        "event.emit@1" => {
            let data = with.get("data").cloned().unwrap_or(Value::Null);
            Ok(Done {
                output: data.clone(),
                memory: Map::new(),
                emit: Some((text("type")?, text("target")?, data)),
            })
        }
        other => Err(format!("{other} does not run in the manager")),
    }
}
