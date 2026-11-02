//! `#ai` with a `url`: what a server refuses for a model is learned, the
//! request sent again without it, and left out from then on. Its own test
//! binary: it sets XDG_STATE_HOME and XDG_CACHE_HOME for the process.
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
fn what_a_server_refuses_is_learned_and_left_out() {
    let d = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("XDG_STATE_HOME", d.path().join("durum"));
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
    }
    let ws = d.path().join("çalışma");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("itero.config.json"), "{}").unwrap();

    // the form refused: the same request again in words, then remembered
    let s = serve_http(vec![
        Reply::Refuse("response_format"),
        Reply::Text(r#"{"tutar": 3}"#.into()),
    ]);
    let url = format!("{}/chat/completions", s.url);
    let args = [
        ("url", url.as_str()),
        ("model", "küçük"),
        ("message", "fatura"),
        ("fields", r#"{"tutar": "number"}"#),
    ];
    assert_eq!(
        worker::run_step(&step(&ws, &args)).unwrap(),
        Outcome::Done(json!({ "tutar": 3 }))
    );
    {
        let reqs = s.requests.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        assert!(reqs[0].0.get("response_format").is_some());
        assert!(reqs[1].0.get("response_format").is_none());
        assert!(
            reqs[1].0["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("JSON object")
        );
    }
    worker::run_step(&step(&ws, &args)).unwrap();
    assert!(
        s.requests.lock().unwrap()[2]
            .0
            .get("response_format")
            .is_none(),
        "remembered"
    );

    // thinking false: none refused, then minimal sent
    let s = serve_http(vec![
        Reply::Refuse("reasoning_effort"),
        Reply::Text("özet".into()),
    ]);
    let url = format!("{}/chat/completions", s.url);
    worker::run_step(&step(
        &ws,
        &[
            ("url", &url),
            ("model", "düşünen"),
            ("message", "x"),
            ("thinking", "false"),
        ],
    ))
    .unwrap();
    {
        let reqs = s.requests.lock().unwrap();
        assert_eq!(reqs[0].0["reasoning_effort"], "none");
        assert_eq!(reqs[1].0["reasoning_effort"], "minimal");
    }

    // streaming refused: asked again without it, the whole answer read
    let s2 = serve_http(vec![Reply::Refuse("stream"), Reply::Text("özet".into())]);
    let url2 = format!("{}/chat/completions", s2.url);
    let out = worker::run_step(&step(
        &ws,
        &[("url", &url2), ("model", "akışsız"), ("message", "x")],
    ))
    .unwrap();
    assert_eq!(out, Outcome::Done(json!("özet")));
    {
        let reqs = s2.requests.lock().unwrap();
        assert_eq!(reqs[0].0["stream"], true);
        assert!(reqs[1].0.get("stream").is_none());
        assert!(reqs[1].0.get("stream_options").is_none());
    }

    // a refusal of anything else is a failure, not learned
    let s3 = serve_http(vec![Reply::Refuse("temperature")]);
    let err = worker::run_step(&step(
        &ws,
        &[
            ("url", &format!("{}/chat/completions", s3.url)),
            ("model", "m"),
            ("message", "x"),
            ("temperature", "0.2"),
        ],
    ))
    .unwrap_err();
    assert!(
        format!("{err:?}").contains("temperature desteklenmiyor"),
        "{err:?}"
    );
    assert_eq!(s3.requests.lock().unwrap().len(), 1);

    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(d.path().join("durum/itero/ai-servers.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        saved[format!("{url} düşünen")]["efforts_refused"],
        json!(["none"])
    );
    assert_eq!(saved[format!("{url2} akışsız")]["no_stream"], true);
}
