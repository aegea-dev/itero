//! Stand-ins for the tests of `#ai`: itero-ai on a Unix socket, and a
//! model behind a URL on a local port. Dropped, they stop.
// each test binary uses one of them
#![allow(dead_code)]
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// A stand-in for itero-ai: it answers every chat with the same text,
/// streamed as itero-ai does, and keeps each request's body.
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

/// What the stand-in on a port answers a request with.
#[derive(Clone)]
pub enum Reply {
    /// The answer's text, in the API's own shape.
    Text(String),
    /// A 400 refusing this parameter, as OpenAI's errors name it.
    Refuse(&'static str),
    /// A failure with this status.
    Status(u16),
}

/// Each request's body and its Authorization header.
pub type Asked = Arc<Mutex<Vec<(Value, Option<String>)>>>;

/// A stand-in for a model behind a URL: Chat Completions and Responses on
/// a local port, streamed when the request asks. Each request's body and
/// Authorization header are kept; the replies are given in turn, the last
/// one again.
pub struct FakeHttp {
    pub requests: Asked,
    /// `http://127.0.0.1:<port>/v1`.
    pub url: String,
    _stop: tokio::sync::oneshot::Sender<()>,
}

#[derive(Clone)]
struct H {
    replies: Arc<Mutex<Vec<Reply>>>,
    requests: Asked,
}

fn next(h: &H) -> Reply {
    let mut r = h.replies.lock().unwrap();
    if r.len() > 1 {
        r.remove(0)
    } else {
        r[0].clone()
    }
}

async fn http(
    State(h): State<H>,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    h.requests.lock().unwrap().push((body.clone(), auth));
    let responses = uri.path().ends_with("/responses");
    let usage = json!({ "prompt_tokens": 7, "completion_tokens": 3,
                        "prompt_tokens_details": { "cached_tokens": 2 } });
    match next(&h) {
        Reply::Status(s) => (
            StatusCode::from_u16(s).unwrap(),
            Json(json!({ "error": { "message": format!("sunucu {s} dedi") } })),
        )
            .into_response(),
        Reply::Refuse(param) => (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": { "message": format!("{param} desteklenmiyor"),
                                    "type": "invalid_request_error", "param": param } }),
            ),
        )
            .into_response(),
        Reply::Text(t) if body["stream"] == true => sse(&t, responses, &usage),
        Reply::Text(t) if responses => Json(json!({
            "id": "resp_1", "object": "response", "model": body["model"],
            "output": [{ "type": "message", "role": "assistant",
                         "content": [{ "type": "output_text", "text": t }] }],
            "usage": usage }))
        .into_response(),
        Reply::Text(t) => Json(json!({
            "id": "chatcmpl_1", "object": "chat.completion", "model": body["model"],
            "choices": [{ "index": 0, "message": { "role": "assistant", "content": t },
                          "finish_reason": "stop" }],
            "usage": usage }))
        .into_response(),
    }
}

/// The answer as server-sent events, in two pieces, as servers send them:
/// a keep-alive comment, `data:` without its space, `\r\n` line ends.
fn sse(t: &str, responses: bool, usage: &Value) -> Response {
    let mid = t
        .char_indices()
        .nth(t.chars().count() / 2)
        .map_or(0, |(i, _)| i);
    let mut out = String::from(": canlı tutuluyor\r\n\r\n");
    for piece in [&t[..mid], &t[mid..]] {
        let delta = if responses {
            json!({ "type": "response.output_text.delta", "delta": piece })
        } else {
            json!({ "choices": [{ "index": 0, "delta": { "content": piece } }] })
        };
        if responses {
            out.push_str("event: response.output_text.delta\r\n");
        }
        out.push_str(&format!("data:{delta}\r\n\r\n"));
    }
    if responses {
        let done = json!({ "type": "response.completed",
                           "response": { "id": "resp_1", "usage": usage } });
        out.push_str(&format!(
            "event: response.completed\r\ndata: {done}\r\n\r\n"
        ));
    } else {
        let last = json!({ "choices": [], "usage": usage });
        out.push_str(&format!("data: {last}\r\n\r\ndata: [DONE]\r\n\r\n"));
    }
    ([("content-type", "text/event-stream")], out).into_response()
}

/// Serves on a free local port, answering with `replies` in turn.
pub fn serve_http(replies: Vec<Reply>) -> FakeHttp {
    let requests = Arc::new(Mutex::new(vec![]));
    let h = H {
        replies: Arc::new(Mutex::new(replies)),
        requests: requests.clone(),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let (ready, is_ready) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ready.send(listener.local_addr().unwrap().port()).unwrap();
            let app = axum::Router::new()
                .route("/v1/chat/completions", axum::routing::post(http))
                .route("/v1/responses", axum::routing::post(http))
                .with_state(h);
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
    });
    let port = is_ready.recv().unwrap();
    FakeHttp {
        requests,
        url: format!("http://127.0.0.1:{port}/v1"),
        _stop: stop,
    }
}
