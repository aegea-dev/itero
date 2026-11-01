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
    let (ok, out) = check(ai(json!({ "model": "u/var", "text": "$output.body",
                                     "choose": ["acil", "sonra"] })));
    assert!(ok && !out.contains("warning"), "{out}");
    let (ok, out) = check(ai(json!({ "model": "u/yok", "text": "x" })));
    assert!(
        ok && out.contains("warning: posta: #ai: no model u/yok on this machine"),
        "{out}"
    );
    // a model given as a value is known only in a run
    let (ok, out) = check(ai(
        json!({ "model": "$event.model", "text": "x", "fields": "$event.alanlar" }),
    ));
    assert!(ok && !out.contains("warning"), "{out}");
}

#[test]
fn a_step_without_a_model_is_warned_of_unless_the_machine_has_one() {
    let (ok, out) = check(ai(json!({ "text": "x" })));
    assert!(ok && out.contains("warning: posta: #ai: no model"), "{out}");
    let (ok, out) = check_with(
        ai(json!({ "text": "x" })),
        Some(json!({ "model": "u/var" })),
    );
    assert!(ok && !out.contains("warning"), "{out}");
    let (ok, out) = check_with(
        ai(json!({ "text": "x" })),
        Some(json!({ "model": "u/yok" })),
    );
    assert!(ok && out.contains("no model u/yok on this machine"), "{out}");
}

#[test]
fn thinking_off_on_a_model_that_cannot_is_warned_of() {
    let (ok, out) = check(ai(
        json!({ "model": "u/düşünen", "text": "x", "thinking": false }),
    ));
    assert!(ok && out.contains("cannot turn thinking off"), "{out}");
    let (ok, out) = check(ai(json!({ "model": "u/düşünen", "text": "x" })));
    assert!(ok && !out.contains("warning"), "says nothing, asks nothing: {out}");
    let (ok, out) = check(ai(
        json!({ "model": "u/var", "text": "x", "thinking": false }),
    ));
    assert!(ok && !out.contains("warning"), "{out}");
}

#[test]
fn what_a_run_would_refuse_is_an_error() {
    for (args, says) in [
        (json!({ "model": "u/var" }), "it needs $text"),
        (
            json!({ "model": "u/var", "text": "x", "choose": [] }),
            "choose is a list",
        ),
        (
            json!({ "model": "u/var", "text": "x", "fields": { "a": "renk" } }),
            "not renk",
        ),
        (
            json!({ "model": "u/var", "text": "x", "choose": ["a"], "fields": { "a": "text" } }),
            "not both",
        ),
    ] {
        let (ok, out) = check(ai(args.clone()));
        assert!(!ok && out.contains(says), "{args}: {out}");
    }
}

#[test]
fn an_arg_it_does_not_read_is_warned_of() {
    let (ok, out) = check(ai(json!({ "model": "u/var", "text": "x", "renk": "mavi" })));
    assert!(
        ok && out.contains("#ai takes model, text,") && out.contains("not renk"),
        "{out}"
    );
}
