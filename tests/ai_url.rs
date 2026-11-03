//! `#ai` with a `url`: the request for each API, the key, the answer, the
//! failures, and the `ai.answer` log line. Its own test binary: it sets
//! XDG_STATE_HOME and XDG_CACHE_HOME for the process, so no machine's
//! defaults or models are read.
#![cfg(unix)]
#[path = "common/fake_ai.rs"]
mod fake_ai;
use fake_ai::{Reply, serve_http};
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

fn step(root: &Path, env: &[(&str, &str)]) -> StepRun {
    StepRun {
        file: PathBuf::from(worker::AI),
        root: root.into(),
        workflow: "posta".into(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        input: Map::new(),
        memory: root.join("hafıza"),
        waiting_since: None,
        quiet: 30,
        most: 30,
        now: String::new(),
    }
}

#[test]
fn a_url_step_asks_the_model_behind_it() {
    let d = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("XDG_STATE_HOME", d.path().join("durum"));
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
    }
    let ws = d.path().join("çalışma");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("itero.config.json"), "{}").unwrap();

    // Chat Completions: system and message, the key as a bearer token,
    // streamed with usage, nothing of itero-ai's sent
    let s = serve_http(vec![Reply::Text("  kısa özet ".into())]);
    let url = format!("{}/chat/completions", s.url);
    let out = worker::run_step(&step(
        &ws,
        &[
            ("url", &url),
            ("key", "gizli-anahtar"),
            ("model", "openai/gpt-x"),
            ("system", "Postayı sırala."),
            ("message", "uzun mektup"),
        ],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!("kısa özet")));
    let (body, auth) = s.requests.lock().unwrap()[0].clone();
    assert_eq!(auth.as_deref(), Some("Bearer gizli-anahtar"));
    assert_eq!(body["model"], "openai/gpt-x");
    assert_eq!(
        body["messages"],
        json!([
            { "role": "system", "content": "Postayı sırala." },
            { "role": "user", "content": "uzun mektup" }])
    );
    assert_eq!(body["stream"], true);
    assert!(
        body.get("store").is_none(),
        "Chat Completions keeps none by default"
    );
    assert_eq!(body["stream_options"], json!({ "include_usage": true }));
    for k in ["top_k", "min_p", "context", "chat_template_kwargs"] {
        assert!(body.get(k).is_none(), "{k} sent: {body}");
    }

    // Responses: instructions and input; fields as text.format, checked
    let s = serve_http(vec![Reply::Text(r#"{"tutar": 12.5}"#.into())]);
    let url = format!("{}/responses", s.url);
    let out = worker::run_step(&step(
        &ws,
        &[
            ("url", &url),
            ("model", "gpt-x"),
            ("system", "Faturayı oku."),
            ("message", "fatura"),
            ("fields", r#"{"tutar": "number"}"#),
            ("thinking", "true"),
        ],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!({ "tutar": 12.5 })));
    let (body, auth) = s.requests.lock().unwrap()[0].clone();
    assert_eq!(auth, None, "no key, no header");
    assert_eq!(body["instructions"], "Faturayı oku.");
    assert!(body["input"].as_str().unwrap().starts_with("fatura"));
    assert_eq!(body["text"]["format"]["type"], "json_schema");
    assert!(
        body.get("reasoning").is_none(),
        "thinking true sends nothing"
    );
    assert_eq!(body["stream"], true);
    // the server keeps no copy: itero logs its own (ai.answer)
    assert_eq!(body["store"], false);

    // an answer not in the form fails the step
    let s = serve_http(vec![Reply::Text("on iki buçuk".into())]);
    let err = worker::run_step(&step(
        &ws,
        &[
            ("url", &format!("{}/chat/completions", s.url)),
            ("model", "m"),
            ("message", "fatura"),
            ("fields", r#"{"tutar": "number"}"#),
        ],
    ))
    .unwrap_err();
    assert!(
        format!("{err:?}").contains("not the JSON asked for"),
        "{err:?}"
    );

    // a 429 and a 500 fail the step with the server's status and message
    for code in [429, 500] {
        let s = serve_http(vec![Reply::Status(code)]);
        let err = worker::run_step(&step(
            &ws,
            &[
                ("url", &format!("{}/chat/completions", s.url)),
                ("model", "m"),
                ("message", "x"),
            ],
        ))
        .unwrap_err();
        let e = format!("{err:?}");
        assert!(e.contains(&code.to_string()) && e.contains("sunucu"), "{e}");
        assert_eq!(s.requests.lock().unwrap().len(), 1, "no retry");
    }

    // no model: none is sent, and the server answers with its default
    let s = serve_http(vec![Reply::Text("varsayılan".into())]);
    let out = worker::run_step(&step(
        &ws,
        &[
            ("url", &format!("{}/chat/completions", s.url)),
            ("message", "x"),
        ],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!("varsayılan")));
    let (body, _) = s.requests.lock().unwrap()[0].clone();
    assert!(body.get("model").is_none(), "{body}");

    // every call is an ai.answer line in the log, the whole response in it
    let log = std::fs::read_to_string(ws.join(".itero/itero.log")).unwrap();
    let answers: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|v: &Value| v["event"] == "ai.answer")
        .collect();
    assert_eq!(answers.len(), 6, "{log}");
    assert_eq!(answers[5]["model"], Value::Null, "none asked for");
    assert_eq!(
        answers[0]["response"]["usage"]["prompt_tokens_details"]["cached_tokens"],
        2
    );
    assert_eq!(
        answers[1]["response"]["usage"]["prompt_tokens"], 7,
        "from response.completed"
    );
    assert!(
        answers[0]["at"]
            .as_str()
            .unwrap()
            .ends_with("/chat/completions")
    );
}
