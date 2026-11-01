//! `itero ai defaults`, with the machine's state and cache in a temporary
//! folder.
#![cfg(unix)]
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

fn itero(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .env("XDG_STATE_HOME", d.join("durum"))
        .env("XDG_CACHE_HOME", d.join("önbellek"))
        .env("HOME", d)
        .output()
        .unwrap()
}

fn index(d: &Path) {
    itero_ai_index::Index {
        models: vec![itero_ai_index::Entry {
            name: "u/qwen:Q4_K_M".into(),
            repo: "u/qwen".into(),
            quant: "Q4_K_M".into(),
            paths: vec!["/m.gguf".into()],
            maker: "u".into(),
            params: None,
            architecture: "qwen35".into(),
            context: Some(262144),
            bytes: 0,
            licence: "apache-2.0".into(),
            chat: true,
            runs: true,
            thinking: true,
            basename: Some("Qwen3.5-4B".into()),
            sampling: None,
            thinking_off: Some(true),
            modified: 0,
            downloaded: None,
            removed: None,
        }],
    }
    .write(&d.join("önbellek/itero/models.json"))
    .unwrap();
}

fn shown(d: &Path) -> Value {
    let o = itero(d, &["ai", "defaults", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn each_value_says_where_it_comes_from() {
    let d = tempfile::tempdir().unwrap();
    index(d.path());
    let v = shown(d.path());
    assert_eq!(
        v["temperature"],
        json!({ "value": 0.8, "from": "llama.cpp" })
    );
    assert_eq!(v["context"], json!({ "value": 32768, "from": "itero-ai" }));
    assert!(v["model"]["value"].is_null());
    let o = itero(
        d.path(),
        &[
            "ai",
            "defaults",
            "set",
            r#"{"model": "u/qwen", "top_p": 0.5}"#,
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v = shown(d.path());
    assert_eq!(v["model"], json!({ "value": "u/qwen", "from": "machine" }));
    assert_eq!(v["top_p"], json!({ "value": 0.5, "from": "machine" }));
    assert_eq!(v["temperature"]["value"], 0.7);
    assert_eq!(
        v["temperature"]["from"],
        "itero's table (Unsloth's Qwen3.5 guide)"
    );
    itero(d.path(), &["ai", "defaults", "set", r#"{"top_p": null}"#]);
    assert_eq!(shown(d.path())["top_p"]["value"], 0.8);
    let o = itero(d.path(), &["ai", "defaults"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("itero's table") && out.contains("u/qwen"), "{out}");
}

#[test]
fn what_is_wrong_is_said_and_a_missing_model_warned_of() {
    let d = tempfile::tempdir().unwrap();
    index(d.path());
    for (bad, says) in [
        (r#"{"text": "özetle"}"#, "not text"),
        (r#"{"temperature": "sıcak"}"#, "temperature"),
        (r#"[1]"#, "an object"),
    ] {
        let o = itero(d.path(), &["ai", "defaults", "set", bad]);
        assert!(!o.status.success(), "{bad}");
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(e.contains(says), "{e}");
    }
    let o = itero(
        d.path(),
        &["ai", "defaults", "set", r#"{"model": "u/yok"}"#],
    );
    assert!(o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("warning: no model u/yok on this machine"), "{e}");
}
