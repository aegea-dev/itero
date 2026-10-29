//! The OpenAI Chat Completions shapes itero-ai takes and gives. A field it
//! does not honour is refused by name, never silently ignored.
use crate::engine::{Finish, Message, Piece, Prompt, Reason, Sampling};
use serde_json::{Map, Value, json};

/// A request refused, in OpenAI's error shape.
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    /// The field at fault.
    pub param: Option<String>,
}

fn bad(param: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        status: 400,
        message: message.into(),
        param: Some(param.into()),
    }
}

/// The fields it reads; any other is refused by name. `top_k`, `min_p` and
/// `chat_template_kwargs` are not OpenAI's, but llama-server's and vLLM's.
const TAKEN: [&str; 17] = [
    "model",
    "messages",
    "max_tokens",
    "max_completion_tokens",
    "temperature",
    "top_p",
    "top_k",
    "min_p",
    "stop",
    "seed",
    "presence_penalty",
    "frequency_penalty",
    "stream",
    "stream_options",
    "response_format",
    "chat_template_kwargs",
    "n",
];

/// A request's body as a prompt, and whether it is to be streamed.
pub fn validate(raw: Value) -> Result<(Prompt, bool), ApiError> {
    let Value::Object(o) = raw else {
        return Err(bad("body", "the body is a JSON object"));
    };
    if let Some(k) = o.keys().find(|k| !TAKEN.contains(&k.as_str())) {
        return Err(bad(k, format!("{k} is not something itero-ai does")));
    }
    if o.get("n").is_some_and(|n| n.as_u64() != Some(1)) {
        return Err(bad("n", "itero-ai gives one answer: n is 1"));
    }
    let model = o
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("model", "model is needed: <org>/<repo>[:<QUANT>]"))?;
    let messages = o
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("messages", "messages is a list"))?
        .iter()
        .map(message)
        .collect::<Result<Vec<_>, _>>()?;
    let float = |k: &str| o.get(k).and_then(Value::as_f64).map(|v| v as f32);
    let sampling = Sampling {
        temperature: float("temperature"),
        top_p: float("top_p"),
        top_k: o.get("top_k").and_then(Value::as_i64).map(|v| v as i32),
        min_p: float("min_p"),
        presence_penalty: float("presence_penalty"),
        frequency_penalty: float("frequency_penalty"),
        seed: o.get("seed").and_then(Value::as_u64).map(|v| v as u32),
    };
    let max_tokens = o
        .get("max_completion_tokens")
        .or(o.get("max_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let stop = match o.get("stop") {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => a
            .iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect(),
        Some(_) => return Err(bad("stop", "stop is a text or a list of texts")),
    };
    let (schema, json) = match o.get("response_format") {
        None => (None, false),
        Some(f) => match f.get("type").and_then(Value::as_str) {
            Some("text") => (None, false),
            Some("json_object") => (None, true),
            Some("json_schema") => {
                let s = f.pointer("/json_schema/schema").cloned().ok_or_else(|| {
                    bad("response_format", "json_schema needs json_schema.schema")
                })?;
                (Some(s), true)
            }
            other => {
                return Err(bad(
                    "response_format",
                    format!(
                        "response_format's type is text, json_object or json_schema, not {other:?}"
                    ),
                ));
            }
        },
    };
    if let Some(Value::Object(k)) = o.get("chat_template_kwargs")
        && let Some(other) = k.keys().find(|k| *k != "enable_thinking")
    {
        return Err(bad(
            "chat_template_kwargs",
            format!("{other} is not something itero-ai does: it takes enable_thinking"),
        ));
    }
    let thinking = o
        .get("chat_template_kwargs")
        .and_then(|k| k.get("enable_thinking"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let stream = o.get("stream").and_then(Value::as_bool).unwrap_or(false);
    Ok((
        Prompt {
            model: model.into(),
            messages,
            sampling,
            max_tokens,
            stop,
            schema,
            json,
            thinking,
        },
        stream,
    ))
}

fn message(m: &Value) -> Result<Message, ApiError> {
    let role = m.get("role").and_then(Value::as_str).unwrap_or("");
    if !["system", "user", "assistant"].contains(&role) {
        return Err(bad(
            "messages",
            format!("a message's role is system, user or assistant, not {role:?}"),
        ));
    }
    let content = m.get("content").and_then(Value::as_str).ok_or_else(|| {
        bad(
            "messages",
            "a message's content is text: images and audio are not something itero-ai does",
        )
    })?;
    Ok(Message {
        role: role.into(),
        content: content.into(),
    })
}

fn reason(f: &Finish) -> &'static str {
    match f.reason {
        Reason::Stop => "stop",
        Reason::Length => "length",
    }
}

fn usage(f: &Finish) -> Value {
    json!({
        "prompt_tokens": f.prompt_tokens,
        "completion_tokens": f.completion_tokens,
        "total_tokens": f.prompt_tokens + f.completion_tokens,
    })
}

/// A whole answer: `chat.completion`.
pub fn completion(
    id: &str,
    created: u64,
    model: &str,
    content: &str,
    reasoning: &str,
    f: &Finish,
) -> Value {
    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert("content".into(), json!(content));
    if !reasoning.is_empty() {
        message.insert("reasoning_content".into(), json!(reasoning));
    }
    json!({
        "id": id, "object": "chat.completion", "created": created, "model": model,
        "choices": [{ "index": 0, "message": message, "finish_reason": reason(f) }],
        "usage": usage(f),
    })
}

/// A piece of a streamed answer, `chat.completion.chunk`; the last one has
/// no piece, its finish reason and the usage.
pub fn chunk(
    id: &str,
    created: u64,
    model: &str,
    piece: Option<&Piece>,
    finish: Option<&Finish>,
) -> Value {
    let delta = match piece {
        Some(Piece::Content(t)) => json!({ "content": t }),
        Some(Piece::Reasoning(t)) => json!({ "reasoning_content": t }),
        None => json!({}),
    };
    let mut v = json!({
        "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish.map(reason) }],
    });
    if let Some(f) = finish {
        v["usage"] = usage(f);
    }
    v
}

/// OpenAI's error body.
pub fn error_body(e: &ApiError) -> Value {
    json!({ "error": { "message": e.message, "type": "invalid_request_error", "param": e.param } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> serde_json::Value {
        json!({ "model": "u/küçük", "messages": [{ "role": "user", "content": "merhaba" }] })
    }

    #[test]
    fn a_plain_request_is_a_prompt() {
        let (p, stream) = validate(base()).unwrap();
        assert_eq!(p.model, "u/küçük");
        assert_eq!(p.messages[0].content, "merhaba");
        assert!(!stream && !p.thinking && !p.json && p.schema.is_none());
    }

    #[test]
    fn what_it_does_not_honour_is_refused_by_name() {
        for (k, v) in [
            ("n", json!(2)),
            ("tools", json!([])),
            ("logprobs", json!(true)),
            ("şey", json!(1)),
        ] {
            let mut r = base();
            r[k] = v;
            let e = validate(r).unwrap_err();
            assert_eq!((e.status, e.param.as_deref()), (400, Some(k)), "{k}");
            assert!(e.message.contains(k), "{}", e.message);
        }
        let mut r = base();
        r["n"] = json!(1);
        assert!(validate(r).is_ok(), "n of 1 is what it does");
        let mut r = base();
        r["chat_template_kwargs"] = json!({ "enable_thinking": true, "başka": 1 });
        assert_eq!(
            validate(r).unwrap_err().param.as_deref(),
            Some("chat_template_kwargs")
        );
    }

    #[test]
    fn what_a_message_holds_is_checked() {
        let mut r = base();
        r["messages"][0]["content"] = json!([{ "type": "image_url", "image_url": { "url": "x" } }]);
        let e = validate(r).unwrap_err();
        assert_eq!(e.param.as_deref(), Some("messages"));
        assert!(e.message.contains("images"), "{}", e.message);
        let mut r = base();
        r["messages"][0]["role"] = json!("tool");
        assert_eq!(validate(r).unwrap_err().param.as_deref(), Some("messages"));
        let mut r = base();
        r.as_object_mut().unwrap().remove("model");
        assert_eq!(validate(r).unwrap_err().param.as_deref(), Some("model"));
    }

    #[test]
    fn thinking_schemas_and_sampling_are_read() {
        let mut r = base();
        r["chat_template_kwargs"] = json!({ "enable_thinking": true });
        r["response_format"] = json!({ "type": "json_schema",
            "json_schema": { "name": "x", "schema": { "type": "object" } } });
        r["top_k"] = json!(20);
        r["temperature"] = json!(0.7);
        r["max_completion_tokens"] = json!(64);
        r["max_tokens"] = json!(8);
        r["stop"] = json!("bitti");
        r["stream"] = json!(true);
        let (p, stream) = validate(r).unwrap();
        assert!(p.thinking && stream && p.json);
        assert_eq!(p.schema, Some(json!({ "type": "object" })));
        assert_eq!((p.sampling.top_k, p.max_tokens), (Some(20), Some(64)));
        assert_eq!(p.sampling.temperature, Some(0.7));
        assert_eq!(p.stop, vec!["bitti".to_string()]);
        let mut r = base();
        r["response_format"] = json!({ "type": "json_object" });
        let (p, _) = validate(r).unwrap();
        assert!(p.json && p.schema.is_none());
        let mut r = base();
        r["response_format"] = json!({ "type": "xml" });
        assert_eq!(
            validate(r).unwrap_err().param.as_deref(),
            Some("response_format")
        );
    }

    #[test]
    fn answers_and_chunks_are_openais_shapes() {
        let f = Finish {
            reason: Reason::Length,
            prompt_tokens: 5,
            completion_tokens: 2,
        };
        let v = completion("c1", 7, "u/k", "cevap", "", &f);
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["content"], "cevap");
        assert!(
            v["choices"][0]["message"]
                .get("reasoning_content")
                .is_none()
        );
        assert_eq!(v["choices"][0]["finish_reason"], "length");
        assert_eq!(v["usage"]["total_tokens"], 7);
        let c = chunk(
            "c1",
            7,
            "u/k",
            Some(&Piece::Reasoning("düşün".into())),
            None,
        );
        assert_eq!(c["choices"][0]["delta"]["reasoning_content"], "düşün");
        assert!(c.get("usage").is_none());
        let last = chunk("c1", 7, "u/k", None, Some(&f));
        assert_eq!(last["choices"][0]["finish_reason"], "length");
        assert_eq!(last["usage"]["completion_tokens"], 2);
    }
}
