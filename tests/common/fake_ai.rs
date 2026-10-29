//! A stand-in for itero-ai on a Unix socket, for the tests of `#ai`: it
//! answers every chat with the same text, streamed as itero-ai does, and
//! keeps each request's body. Dropped, it stops.
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct FakeAi {
    pub requests: Arc<Mutex<Vec<Value>>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for FakeAi {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

#[derive(Clone)]
struct S {
    answer: Option<String>,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn chat(State(s): State<S>, Json(body): Json<Value>) -> Response {
    s.requests.lock().unwrap().push(body.clone());
    let Some(answer) = s.answer else {
        let model = body["model"].as_str().unwrap_or("");
        return (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "error": { "message": format!("no model {model} on this machine"),
                                    "type": "invalid_request_error", "param": "model" } }),
            ),
        )
            .into_response();
    };
    // the answer in two pieces, thinking first, then the end and [DONE]
    let mid = answer
        .char_indices()
        .nth(answer.chars().count() / 2)
        .map_or(0, |(i, _)| i);
    let chunk = |delta: Value, finish: Value| {
        format!(
            "data: {}\n\n",
            json!({ "object": "chat.completion.chunk",
                    "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] })
        )
    };
    let text = [
        chunk(json!({ "reasoning_content": "düşünüyor" }), Value::Null),
        chunk(json!({ "content": &answer[..mid] }), Value::Null),
        chunk(json!({ "content": &answer[mid..] }), Value::Null),
        chunk(json!({}), json!("stop")),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    ([("content-type", "text/event-stream")], text).into_response()
}

/// Serves on `socket`, answering `answer`; `None` answers that the model
/// is missing (404).
pub fn serve(socket: &Path, answer: Option<&str>) -> FakeAi {
    let requests = Arc::new(Mutex::new(vec![]));
    let s = S {
        answer: answer.map(String::from),
        requests: requests.clone(),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let socket = socket.to_path_buf();
    let (ready, is_ready) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            ready.send(()).unwrap();
            let app = axum::Router::new()
                .route("/v1/chat/completions", axum::routing::post(chat))
                .with_state(s);
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
    });
    is_ready.recv().unwrap();
    FakeAi {
        requests,
        stop: Some(stop),
    }
}
