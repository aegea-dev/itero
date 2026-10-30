//! The built-in `#ai`, against a stand-in for itero-ai on a socket.
#![cfg(unix)]
#[path = "common/fake_ai.rs"]
mod fake_ai;
use itero::engine::worker::{self, Outcome, StepRun};
use serde_json::{Map, json};
use std::path::{Path, PathBuf};

fn step(root: &Path, sock: &Path, env: &[(&str, &str)]) -> StepRun {
    let mut e: std::collections::BTreeMap<String, String> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    e.insert("ITERO_AI_SOCKET".into(), sock.display().to_string());
    StepRun {
        file: PathBuf::from(worker::AI),
        root: root.into(),
        workflow: "posta".into(),
        env: e,
        input: Map::new(),
        memory: root.join("hafıza"),
        waiting_since: None,
        quiet: 30,
        most: 60,
        now: String::new(),
    }
}

#[test]
fn a_plain_answer_is_the_output_and_the_thinking_is_not() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ai.sock");
    let s = fake_ai::serve(&sock, Some("  kısa özet  "));
    let out = worker::run_step(&step(
        d.path(),
        &sock,
        &[("model", "u/küçük"), ("text", "özetle: uzun mektup")],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!("kısa özet")));
    let req = &s.requests.lock().unwrap()[0];
    assert_eq!(req["model"], "u/küçük");
    assert_eq!(
        req["messages"][0],
        json!({ "role": "user", "content": "özetle: uzun mektup" })
    );
    assert_eq!(req["stream"], true);
}

#[test]
fn choose_is_a_schema_of_its_labels_and_the_output_the_label() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ai.sock");
    let s = fake_ai::serve(&sock, Some("\"acil\""));
    let out = worker::run_step(&step(
        d.path(),
        &sock,
        &[
            ("model", "u/k"),
            ("text", "x"),
            ("choose", r#"["acil","sonra"]"#),
        ],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!("acil")));
    assert_eq!(
        s.requests.lock().unwrap()[0]["response_format"]["json_schema"]["schema"],
        json!({ "type": "string", "enum": ["acil", "sonra"] })
    );
}

#[test]
fn fields_are_an_object_schema_and_the_output_the_object() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ai.sock");
    let s = fake_ai::serve(&sock, Some(r#"{"tutar": 1250, "son": "2026-11-15"}"#));
    let out = worker::run_step(&step(
        d.path(),
        &sock,
        &[
            ("model", "u/k"),
            ("text", "fatura"),
            ("fields", r#"{"tutar":"number","son":"date"}"#),
            ("temperature", "0.7"),
            ("thinking", "true"),
        ],
    ))
    .unwrap();
    assert_eq!(
        out,
        Outcome::Done(json!({ "tutar": 1250, "son": "2026-11-15" }))
    );
    let req = &s.requests.lock().unwrap()[0];
    let schema = &req["response_format"]["json_schema"]["schema"];
    assert_eq!(
        schema["properties"]["son"],
        json!({ "type": "string", "format": "date" })
    );
    assert_eq!(schema["required"], json!(["tutar", "son"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(req["temperature"], 0.7);
    assert_eq!(req["chat_template_kwargs"]["enable_thinking"], true);
}

#[test]
fn what_is_wrong_is_said() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ai.sock");
    let _s = fake_ai::serve(&sock, Some("x"));
    for (env, says) in [
        (vec![("text", "x")], "$model"),
        (vec![("model", "u/k")], "$text"),
        (
            vec![("model", "u/k"), ("text", "x"), ("choose", "[]")],
            "choose",
        ),
        (
            vec![
                ("model", "u/k"),
                ("text", "x"),
                ("fields", r#"{"a":"renk"}"#),
            ],
            "renk",
        ),
        (
            vec![
                ("model", "u/k"),
                ("text", "x"),
                ("choose", r#"["a"]"#),
                ("fields", r#"{"a":"text"}"#),
            ],
            "not both",
        ),
        (
            vec![("model", "u/k"), ("text", "x"), ("temperature", "sıcak")],
            "temperature",
        ),
        (
            vec![("model", "u/k"), ("text", "x"), ("thinking", "belki")],
            "thinking",
        ),
    ] {
        let e = worker::run_step(&step(d.path(), &sock, &env))
            .unwrap_err()
            .to_string();
        assert!(e.contains(says) && e.starts_with("#ai: "), "{env:?}: {e}");
    }
}

#[test]
fn a_missing_model_is_told_to_the_workspace() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let sock = d.path().join("ai.sock");
    let _s = fake_ai::serve(&sock, None);
    let e = worker::run_step(&step(d.path(), &sock, &[("model", "u/yok"), ("text", "x")]))
        .unwrap_err()
        .to_string();
    assert!(e.contains("no model u/yok"), "{e}");
    assert!(e.contains("itero ai models list"), "{e}");
    let ws = itero::workspace::find(d.path()).unwrap();
    let j = itero::engine::journal::Journal::open(&ws).unwrap();
    let kinds: Vec<String> = j
        .events(None)
        .unwrap()
        .into_iter()
        .map(|r| r.event.kind)
        .collect();
    assert!(
        kinds.contains(&"itero.ai.model.missing".to_string()),
        "{kinds:?}"
    );
}
