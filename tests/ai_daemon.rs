//! The daemon gives an `#ai` step its itero-ai: here a stand-in, through
//! ITERO_AI_SOCKET, so no model or itero-ai is needed. Its own test
//! binary: it sets ITERO_AI_SOCKET for the process.
#![cfg(unix)]
#[path = "common/fake_ai.rs"]
mod fake_ai;
use itero::engine::journal::Journal;
use serde_json::json;
use std::path::Path;

const NOW: &str = "2026-10-29T10:00:00Z";

#[test]
fn an_ai_step_runs_in_a_workflow() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ai.sock");
    let fake = fake_ai::serve(&sock, Some("\"acil\""));
    // SAFETY: this binary's only test, before the daemon's threads
    unsafe { std::env::set_var("ITERO_AI_SOCKET", &sock) };
    let root = d.path().join("çalışma");
    std::fs::create_dir_all(root.join("workflows")).unwrap();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    std::fs::write(
        root.join("workflows/posta.json"),
        json!({
            "on": [{ "event": "posta.geldi" }],
            "steps": [{ "#ai": { "args": {
                "model": "unsloth/Qwen3.5-4B-GGUF", "text": "$konu",
                "choose": ["acil", "sonra", "çöp"] } } }]
        })
        .to_string(),
    )
    .unwrap();
    let ws = itero::workspace::find(&root).unwrap();
    itero::engine::events::emit(
        &ws,
        "posta.geldi",
        Some(json!({ "konu": "Sunucu çöktü!" })),
        NOW,
    )
    .unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        itero::engine::steps::status(&runs[0].runtime),
        "done",
        "{:?}",
        runs[0].runtime
    );
    assert_eq!(runs[0].runtime.state["output"], json!("acil"));
    let req = &fake.requests.lock().unwrap()[0];
    assert!(
        req["messages"][0]["content"]
            .as_str()
            .unwrap()
            .starts_with("Sunucu çöktü!"),
        "{req}"
    );
}
