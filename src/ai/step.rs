//! The built-in `#ai`: asks the local model, through itero-ai on the socket
//! the daemon gave the step, for a text, one of a list, or typed fields.
use crate::engine::worker::{Failure, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::time::Duration;

/// The args `#ai` reads.
pub const ARGS: [&str; 13] = [
    "model",
    "text",
    "system",
    "choose",
    "fields",
    "thinking",
    "temperature",
    "top_p",
    "top_k",
    "min_p",
    "presence_penalty",
    "max_tokens",
    "seed",
];

/// The types a field of `fields` may have.
pub const FIELD_TYPES: [&str; 5] = ["text", "number", "integer", "boolean", "date"];

/// What the answer is to be.
#[derive(Debug, PartialEq)]
enum Shape {
    Text,
    Choose(Vec<String>),
    Fields(Vec<(String, String)>),
}

fn fail(why: impl std::fmt::Display) -> String {
    format!("#ai: {why}")
}

/// The labels of `choose`, given as a list.
fn labels(text: &str) -> Result<Vec<String>, String> {
    let bad = || fail(format!("choose is a list of one label or more: {text}"));
    let labels: Vec<String> = serde_json::from_str(text).map_err(|_| bad())?;
    if labels.is_empty() || labels.iter().any(|l| l.trim().is_empty()) {
        return Err(bad());
    }
    Ok(labels)
}

/// The names and types of `fields`, given as an object, in its order.
fn fields(text: &str) -> Result<Vec<(String, String)>, String> {
    let bad = || fail(format!("fields is an object of names and types: {text}"));
    let o: Map<String, Value> = serde_json::from_str(text).map_err(|_| bad())?;
    if o.is_empty() {
        return Err(bad());
    }
    o.into_iter()
        .map(|(name, t)| match t.as_str() {
            Some(t) if FIELD_TYPES.contains(&t) => Ok((name, t.to_string())),
            _ => Err(fail(format!(
                "fields take text, number, integer, boolean or date, not {} (for {name})",
                t.as_str().map_or_else(|| t.to_string(), String::from)
            ))),
        })
        .collect()
}

fn shape(env: &BTreeMap<String, String>) -> Result<Shape, String> {
    match (env.get("choose"), env.get("fields")) {
        (Some(_), Some(_)) => Err(fail("it takes choose or fields, not both")),
        (Some(c), None) => Ok(Shape::Choose(labels(c)?)),
        (None, Some(f)) => Ok(Shape::Fields(fields(f)?)),
        (None, None) => Ok(Shape::Text),
    }
}

fn field_schema(t: &str) -> Value {
    match t {
        "date" => json!({ "type": "string", "format": "date" }),
        "text" => json!({ "type": "string" }),
        other => json!({ "type": other }),
    }
}

/// How the answer's form is said in words: a small model held to a form
/// by a grammar alone, without being told it, answers poorly.
fn form_sentence(shape: &Shape) -> Option<String> {
    match shape {
        Shape::Text => None,
        Shape::Choose(ls) => {
            let quoted: Vec<String> = ls.iter().map(|l| format!("\"{l}\"")).collect();
            let list = match quoted.split_last() {
                Some((last, [])) => last.clone(),
                Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
                None => String::new(),
            };
            Some(format!("Answer as a JSON string: {list}."))
        }
        Shape::Fields(fs) => {
            let parts: Vec<String> = fs
                .iter()
                .map(|(n, t)| {
                    let v = match t.as_str() {
                        "text" => "\"<text>\"",
                        "number" => "<number>",
                        "integer" => "<whole number>",
                        "boolean" => "<true or false>",
                        _ => "\"<YYYY-MM-DD>\"",
                    };
                    format!("\"{n}\": {v}")
                })
                .collect();
            Some(format!(
                "Answer as a JSON object: {{{}}}.",
                parts.join(", ")
            ))
        }
    }
}

fn number(env: &BTreeMap<String, String>, k: &str) -> Result<Option<Value>, String> {
    let Some(t) = env.get(k) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(t.trim())
        .ok()
        .filter(Value::is_number)
        .ok_or_else(|| fail(format!("{k} is a number, not {t}")))?;
    Ok(Some(v))
}

/// The chat request for a step's args (as a step's environment has them:
/// text, and lists and objects as JSON).
pub fn request(env: &BTreeMap<String, String>) -> Result<Value, String> {
    let model = env.get("model").ok_or_else(|| fail("it needs $model"))?;
    let text = env.get("text").ok_or_else(|| fail("it needs $text"))?;
    let shape = shape(env)?;
    let mut messages = vec![];
    if let Some(system) = env.get("system") {
        messages.push(json!({ "role": "system", "content": system }));
    }
    let content = match form_sentence(&shape) {
        Some(s) => format!("{text}\n\n{s}"),
        None => text.clone(),
    };
    messages.push(json!({ "role": "user", "content": content }));
    let mut r = json!({ "model": model, "messages": messages, "stream": true });
    let schema = match &shape {
        Shape::Text => None,
        Shape::Choose(ls) => Some(json!({ "type": "string", "enum": ls })),
        Shape::Fields(fs) => {
            let props: Map<String, Value> = fs
                .iter()
                .map(|(n, t)| (n.clone(), field_schema(t)))
                .collect();
            let required: Vec<&String> = fs.iter().map(|(n, _)| n).collect();
            Some(json!({ "type": "object", "properties": props,
                         "required": required, "additionalProperties": false }))
        }
    };
    if let Some(schema) = schema {
        r["response_format"] =
            json!({ "type": "json_schema", "json_schema": { "name": "answer", "schema": schema } });
    }
    match env.get("thinking").map(String::as_str) {
        None | Some("false") => {}
        Some("true") => r["chat_template_kwargs"] = json!({ "enable_thinking": true }),
        Some(other) => return Err(fail(format!("thinking is true or false, not {other}"))),
    }
    for k in [
        "temperature",
        "top_p",
        "top_k",
        "min_p",
        "presence_penalty",
        "seed",
    ] {
        if let Some(v) = number(env, k)? {
            r[k] = v;
        }
    }
    if let Some(v) = number(env, "max_tokens")? {
        r["max_tokens"] = v;
    }
    Ok(r)
}

/// What `itero check` can say of an `#ai` step's args before a run: the
/// same rules as a run's, on the values written in the workflow; a
/// `"$name"` is known only in a run, and is left aside. Gives the model's
/// name when it is written out.
pub fn check(args: &Value) -> Result<Option<String>, String> {
    let items: Vec<&Map<String, Value>> = match args {
        Value::Object(o) => vec![o],
        Value::Array(a) => a.iter().filter_map(Value::as_object).collect(),
        // "$output" and the like: known only in a run
        _ => return Ok(None),
    };
    let mut env = BTreeMap::new();
    for o in items {
        for (k, v) in o {
            let text = match v {
                Value::String(s)
                    if s.starts_with('$') && !["model", "text", "system"].contains(&k.as_str()) =>
                {
                    continue;
                }
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            env.insert(k.clone(), text);
        }
    }
    request(&env)?;
    Ok(env.get("model").filter(|m| !m.starts_with('$')).cloned())
}

/// A warning when this machine has no such model (another machine may).
pub fn model_warning(model: &str) -> Option<String> {
    let ix = itero_ai_index::Index::read(&super::index_path().ok()?).ok()?;
    match itero_ai_index::resolve(&ix, model) {
        Ok(_) => None,
        Err(e) => Some(format!("#ai: {e} (itero ai models list shows them)")),
    }
}

/// The answer as `$output`: the text, the label, or the fields' object.
fn output(env: &BTreeMap<String, String>, content: &str) -> Result<Value, String> {
    match shape(env)? {
        Shape::Text => Ok(Value::String(content.trim().to_string())),
        Shape::Choose(_) | Shape::Fields(_) => serde_json::from_str(content.trim()).map_err(|e| {
            fail(format!(
                "the model's answer is not the JSON asked for ({e}): {content}"
            ))
        }),
    }
}

/// Runs `#ai`: streams the answer, telling its token count as progress.
pub fn run(r: &StepRun, told: &(dyn Fn(Option<&str>) + Sync)) -> Result<Outcome, Failure> {
    let body = request(&r.env)?;
    let socket = r
        .env
        .get("ITERO_AI_SOCKET")
        .ok_or_else(|| fail("no socket: the daemon gives one to the step"))?;
    let client = crate::net::unix_client(socket.as_ref(), Duration::from_secs(r.most.max(1)))
        .map_err(fail)?;
    let resp = client
        .post("http://itero-ai/v1/chat/completions")
        .json(&body)
        .send()
        .map_err(|e| fail(format!("itero-ai did not answer: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let v: Value = resp.json().unwrap_or_default();
        let why = v["error"]["message"]
            .as_str()
            .unwrap_or("no reason given")
            .to_string();
        if status.as_u16() == 404 {
            missing(r, body["model"].as_str().unwrap_or(""));
            return Err(fail(format!(
                "{why}: looking for models now; run it again in a minute \
                 (itero ai models list shows them)"
            ))
            .into());
        }
        return Err(fail(why).into());
    }
    let (mut content, mut thinking, mut n) = (String::new(), String::new(), 0u64);
    for line in std::io::BufReader::new(resp).lines() {
        let line = line.map_err(|e| fail(format!("reading the answer: {e}")))?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let v: Value =
            serde_json::from_str(data).map_err(|e| fail(format!("a piece of the answer: {e}")))?;
        if let Some(m) = v["error"]["message"].as_str() {
            return Err(fail(m).into());
        }
        let delta = &v["choices"][0]["delta"];
        if let Some(t) = delta["content"].as_str() {
            content.push_str(t);
        }
        if let Some(t) = delta["reasoning_content"].as_str() {
            thinking.push_str(t);
        }
        if !delta.as_object().is_none_or(Map::is_empty) {
            n += 1;
            if n % 16 == 0 {
                told(Some(&format!("[{n} tokens]")));
            } else {
                told(None);
            }
        }
    }
    if !thinking.is_empty() {
        log_thinking(r, &thinking);
    }
    Ok(Outcome::Done(output(&r.env, &content)?))
}

/// Tells the workspace a model is missing: the `models` workflow looks
/// for models on it.
fn missing(r: &StepRun, model: &str) {
    if let Ok(ws) = crate::workspace::find(&r.root) {
        super::keeper::publish(
            &ws,
            crate::engine::events::AI_MODEL_MISSING,
            json!({ "model": model, "workflow": r.workflow }),
        );
    }
}

/// The model's thinking goes to the workspace's log, never into `$output`.
fn log_thinking(r: &StepRun, thinking: &str) {
    let Ok(ws) = crate::workspace::find(&r.root) else {
        return;
    };
    if let Ok(id) = crate::engine::journal::Journal::open(&ws).and_then(|j| j.workspace()) {
        crate::engine::log::Log::new(&ws, &id.id).write(
            "ai.thinking",
            json!({ "workflow": r.workflow, "thinking": thinking }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_form_is_said_in_words_too() {
        let r = request(&env(&[
            ("model", "u/k"),
            ("text", "Bu posta ne?"),
            ("choose", r#"["acil","sonra","çöp"]"#),
        ]))
        .unwrap();
        assert_eq!(
            r["messages"][0]["content"],
            "Bu posta ne?\n\nAnswer as a JSON string: \"acil\", \"sonra\" or \"çöp\"."
        );
        let r = request(&env(&[
            ("model", "u/k"),
            ("text", "Fatura"),
            (
                "fields",
                r#"{"tutar":"number","son":"date","acil":"boolean"}"#,
            ),
        ]))
        .unwrap();
        assert_eq!(
            r["messages"][0]["content"],
            "Fatura\n\nAnswer as a JSON object: {\"tutar\": <number>, \"son\": \"<YYYY-MM-DD>\", \"acil\": <true or false>}."
        );
        let r = request(&env(&[("model", "u/k"), ("text", "Özetle")])).unwrap();
        assert_eq!(r["messages"][0]["content"], "Özetle");
        assert!(r.get("response_format").is_none());
    }
}
