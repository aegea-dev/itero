//! `#ai` for a model behind a `url`: OpenAI's Chat Completions or
//! Responses API, on this machine or another, streamed and read into one
//! answer.
use super::step::{fail, form_sentence, number, output, schema, shape};
use crate::engine::worker::{Failure, Outcome, StepRun};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Which API a `url` asks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Api {
    Chat,
    Responses,
}

/// The API, by the end of the url's path.
pub fn api(url: &str) -> Result<Api, String> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    if path.ends_with("/chat/completions") {
        Ok(Api::Chat)
    } else if path.ends_with("/responses") {
        Ok(Api::Responses)
    } else {
        Err(fail(format!(
            "url ends in /chat/completions or /responses, not {url}"
        )))
    }
}

/// Whether the url is on this machine.
pub fn is_local(url: &str) -> bool {
    reqwest::Url::parse(url).ok().is_some_and(|u| {
        u.host_str()
            .is_some_and(|h| matches!(h, "localhost" | "127.0.0.1" | "[::1]"))
    })
}

/// The lowest efforts, in the order tried for `thinking: false`.
const EFFORTS: [&str; 3] = ["none", "minimal", "low"];

/// What a server refused for a model, learned from its answers.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Learned {
    #[serde(default)]
    pub no_schema: bool,
    #[serde(default)]
    pub efforts_refused: Vec<String>,
    #[serde(default)]
    pub no_stream: bool,
}

/// The request: only what the step and the machine's defaults set.
pub fn body(
    api: Api,
    model: &str,
    env: &BTreeMap<String, String>,
    l: &Learned,
) -> Result<Value, String> {
    let message = env
        .get("message")
        .ok_or_else(|| fail("it needs $message"))?;
    let shape = shape(env)?;
    let content = match form_sentence(&shape) {
        Some(s) => format!("{message}\n\n{s}"),
        None => message.clone(),
    };
    let mut r = match api {
        Api::Chat => {
            let mut messages = vec![];
            if let Some(s) = env.get("system") {
                messages.push(json!({ "role": "system", "content": s }));
            }
            messages.push(json!({ "role": "user", "content": content }));
            json!({ "model": model, "messages": messages })
        }
        Api::Responses => {
            let mut r = json!({ "model": model, "input": content });
            if let Some(s) = env.get("system") {
                r["instructions"] = json!(s);
            }
            r
        }
    };
    if let (Some(schema), false) = (schema(&shape), l.no_schema) {
        match api {
            Api::Chat => {
                r["response_format"] = json!({ "type": "json_schema",
                    "json_schema": { "name": "answer", "schema": schema } })
            }
            Api::Responses => {
                r["text"] = json!({ "format": { "type": "json_schema",
                    "name": "answer", "schema": schema } })
            }
        }
    }
    match env.get("thinking").map(String::as_str) {
        None | Some("true") => {}
        Some("false") => {
            if let Some(e) = EFFORTS
                .iter()
                .find(|e| !l.efforts_refused.iter().any(|r| r == *e))
            {
                match api {
                    Api::Chat => r["reasoning_effort"] = json!(e),
                    Api::Responses => r["reasoning"] = json!({ "effort": e }),
                }
            }
        }
        Some(o) => return Err(fail(format!("thinking is true or false, not {o}"))),
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
        let k = match api {
            Api::Chat => "max_tokens",
            Api::Responses => "max_output_tokens",
        };
        r[k] = v;
    }
    Ok(r)
}

/// The answer's text, from either API's response.
pub fn answer_text(api: Api, v: &Value) -> Result<String, String> {
    let text = match api {
        Api::Chat => v["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from),
        Api::Responses => {
            let parts: Vec<&str> = v["output"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|o| o["type"] == "message")
                .flat_map(|o| o["content"].as_array().into_iter().flatten())
                .filter(|c| c["type"] == "output_text")
                .filter_map(|c| c["text"].as_str())
                .collect();
            (!parts.is_empty()).then(|| parts.concat())
        }
    };
    text.ok_or_else(|| fail(format!("the server's answer has no text: {v}")))
}

/// The streamed answer read into one response, as the server would have
/// answered without streaming, telling the pieces as progress.
fn read_stream(
    api: Api,
    resp: reqwest::blocking::Response,
    told: &(dyn Fn(Option<&str>) + Sync),
) -> Result<Value, String> {
    let (mut text, mut usage, mut last, mut n) = (String::new(), Value::Null, Value::Null, 0u64);
    for data in super::step::sse(resp) {
        let data = data?;
        if data == "[DONE]" {
            break;
        }
        let v: Value =
            serde_json::from_str(&data).map_err(|e| fail(format!("a piece of the answer: {e}")))?;
        if let Some(m) = v["error"]["message"].as_str() {
            return Err(fail(m));
        }
        let piece = match api {
            Api::Chat => v["choices"][0]["delta"]["content"].as_str(),
            Api::Responses if v["type"] == "response.output_text.delta" => v["delta"].as_str(),
            Api::Responses => None,
        };
        if let Some(t) = piece {
            text.push_str(t);
            n += 1;
            if n % 16 == 0 {
                told(Some(&format!("[{n} tokens]")));
            } else {
                told(None);
            }
        }
        if api == Api::Responses && v["type"] == "response.completed" {
            usage = v["response"]["usage"].clone();
            last = v["response"].clone();
        } else if api == Api::Chat {
            if !v["usage"].is_null() {
                usage = v["usage"].clone();
            }
            last = v;
        }
    }
    let mut whole = match api {
        Api::Chat => json!({ "object": "chat.completion",
            "choices": [{ "index": 0, "message": { "role": "assistant", "content": text } }] }),
        Api::Responses => json!({ "object": "response",
            "output": [{ "type": "message", "role": "assistant",
                         "content": [{ "type": "output_text", "text": text }] }] }),
    };
    for k in ["id", "model", "created", "system_fingerprint", "status"] {
        if !last[k].is_null() {
            whole[k] = last[k].clone();
        }
    }
    whole["usage"] = usage;
    Ok(whole)
}

/// Asks the model behind `url`, streamed.
pub fn run(
    r: &StepRun,
    env: &BTreeMap<String, String>,
    url: &str,
    told: &(dyn Fn(Option<&str>) + Sync),
) -> Result<Outcome, Failure> {
    let api = api(url)?;
    let model = env
        .get("model")
        .ok_or_else(|| fail("a step with a url names its model"))?;
    let client = crate::net::client(Duration::from_secs(r.most.max(1))).map_err(fail)?;
    let l = Learned::default();
    let mut body = body(api, model, env, &l)?;
    if !l.no_stream {
        body["stream"] = json!(true);
        if api == Api::Chat {
            body["stream_options"] = json!({ "include_usage": true });
        }
    }
    let started = Instant::now();
    let mut req = client.post(url).json(&body);
    if let Some(k) = env.get("key") {
        req = req.bearer_auth(k);
    }
    let resp = req
        .send()
        .map_err(|e| fail(format!("{url} did not answer: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let v: Value = resp.json().unwrap_or_default();
        super::step::log_answer(r, url, model, started.elapsed(), &v);
        let why = v["error"]["message"].as_str().unwrap_or("no reason given");
        return Err(fail(format!("{url} answered {status}: {why}")).into());
    }
    let v = if body["stream"] == true {
        read_stream(api, resp, told)?
    } else {
        resp.json()
            .map_err(|e| fail(format!("reading the answer: {e}")))?
    };
    super::step::log_answer(r, url, model, started.elapsed(), &v);
    Ok(Outcome::Done(output(env, &answer_text(api, &v)?)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_picks_the_api_and_the_host_says_where() {
        assert_eq!(
            api("https://api.openai.com/v1/chat/completions"),
            Ok(Api::Chat)
        );
        assert_eq!(
            api("http://localhost:1234/v1/responses/"),
            Ok(Api::Responses)
        );
        let e = api("https://openrouter.ai/api/v1").unwrap_err();
        assert!(e.contains("/chat/completions or /responses"), "{e}");
        for (url, local) in [
            ("http://localhost:1234/v1/chat/completions", true),
            ("http://127.0.0.1:8080/v1/responses", true),
            ("http://[::1]:8080/v1/responses", true),
            ("http://192.168.1.20:1234/v1/chat/completions", false),
            ("https://api.openai.com/v1/chat/completions", false),
        ] {
            assert_eq!(is_local(url), local, "{url}");
        }
    }
}
