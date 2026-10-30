//! `itero check` knows `#ai`: the rules of a run, on what the workflow
//! writes, and a warning for a model this machine does not have.
#![cfg(unix)]
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn check(steps: Value) -> (bool, String) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    std::fs::write(
        d.path().join("workflows/posta.json"),
        json!({ "on": [{ "event": "posta.geldi" }], "steps": steps }).to_string(),
    )
    .unwrap();
    let cache = d.path().join("önbellek");
    itero_ai_index::Index {
        models: vec![itero_ai_index::Entry {
            name: "u/var:Q4_K_M".into(),
            repo: "u/var".into(),
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
            modified: 0,
            downloaded: None,
            removed: None,
        }],
    }
    .write(&cache.join("itero/models.json"))
    .unwrap();
    let out = Command::new(Path::new(env!("CARGO_BIN_EXE_itero")))
        .arg("check")
        .current_dir(d.path())
        .env("XDG_CACHE_HOME", &cache)
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
fn what_a_run_would_refuse_is_an_error() {
    for (args, says) in [
        (json!({ "text": "x" }), "it needs $model"),
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
