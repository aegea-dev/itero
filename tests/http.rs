use axum::body::Body;
use axum::http::{Request, StatusCode};
use itero::engine::daemon::Health;
use itero::engine::http::{Engine, router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// A workspace on port 8790, its router, and the receiver the router wakes.
fn app(d: &std::path::Path) -> (axum::Router, std::sync::mpsc::Receiver<()>) {
    std::fs::write(d.join("itero.config.json"), r#"{"port": 8790}"#).unwrap();
    let ws = itero::workspace::find(d).unwrap();
    let (wake, woken) = std::sync::mpsc::channel();
    let engine = Arc::new(Engine {
        health: Mutex::new(Health::starting("2026-10-15T14:00:00Z")),
        wake,
    });
    (router(ws, engine, 8790), woken)
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
    assert_eq!(v["connectors"], json!({}), "no connectors");
    assert_eq!(
        v["workspace"]["path"],
        json!(d.path().display().to_string())
    );
    assert_eq!(v["workspace"]["id"].as_str().unwrap().len(), 36);
    assert!(v["workspace"]["name"].as_str().unwrap().contains('-'));
    assert_eq!(v["daemon"]["id"].as_str().unwrap().len(), 36);
    assert!(v["daemon"]["name"].as_str().unwrap().contains('-'));
    assert_eq!(v["daemon"]["pid"], std::process::id());
    assert_eq!(v["daemon"]["itero"], env!("CARGO_PKG_VERSION"));
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

#[tokio::test]
async fn runs_and_workflows_can_be_read() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::create_dir_all(d.path().join("actions")).unwrap();
    std::fs::write(
        d.path().join("workflows/sayaç.json"),
        r##"{ "on": [{ "event": "say.bir" }], "steps": ["#yaz"] }"##,
    )
    .unwrap();
    std::fs::write(
        d.path().join("workflows/bakım.json"),
        r##"{ "on": [{ "event": "say.bir" }], "steps": ["#yaz"] }"##,
    )
    .unwrap();
    let action = d.path().join("actions/yaz");
    std::fs::write(&action, "#!/bin/sh\necho '{\"son\": \"çay\"}'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let (app, _w) = app(d.path());
    std::fs::write(
        d.path().join("itero.config.json"),
        r#"{"port": 8790, "workflow": {"bakım": {"internal": true}}}"#,
    )
    .unwrap();
    let ws = itero::workspace::find(d.path()).unwrap();
    let now = "2026-10-15T14:00:00Z";
    itero::engine::events::emit(&ws, "say.bir", Some(json!({"kim": "ayşe"})), now).unwrap();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, now.parse().unwrap(), exe).unwrap();

    let (code, runs) = send(&app, get("/runs?workflow=sayaç&limit=10")).await;
    assert_eq!(code, StatusCode::OK, "{runs}");
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["status"], "done");
    assert_eq!(runs[0]["workflow"], "sayaç");
    let (_, none) = send(&app, get("/runs?workflow=baska")).await;
    assert_eq!(none, json!([]));
    let (_, none) = send(&app, get("/runs?status=error")).await;
    assert_eq!(none, json!([]));
    let (_, none) = send(&app, get("/runs?status=waiting")).await;
    assert_eq!(none, json!([]));
    let (_, done) = send(&app, get("/runs?status=done")).await;
    assert_eq!(done.as_array().unwrap().len(), 1, "internal runs left out");
    let (_, all) = send(&app, get("/runs?all=true")).await;
    assert_eq!(all.as_array().unwrap().len(), 2, "{all}");
    let id = runs[0]["id"].as_str().unwrap();
    let (code, run) = send(&app, get(&format!("/runs/{id}"))).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(run["status"], "done");
    assert_eq!(run["input"], json!({"kim": "ayşe"}));
    assert_eq!(
        run["state"],
        json!({"event": {"kim": "ayşe"}, "output": {"son": "çay"}, "steps": {"yaz": {"son": "çay"}}})
    );
    assert_eq!(run["events"][0]["type"], "run.started");
    let name = runs[0]["name"].as_str().unwrap();
    let (code, by_name) = send(&app, get(&format!("/runs/{name}"))).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(by_name["id"], id);
    assert_eq!(send(&app, get("/runs/yok")).await.0, StatusCode::NOT_FOUND);
    let (_, flows) = send(&app, get("/workflows")).await;
    let named = |n: &str| {
        flows
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(named("sayaç")["on"]["events"], json!(["say.bir"]));
    assert_eq!(named("sayaç")["internal"], false);
    assert_eq!(named("sayaç")["path"], "workflows/sayaç.json");
    assert_eq!(named("keep")["path"], serde_json::Value::Null, "built in");
    assert_eq!(named("keep")["internal"], true, "built in");
}

fn post(path: &str, body: &str) -> Request<Body> {
    Request::post(path)
        .header("host", "127.0.0.1:8790")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_event_posted_with_wait_is_answered_with_its_runs_end() {
    let d = tempfile::tempdir().unwrap();
    let (app, woken) = app(d.path());
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::create_dir_all(d.path().join("actions")).unwrap();
    std::fs::write(
        d.path().join("workflows/selam.json"),
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }"##,
    )
    .unwrap();
    let action = d.path().join("actions/selamla");
    std::fs::write(&action, "#!/bin/sh\necho '{\"selam\": \"merhaba\"}'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // the daemon's loop, which the post wakes
    let ws = itero::workspace::find(d.path()).unwrap();
    std::thread::spawn(move || {
        let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
        while woken.recv().is_ok() {
            itero::engine::daemon::run_once(&ws, chrono::Utc::now(), exe).unwrap();
        }
    });
    let (code, v) = send(
        &app,
        post(
            "/events?wait=20",
            r#"{"type": "kişi.geldi", "data": {"kim": "ayşe"}}"#,
        ),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["message"].as_str().unwrap().len(), 36);
    assert_eq!(v["runs"][0]["workflow"], "selam");
    assert_eq!(v["runs"][0]["status"], "done");
    assert_eq!(v["runs"][0]["state"]["output"]["selam"], "merhaba");
    let (code, v) = send(
        &app,
        post(
            "/events",
            r#"{"type": "kişi.geldi", "reply": "http://örnek.test"}"#,
        ),
    )
    .await;
    // gone: a workflow posts its own result
    assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("reply"), "{v}");
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
    let (code, _) = send(&app, post("/events", r#"{"type": "foto"}"#)).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "a type of one word");
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
