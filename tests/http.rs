use axum::body::Body;
use axum::http::{Request, StatusCode};
use itero::engine::daemon::Health;
use itero::engine::http::{Engine, router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// A workspace on port 8790, its router, and the receiver the router wakes.
fn app(d: &std::path::Path) -> (axum::Router, std::sync::mpsc::Receiver<()>) {
    std::fs::write(d.join("itero.toml"), "port = 8790\n").unwrap();
    let ws = itero::workspace::find(d).unwrap();
    let (wake, woken) = std::sync::mpsc::channel();
    let engine = Arc::new(Engine {
        health: Mutex::new(Health::starting("2026-10-15T14:00:00Z")),
        wake,
    });
    (router(ws, engine), woken)
}

fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header("host", "127.0.0.1:8790")
        .body(Body::empty())
        .unwrap()
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn health_says_the_loop_is_alive() {
    let d = tempfile::tempdir().unwrap();
    let (app, _w) = app(d.path());
    let (code, v) = send(&app, get("/health")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(v["passes"], 0);
    assert_eq!(v["started"], "2026-10-15T14:00:00Z");
    assert_eq!(v["workspace"], json!(d.path().display().to_string()));
}

#[tokio::test]
async fn another_host_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let (app, _w) = app(d.path());
    for host in ["kötü.örnek.test:8790", "127.0.0.1:9999", "localhost"] {
        let req = Request::get("/health")
            .header("host", host)
            .body(Body::empty())
            .unwrap();
        let (code, v) = send(&app, req).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{host}");
        assert!(v["error"].as_str().unwrap().contains("Host"), "{v}");
    }
    let req = Request::get("/health")
        .header("host", "localhost:8790")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, req).await.0, StatusCode::OK);
}

fn post(path: &str, body: &str) -> Request<Body> {
    Request::post(path)
        .header("host", "127.0.0.1:8790")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn an_event_posted_is_saved_and_wakes_the_loop() {
    let d = tempfile::tempdir().unwrap();
    let (app, woken) = app(d.path());
    let (code, v) = send(
        &app,
        post(
            "/events",
            r#"{"type": "foto.eklendi", "data": {"yol": "çarşı.jpg"}}"#,
        ),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{v}");
    assert!(v["id"].is_string());
    assert!(woken.try_recv().is_ok(), "the loop is woken");
    let (code, v) = send(&app, post("/events", r#"{"type": "timer.fired"}"#)).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("engine's own"));
    let (code, _) = send(
        &app,
        post("/events", r#"{"type": "onay.geldi", "run": "yok"}"#),
    )
    .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_event_must_be_json_said_so() {
    let d = tempfile::tempdir().unwrap();
    let (app, _w) = app(d.path());
    let req = Request::post("/events")
        .header("host", "127.0.0.1:8790")
        .header("content-type", "text/plain")
        .body(Body::from(r#"{"type": "foto.eklendi"}"#))
        .unwrap();
    let (code, _) = send(&app, req).await;
    assert_eq!(
        code,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "a cross-site form post is refused"
    );
}
