//! The API over a real Unix socket, with the fake engine: no model.
#![cfg(unix)]
use itero_ai::engine::{FakeEngine, Piece};
use itero_ai::server::{Config, serve};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn client(sock: &Path) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .unix_socket(sock)
        .build()
        .unwrap()
}

struct Running {
    d: tempfile::TempDir,
    sock: PathBuf,
    done: std::thread::JoinHandle<anyhow::Result<()>>,
}

fn start(idle: u64) -> Running {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ş.sock");
    let index = d.path().join("models.json");
    std::fs::write(&index, r#"{"models":[]}"#).unwrap();
    let cfg = Config {
        socket: sock.clone(),
        idle: Duration::from_secs(idle),
        index,
    };
    let engine = FakeEngine {
        models: vec!["u/küçük".into()],
        pieces: vec![
            Piece::Reasoning("düşün".into()),
            Piece::Content("cev".into()),
            Piece::Content("ap".into()),
        ],
        loaded: None,
    };
    let done = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(serve(cfg, Box::new(engine)))
    });
    for _ in 0..200 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Running { d, sock, done }
}

fn ask(sock: &Path, body: Value) -> reqwest::blocking::Response {
    client(sock)
        .post("http://itero-ai/v1/chat/completions")
        .json(&body)
        .send()
        .unwrap()
}

#[test]
fn an_answer_in_openai_shape() {
    let r = start(60);
    let v: Value = ask(
        &r.sock,
        json!({ "model": "u/küçük", "messages": [{ "role": "user", "content": "selam" }] }),
    )
    .json()
    .unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["model"], "u/küçük");
    assert_eq!(v["choices"][0]["message"]["content"], "cevap");
    assert_eq!(v["choices"][0]["message"]["reasoning_content"], "düşün");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["usage"]["completion_tokens"], 3);
}

#[test]
fn a_stream_of_chunks_ending_done() {
    let r = start(60);
    let text = ask(
        &r.sock,
        json!({ "model": "u/küçük", "stream": true,
                "messages": [{ "role": "user", "content": "selam" }] }),
    )
    .text()
    .unwrap();
    let datas: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .collect();
    assert_eq!(datas.len(), 5, "{text}");
    assert_eq!(datas.last(), Some(&"[DONE]"));
    let first: Value = serde_json::from_str(datas[0]).unwrap();
    assert_eq!(first["object"], "chat.completion.chunk");
    assert_eq!(first["choices"][0]["delta"]["reasoning_content"], "düşün");
    let last: Value = serde_json::from_str(datas[3]).unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["completion_tokens"], 3);
}

#[test]
fn errors_in_openai_shape() {
    let r = start(60);
    let res = ask(
        &r.sock,
        json!({ "model": "u/küçük", "tools": [], "messages": [] }),
    );
    assert_eq!(res.status(), 400);
    let v: Value = res.json().unwrap();
    assert_eq!(v["error"]["param"], "tools");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    for stream in [false, true] {
        let res = ask(
            &r.sock,
            json!({ "model": "u/yok", "stream": stream,
                    "messages": [{ "role": "user", "content": "x" }] }),
        );
        assert_eq!(res.status(), 404, "stream {stream}");
        let v: Value = res.json().unwrap();
        assert!(v["error"]["message"].as_str().unwrap().contains("u/yok"));
    }
}

#[test]
fn models_and_health() {
    let r = start(60);
    std::fs::write(
        r.d.path().join("models.json"),
        json!({ "models": [{
            "name": "u/küçük:Q4_K_M", "repo": "u/küçük", "quant": "Q4_K_M",
            "paths": ["/m/k.gguf"], "maker": "u", "params": "4B", "architecture": "qwen35",
            "context": 262144, "bytes": 1, "licence": "apache-2.0", "chat": true,
            "runs": true, "thinking": true, "modified": 0 }] })
        .to_string(),
    )
    .unwrap();
    let c = client(&r.sock);
    let v: Value = c
        .get("http://itero-ai/v1/models")
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"][0]["id"], "u/küçük:Q4_K_M");
    assert_eq!(v["data"][0]["object"], "model");
    assert_eq!(v["data"][0]["owned_by"], "u");
    let h: Value = c
        .get("http://itero-ai/health")
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(h["loaded"], Value::Null);
    ask(
        &r.sock,
        json!({ "model": "u/küçük", "messages": [{ "role": "user", "content": "x" }] }),
    );
    let h: Value = c
        .get("http://itero-ai/health")
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(h["loaded"], "u/küçük");
    assert_eq!(h["waiting"], 0);
}

#[test]
fn it_exits_when_idle_and_frees_its_socket() {
    let r = start(1);
    // asking how it is does not keep it up
    let _ = client(&r.sock)
        .get("http://itero-ai/health")
        .send()
        .unwrap();
    assert!(r.done.join().unwrap().is_ok());
    assert!(!r.sock.exists());
}

#[test]
fn a_taken_socket_is_refused_and_a_dead_one_removed() {
    let r = start(60);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cfg = Config {
        socket: r.sock.clone(),
        idle: Duration::from_secs(60),
        index: r.d.path().join("models.json"),
    };
    let e = rt
        .block_on(serve(cfg, Box::new(FakeEngine::default())))
        .unwrap_err();
    assert!(e.to_string().contains("answers there"), "{e}");
    let dead = r.d.path().join("ölü.sock");
    // binding leaves the file; nobody listens on it once dropped
    drop(std::os::unix::net::UnixListener::bind(&dead).unwrap());
    assert!(dead.exists());
    let cfg = Config {
        socket: dead.clone(),
        idle: Duration::from_secs(1),
        index: r.d.path().join("models.json"),
    };
    assert!(
        rt.block_on(serve(cfg, Box::new(FakeEngine::default())))
            .is_ok()
    );
}

#[test]
fn the_environment_is_needed_whole() {
    // the config comes only from the environment; one missing is named
    let e =
        itero_ai::server::config_from(|k| (k != "ITERO_AI_IDLE").then(|| "x".into())).unwrap_err();
    assert!(e.to_string().contains("ITERO_AI_IDLE"), "{e}");
    let e = itero_ai::server::config_from(|_| Some("çok".into())).unwrap_err();
    assert!(e.to_string().contains("seconds"), "{e}");
    let c = itero_ai::server::config_from(|k| {
        Some(if k == "ITERO_AI_IDLE" {
            "300".into()
        } else {
            format!("/{k}")
        })
    })
    .unwrap();
    assert_eq!(c.idle, Duration::from_secs(300));
    assert_eq!(c.socket, PathBuf::from("/ITERO_AI_SOCKET"));
}
