//! `itero check` knows `#ai`: the rules of a run, on what the workflow
//! writes, and a warning for a model this machine does not have.
#![cfg(unix)]
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn check(steps: Value) -> (bool, String) {
    check_with(steps, None)
}

fn check_with(steps: Value, defaults: Option<Value>) -> (bool, String) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    std::fs::write(
        d.path().join("workflows/posta.json"),
        json!({ "on": [{ "event": "posta.geldi" }], "steps": steps }).to_string(),
    )
    .unwrap();
    let cache = d.path().join("önbellek");
    let state = d.path().join("durum");
    if let Some(v) = defaults {
        std::fs::create_dir_all(state.join("itero")).unwrap();
        std::fs::write(state.join("itero/ai-defaults.json"), v.to_string()).unwrap();
    }
    let model = |repo: &str, thinking_off: bool| itero_ai_index::Entry {
        name: format!("{repo}:Q4_K_M"),
        repo: repo.into(),
        quant: "Q4_K_M".into(),
        paths: vec!["/m.gguf".into()],
        maker: "u".into(),
        params: None,
        architecture: "qwen35".into(),
        context: None,
        bytes: 0,
        licence: "apache-2.0".into(),
        chat: true,
        runs: true,
        thinking: true,
        basename: None,
        sampling: None,
        thinking_off: Some(thinking_off),
        modified: 0,
        downloaded: None,
        removed: None,
    };
    itero_ai_index::Index {
        models: vec![model("u/var", true), model("u/düşünen", false)],
    }
    .write(&cache.join("itero/models.json"))
    .unwrap();
    let out = Command::new(Path::new(env!("CARGO_BIN_EXE_itero")))
        .args(["ws", "check"])
        .current_dir(d.path())
        .env("XDG_CACHE_HOME", &cache)
        .env("XDG_STATE_HOME", &state)
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
}

fn ai(args: Value) -> Value {
    json!([{ "#ai": { "args": args } }])
}

#[test]
fn a_good_step_passes_and_a_missing_model_is_warned_of() {
    let (ok, out) = check(ai(json!({ "model": "u/var", "message": "$output.body",
                                     "fields": { "acil": "boolean" } })));
    assert!(ok && !out.contains("warning"), "{out}");
    let (ok, out) = check(ai(json!({ "model": "u/yok", "message": "x" })));
    assert!(
        ok && out.contains("warning: no model u/yok on this machine (")
            && out.contains("/workflow/posta/steps/#ai)"),
        "{out}"
    );
    // a model given as a value is known only in a run
    let (ok, out) = check(ai(
        json!({ "model": "$event.model", "message": "x", "fields": "$event.alanlar" }),
    ));
    assert!(ok && !out.contains("warning"), "{out}");
}

#[test]
fn a_step_without_a_model_is_warned_of_unless_the_machine_has_one() {
    let (ok, out) = check(ai(json!({ "message": "x" })));
    assert!(
        ok && out.contains("warning: no model") && out.contains("/workflow/posta/steps/#ai)"),
        "{out}"
    );
    let (ok, out) = check_with(
        ai(json!({ "message": "x" })),
        Some(json!({ "model": "u/var" })),
    );
    assert!(ok && !out.contains("warning"), "{out}");
    let (ok, out) = check_with(
        ai(json!({ "message": "x" })),
        Some(json!({ "model": "u/yok" })),
    );
    assert!(
        ok && out.contains("no model u/yok on this machine"),
        "{out}"
    );
}

#[test]
fn thinking_off_on_a_model_that_cannot_is_warned_of() {
    let (ok, out) = check(ai(
        json!({ "model": "u/düşünen", "message": "x", "thinking": false }),
    ));
    assert!(ok && out.contains("cannot turn thinking off"), "{out}");
    let (ok, out) = check(ai(json!({ "model": "u/düşünen", "message": "x" })));
    assert!(
        ok && !out.contains("warning"),
        "says nothing, asks nothing: {out}"
    );
    let (ok, out) = check(ai(
        json!({ "model": "u/var", "message": "x", "thinking": false }),
    ));
    assert!(ok && !out.contains("warning"), "{out}");
}

#[test]
fn what_a_run_would_refuse_is_an_error() {
    for (args, says) in [
        (json!({ "model": "u/var" }), "it needs $message"),
        (
            json!({ "model": "u/var", "text": "x" }),
            "it takes message, not text",
        ),
        (
            json!({ "model": "u/var", "message": "x", "choose": ["acil", "sonra"] }),
            "choose is gone",
        ),
        (
            json!({ "model": "u/var", "message": "x", "fields": { "a": "renk" } }),
            "not renk",
        ),
    ] {
        let (ok, out) = check(ai(args.clone()));
        assert!(!ok && out.contains(says), "{args}: {out}");
    }
}

#[test]
fn an_arg_it_does_not_read_is_warned_of() {
    let (ok, out) = check(ai(
        json!({ "model": "u/var", "message": "x", "renk": "mavi" }),
    ));
    assert!(
        ok && out.contains("#ai takes url, key, model, message,") && out.contains("not renk"),
        "{out}"
    );
}

#[test]
fn check_says_where_each_ai_step_sends_and_refuses_a_bad_url() {
    let (ok, out) = check(json!([
        { "#ai": { "args": { "model": "u/var", "message": "a" } } },
        { "#ai": { "args": { "url": "http://localhost:1234/v1/chat/completions",
                             "model": "qwen", "message": "b" } } },
        { "#ai": { "args": { "url": "https://api.openai.com/v1/responses",
                             "key": "$$OPENAI_API_KEY", "model": "gpt-x", "message": "c" } } }
    ]));
    assert!(ok, "{out}");
    assert!(out.contains("#ai  built in"), "{out}");
    assert!(out.contains("    local: u/var\n"), "{out}");
    assert!(
        out.contains("    local: http://localhost:1234/v1/chat/completions\n"),
        "{out}"
    );
    assert!(
        out.contains("    remote: https://api.openai.com/v1/responses\n"),
        "{out}"
    );
    // a url from the machine's defaults is where a step without one sends
    let (ok, out) = check_with(
        ai(json!({ "message": "x" })),
        Some(
            json!({ "url": "https://openrouter.ai/api/v1/chat/completions",
                     "model": "openai/gpt-x" }),
        ),
    );
    assert!(
        ok && out.contains("    remote: https://openrouter.ai/api/v1/chat/completions\n"),
        "{out}"
    );
    // a url step without a model: the server picks its default
    let (ok, out) = check(ai(
        json!({ "url": "https://openrouter.ai/api/v1/chat/completions", "message": "x" }),
    ));
    assert!(
        ok && out.contains("    remote: https://openrouter.ai/api/v1/chat/completions\n"),
        "{out}"
    );
    for (args, says) in [
        (
            json!({ "url": "https://api.openai.com/v1/models", "model": "m", "message": "x" }),
            "url ends in /chat/completions or /responses",
        ),
        (
            json!({ "url": "http://localhost:1234/v1/chat/completions", "model": "m",
                    "message": "x", "context": 8192 }),
            "context is itero-ai's",
        ),
        (
            json!({ "model": "u/var", "key": "$$OPENAI_API_KEY", "message": "x" }),
            "key goes with a url",
        ),
        (
            json!({ "model": "u/var", "text": "x" }),
            "takes message, not text",
        ),
    ] {
        let (ok, out) = check(ai(args.clone()));
        assert!(!ok && out.contains(says), "{args}: {out}");
    }
}

#[test]
fn a_url_step_gets_none_of_itero_ais_warnings() {
    // its model is the server's, not this machine's; none is allowed
    let (ok, out) = check(json!([
        { "#ai": { "args": { "url": "https://api.openai.com/v1/chat/completions",
                             "model": "gpt-x", "message": "a", "thinking": false } } },
        { "#ai": { "args": { "url": "https://openrouter.ai/api/v1/chat/completions",
                             "message": "b" } } },
        { "#ai": { "args": { "url": "$event.url", "message": "c" } } }
    ]));
    assert!(ok && !out.contains("warning"), "{out}");
    // a url known only in a run may be any host: remote, as written
    assert!(out.contains("    remote: $event.url\n"), "{out}");
    // the machine's url: a step without one sends there, not to itero-ai
    let (ok, out) = check_with(
        ai(json!({ "message": "x" })),
        Some(json!({ "url": "https://openrouter.ai/api/v1/chat/completions" })),
    );
    assert!(ok && !out.contains("warning"), "{out}");
    // a step of its own on itero-ai is still checked against this machine
    let (ok, out) = check(ai(json!({ "model": "u/yok", "message": "x" })));
    assert!(
        ok && out.contains("no model u/yok on this machine"),
        "{out}"
    );
}

#[test]
fn a_key_that_looks_like_a_token_is_warned_of() {
    let url = "https://api.openai.com/v1/chat/completions";
    let (ok, out) = check(ai(
        json!({ "url": url, "key": "sk-proj-4f9aB2cD8eF1gH3iJ5kL", "message": "x" }),
    ));
    assert!(
        ok && out.contains("warning: key looks like a token: write it as a $$NAME (")
            && out.contains("/workflow/posta/steps/#ai)"),
        "{out}"
    );
    // a short word, a secret, a value of the run: nothing to say
    for key in ["lm-studio", "$$OPENAI_API_KEY", "$event.anahtar"] {
        let (ok, out) = check(ai(json!({ "url": url, "key": key, "message": "x" })));
        assert!(ok && !out.contains("warning"), "{key}: {out}");
    }
}
