//! itero-ai ends cleanly with a model loaded: idle, it frees the model
//! and exits 0, which the daemon reads as stopped, not failed. Local only,
//! with Qwen3.5-4B, as real_model.rs.
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn idle_with_a_model_loaded_it_exits_0() {
    let Ok(path) = std::env::var("ITERO_AI_TEST_MODEL") else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let index = d.path().join("models.json");
    std::fs::write(
        &index,
        serde_json::json!({ "models": [{ "name": "unsloth/Qwen3.5-4B-GGUF:Q4_K_M",
            "repo": "unsloth/Qwen3.5-4B-GGUF", "quant": "Q4_K_M", "paths": [path],
            "maker": "unsloth", "params": "4B", "architecture": "qwen35", "context": null,
            "bytes": 0, "licence": "apache-2.0", "chat": true, "runs": true,
            "thinking": true, "modified": 0 }] })
        .to_string(),
    )
    .unwrap();
    let sock = d.path().join("ç.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_itero-ai"))
        .env("ITERO_AI_SOCKET", &sock)
        .env("ITERO_AI_IDLE", "2")
        .env("ITERO_AI_INDEX", &index)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let r = reqwest::blocking::Client::builder()
        .unix_socket(&*sock)
        .build()
        .unwrap()
        .post("http://itero-ai/v1/chat/completions")
        .json(
            &serde_json::json!({ "model": "unsloth/Qwen3.5-4B-GGUF", "max_tokens": 3,
            "temperature": 0, "messages": [{ "role": "user", "content": "merhaba" }] }),
        )
        .send()
        .unwrap();
    assert!(r.status().is_success());
    let status = child.wait().unwrap();
    let told: Vec<String> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .collect();
    assert_eq!(status.code(), Some(0), "{status}: {told:?}");
    assert!(told.iter().any(|l| l.contains("model.loaded")), "{told:?}");
    assert!(!sock.exists());
}
