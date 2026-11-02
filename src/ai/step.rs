//! The built-in `#ai`: asks the local model, through itero-ai on the socket
//! the daemon gave the step, or a model behind a `url`, for a text or typed
//! fields.
use crate::engine::worker::{Failure, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::time::{Duration, Instant};

/// The args `#ai` reads.
pub const ARGS: [&str; 15] = [
    "url",
    "key",
    "model",
    "message",
    "system",
    "fields",
    "thinking",
    "temperature",
    "top_p",
    "top_k",
    "min_p",
    "presence_penalty",
    "max_tokens",
    "seed",
    "context",
];

/// The types a field of `fields` may have.
pub const FIELD_TYPES: [&str; 5] = ["text", "number", "integer", "boolean", "date"];

/// What the answer is to be.
#[derive(Debug, PartialEq)]
pub(super) enum Shape {
    Text,
    Fields(Vec<(String, String)>),
}

pub(super) fn fail(why: impl std::fmt::Display) -> String {
    format!("#ai: {why}")
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

pub(super) fn shape(env: &BTreeMap<String, String>) -> Result<Shape, String> {
    if env.contains_key("choose") {
        return Err(fail(
            "choose is gone: say the labels, and what each means, in system or message",
        ));
    }
    match env.get("fields") {
        Some(f) => Ok(Shape::Fields(fields(f)?)),
        None => Ok(Shape::Text),
    }
}

fn field_schema(t: &str) -> Value {
    match t {
        "date" => json!({ "type": "string", "format": "date" }),
        "text" => json!({ "type": "string" }),
        other => json!({ "type": other }),
    }
}

/// The JSON schema the answer is held to, if it has a form.
pub(super) fn schema(shape: &Shape) -> Option<Value> {
    match shape {
        Shape::Text => None,
        Shape::Fields(fs) => {
            let props: Map<String, Value> = fs
                .iter()
                .map(|(n, t)| (n.clone(), field_schema(t)))
                .collect();
            let required: Vec<&String> = fs.iter().map(|(n, _)| n).collect();
            Some(json!({ "type": "object", "properties": props,
                         "required": required, "additionalProperties": false }))
        }
    }
}

/// How the answer's form is said in words: a small model held to a form
/// by a grammar alone, without being told it, answers poorly.
pub(super) fn form_sentence(shape: &Shape) -> Option<String> {
    match shape {
        Shape::Text => None,
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

pub(super) fn number(env: &BTreeMap<String, String>, k: &str) -> Result<Option<Value>, String> {
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
    let model = env.get("model").ok_or_else(|| {
        fail("it needs a model: give the step $model, or itero ai defaults set '{\"model\": \"...\"}'")
    })?;
    body(model, env)
}

/// The request for `model`, by `#ai`'s rules on the args.
pub(super) fn body(model: &str, env: &BTreeMap<String, String>) -> Result<Value, String> {
    if env.contains_key("text") {
        return Err(fail("it takes message, not text"));
    }
    if let Some(url) = env.get("url") {
        if !url.starts_with('$') {
            super::remote::api(url)?;
        }
        if env.contains_key("context") {
            return Err(fail(
                "context is itero-ai's; a step with a url leaves it out",
            ));
        }
    } else if env.contains_key("key") {
        return Err(fail("key goes with a url"));
    }
    let text = env
        .get("message")
        .ok_or_else(|| fail("it needs $message"))?;
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
    if let Some(schema) = schema(&shape) {
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
    if let Some(t) = env.get("context") {
        let n: u32 = t
            .trim()
            .parse()
            .map_err(|_| fail(format!("context is a whole number, not {t}")))?;
        if n < 1024 {
            return Err(fail(format!("context is at least 1024, not {n}")));
        }
        r["context"] = json!(n);
    }
    Ok(r)
}

/// A step's model, as far as the workflow's text says.
#[derive(Debug, PartialEq)]
pub enum Written {
    Named(String),
    /// A `"$name"`: known only in a run.
    Later,
    Not,
}

/// What `itero ws check` learned of a step.
#[derive(Debug, PartialEq)]
pub struct Seen {
    pub model: Written,
    pub thinking_false: bool,
}

/// What `itero ws check` can say of an `#ai` step's args before a run: the
/// same rules as a run's, on the values written in the workflow; a
/// `"$name"` is known only in a run, and is left aside.
pub fn check(args: &Value) -> Result<Seen, String> {
    let items: Vec<&Map<String, Value>> = match args {
        Value::Object(o) => vec![o],
        Value::Array(a) => a.iter().filter_map(Value::as_object).collect(),
        // "$output" and the like: known only in a run
        _ => {
            return Ok(Seen {
                model: Written::Later,
                thinking_false: false,
            });
        }
    };
    let mut env = BTreeMap::new();
    for o in items {
        for (k, v) in o {
            let text = match v {
                Value::String(s)
                    if s.starts_with('$')
                        && !["model", "message", "system", "url", "key"].contains(&k.as_str()) =>
                {
                    continue;
                }
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            env.insert(k.clone(), text);
        }
    }
    let thinking_false = env.get("thinking").is_some_and(|t| t == "false");
    if env.contains_key("url") && !env.contains_key("model") {
        return Err(fail("a step with a url names its model"));
    }
    body(env.get("model").map_or("", String::as_str), &env)?;
    let model = match env.get("model") {
        None => Written::Not,
        Some(m) if m.starts_with('$') => Written::Later,
        Some(m) => Written::Named(m.clone()),
    };
    Ok(Seen {
        model,
        thinking_false,
    })
}

/// Where an `#ai` step sends, for `itero ws check`: itero-ai and its
/// model, or the url, local or remote. A `"$name"` is known only in a run.
pub fn place(args: &Value, machine: &Map<String, Value>) -> String {
    let items: Vec<&Map<String, Value>> = match args {
        Value::Object(o) => vec![o],
        Value::Array(a) => a.iter().filter_map(Value::as_object).collect(),
        _ => vec![],
    };
    let get = |k: &str| {
        items
            .iter()
            .find_map(|o| o.get(k))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let from_machine = |k: &str| machine.get(k).and_then(Value::as_str).map(String::from);
    match get("url").or_else(|| from_machine("url")) {
        Some(u) if u.starts_with('$') => "where: known in a run".into(),
        Some(u) if super::remote::is_local(&u) => format!("local: {u}"),
        Some(u) => format!("remote: {u}"),
        None => {
            let model = get("model")
                .or_else(|| from_machine("model"))
                .unwrap_or_else(|| "no model".into());
            format!("local: {model}")
        }
    }
}

/// What `itero ws check` warns of: only warnings, as another machine may
/// have what this one does not.
pub fn warnings(seen: &Seen) -> Vec<String> {
    let machine = super::defaults::path()
        .and_then(|p| super::defaults::read(&p))
        .unwrap_or_default();
    let model = match &seen.model {
        Written::Later => return vec![],
        Written::Named(m) => m.clone(),
        Written::Not => match machine.get("model").and_then(Value::as_str) {
            Some(m) => m.to_string(),
            None => {
                return vec![fail(
                    "no model: give the step $model, or itero ai defaults set '{\"model\": \"...\"}'",
                )];
            }
        },
    };
    let Ok(ix) = super::index_path().and_then(|p| itero_ai_index::Index::read(&p)) else {
        return vec![];
    };
    match itero_ai_index::resolve(&ix, &model) {
        Err(e) => vec![fail(format!("{e} (itero ai models list shows them)"))],
        Ok(e)
            if e.thinking_off == Some(false)
                && (seen.thinking_false || machine.get("thinking") == Some(&json!(false))) =>
        {
            vec![fail(format!(
                "{model} cannot turn thinking off: it may think anyway, and its thinking goes to the log"
            ))]
        }
        Ok(_) => vec![],
    }
}

/// The answer as `$output`: the text, or the fields' object.
pub(super) fn output(env: &BTreeMap<String, String>, content: &str) -> Result<Value, String> {
    match shape(env)? {
        Shape::Text => Ok(Value::String(content.trim().to_string())),
        Shape::Fields(_) => serde_json::from_str(content.trim()).map_err(|e| {
            fail(format!(
                "the model's answer is not the JSON asked for ({e}): {content}"
            ))
        }),
    }
}

/// Runs `#ai`: streams the answer, telling its token count as progress.
pub fn run(r: &StepRun, told: &(dyn Fn(Option<&str>) + Sync)) -> Result<Outcome, Failure> {
    let mut env = r.env.clone();
    let machine = super::defaults::path()
        .and_then(|p| super::defaults::read(&p))
        .map_err(fail)?;
    let ix = super::index_path()
        .and_then(|p| itero_ai_index::Index::read(&p))
        .map_err(fail)?;
    super::defaults::fill(&mut env, &machine, &ix);
    if let Some(url) = env.get("url").cloned() {
        if let Some(n) = env
            .get("key")
            .and_then(|k| k.strip_prefix("$$"))
            .map(String::from)
        {
            // a key from the machine's defaults: a $$NAME the step's
            // binding did not see
            let ws = crate::workspace::find(&r.root).map_err(fail)?;
            let v = crate::engine::secrets::get_in(&ws, &n).ok_or_else(|| {
                fail(format!(
                    "{n} is not set: no secret, not in .env, no environment variable"
                ))
            })?;
            env.insert("key".into(), v);
        }
        return super::remote::run(r, &env, &url, told);
    }
    let body = request(&env)?;
    let model = body["model"].as_str().unwrap_or("").to_string();
    let started = Instant::now();
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
    let (mut content, mut thinking, mut usage, mut n) =
        (String::new(), String::new(), Value::Null, 0u64);
    for data in sse(resp) {
        let data = data?;
        if data == "[DONE]" {
            break;
        }
        let v: Value =
            serde_json::from_str(&data).map_err(|e| fail(format!("a piece of the answer: {e}")))?;
        if !v["usage"].is_null() {
            usage = v["usage"].clone();
        }
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
    log_answer(
        r,
        "itero-ai",
        &model,
        started.elapsed(),
        &json!({ "model": model, "usage": usage,
                 "choices": [{ "index": 0, "message": { "role": "assistant", "content": content } }] }),
    );
    Ok(Outcome::Done(output(&r.env, &content)?))
}

/// The `data` of each server-sent event, as text: `data:` with or without
/// its space, comment lines (`:` first) skipped, `\r\n` line ends taken.
/// The lines of one event's data are joined by `\n`.
pub(super) fn sse(resp: impl std::io::Read) -> impl Iterator<Item = Result<String, String>> {
    let mut lines = std::io::BufReader::new(resp).lines();
    let mut data: Vec<String> = vec![];
    std::iter::from_fn(move || {
        loop {
            match lines.next() {
                None if data.is_empty() => return None,
                None => return Some(Ok(std::mem::take(&mut data).join("\n"))),
                Some(Err(e)) => return Some(Err(fail(format!("reading the answer: {e}")))),
                Some(Ok(l)) => {
                    let l = l.strip_suffix('\r').unwrap_or(&l);
                    if l.is_empty() {
                        if !data.is_empty() {
                            return Some(Ok(std::mem::take(&mut data).join("\n")));
                        }
                    } else if let Some(d) = l.strip_prefix("data:") {
                        data.push(d.strip_prefix(' ').unwrap_or(d).to_string());
                    }
                    // ":" comments, "event:", "id:" and "retry:" lines: nothing to keep
                }
            }
        }
    })
}

/// Whether the step asks itero-ai: it has no url, and the machine's
/// defaults give none.
pub fn needs_itero_ai(env: &BTreeMap<String, String>) -> Result<bool, String> {
    if env.contains_key("url") {
        return Ok(false);
    }
    let machine = super::defaults::path()
        .and_then(|p| super::defaults::read(&p))
        .map_err(fail)?;
    Ok(!machine.contains_key("url"))
}

/// Every call, as the server answered it, to the workspace's log.
pub(super) fn log_answer(r: &StepRun, at: &str, model: &str, took: Duration, response: &Value) {
    let Ok(ws) = crate::workspace::find(&r.root) else {
        return;
    };
    if let Ok(id) = crate::engine::journal::Journal::open(&ws).and_then(|j| j.workspace()) {
        crate::engine::log::Log::new(&ws, &id.id).write(
            "ai.answer",
            json!({ "workflow": r.workflow, "at": at, "model": model,
                    "seconds": took.as_secs_f64(), "response": response }),
        );
    }
}

/// Tells the workspace a model is missing: the `models` workflow looks
/// for models on it, or, for a workspace on the machine's list, the
/// machine's daemon (it watches the file this touches).
fn missing(r: &StepRun, model: &str) {
    if let Ok(ws) = crate::workspace::find(&r.root) {
        if ws.machine
            && let Ok(dir) = crate::machine::state_dir()
        {
            let _ = std::fs::write(
                dir.join(crate::engine::machine::MODELS_WANTED),
                crate::engine::daemon::stamp(chrono::Utc::now()),
            );
        }
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
    fn context_is_sent_as_given_and_checked() {
        let r = request(&env(&[
            ("model", "u/k"),
            ("message", "özetle"),
            ("context", "16384"),
        ]))
        .unwrap();
        assert_eq!(r["context"].as_u64(), Some(16384));
        for (v, says) in [
            ("uzun", "context is a whole number"),
            ("100", "at least 1024"),
        ] {
            let e =
                request(&env(&[("model", "u/k"), ("message", "x"), ("context", v)])).unwrap_err();
            assert!(e.contains(says), "{e}");
        }
    }

    #[test]
    fn a_step_without_a_model_says_how_to_give_one() {
        let e = request(&env(&[("message", "x")])).unwrap_err();
        assert!(e.contains("itero ai defaults set"), "{e}");
    }

    #[test]
    fn the_form_is_said_in_words_too() {
        let r = request(&env(&[
            ("model", "u/k"),
            ("message", "Fatura"),
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
        let r = request(&env(&[("model", "u/k"), ("message", "Özetle")])).unwrap();
        assert_eq!(r["messages"][0]["content"], "Özetle");
        assert!(r.get("response_format").is_none());
    }
}
