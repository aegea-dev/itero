# itero, iteration 2: the HTTP API, event triggers, the installer. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apps reach itero: `itero daemon` serves an HTTP API on its port (health, events in, runs, workflows, memory), an app's event starts the workflows that wait for it, and one `curl | sh` line installs itero (and `Itero.app` on macOS).

**Architecture:** The daemon's loop keeps running in its own thread; an axum server on `127.0.0.1:<port>` shares only the loop's health and a channel that wakes it. An event from outside is written to the journal and nothing more (one function, `engine::events::emit`, behind both `POST /events` and `itero emit`). App events have the subject `itero.app`; the daemon's pass turns each into runs of the workflows with a matching `on` trigger, all in one transaction with marking the event handled. The installer downloads a release archive, checks its SHA-256, and installs the binary and `Itero.app`; `make dist` makes the archive the installer and the release workflow both expect.

**Tech Stack:** Rust 2024 (toolchain 1.96.0); new crates axum `~0.8.9`, tokio `~1.53.1` (`rt-multi-thread`, `net`, `macros`, `sync`), tower `~0.5.3` (dev, `util`), all MIT. POSIX sh for the installer.

**Spec:** `docs/design.md` (sections 1, 4 and 7: the daemon's API, triggers, apps).

## Global Constraints

- Crates: MIT or Apache-2.0 only (check with `cargo info <crate>`); requirements written `~x.y.z`.
- Test first; run the tests and read the output before saying something works.
- Fixtures use non-ASCII text (ı, ş, ğ, ü, é) in neutral words, not personal names.
- Public text (comments, README, docs, commits): no AI attribution, no em dash.
- Stage files by name, never `git add -A`. Commit locally; no push, no GitHub repo (the owner makes it in their org).
- The manager stays pure; the daemon decides nothing beyond what workflows say; one transaction per answer; every handler copes with the same event twice; secrets never reach events, the journal, logs or command lines.
- The server listens on 127.0.0.1 only, never another address.
- `make fmt`, `make fmt-check`, `make lint-check` clean before each commit; Linux checked with Docker Desktop (`desktop-linux` context; never colima).

## Decisions (made for this plan)

- **One event entry, two doors.** `engine::events::emit` validates and writes an event to the journal. `POST /events` and `itero emit` both call it. `itero emit` works with the daemon stopped: the journal is the queue.
- **An event is for a run or for the workspace.** `{"type", "data"?, "run"?}`. With `run`, it goes to that run's manager (its `on` transitions); the run must exist and not have ended. Without, it is an app event (`itero.app`) that may start runs.
- **Engine-own types are refused from outside**: `action.*`, `timer.*`, `run.*`. A type is lowercase words joined by dots (`photo.added`).
- **Event triggers**: a trigger is `{"schedule": "..."}` or `{"on": "<type>"}`, exactly one, plus `args`. An app event starts one run per matching trigger; the event's `data` is the run's `input` (the `input` variable in the first transitions). Starting the runs and marking the event handled happen in one transaction, so a redelivered event starts nothing twice. An app event no trigger matches is handled with the note `no workflow starts on <type>` (not an error).
- **Workspace `triggers/` (adding or replacing a workflow's triggers) waits for iteration 3.**
- **Local only, and guarded.** The server checks the `Host` header is `127.0.0.1:<port>` or `localhost:<port>` (so a web page cannot reach it through DNS rebinding) and takes JSON only with `Content-Type: application/json` (so a cross-site page's simple form post is refused and a fetch needs a preflight the server never grants). No CORS headers are sent.
- **Read routes return what the CLI shows**: `GET /runs` (newest first, `?workflow=`, `?limit=`, default 50), `GET /runs/{id}` (the run and its events), `GET /workflows` (name, version, triggers), `GET /workflows/{name}/memory`.
- **Installer**: `install.sh` at the repository root, after videre's (`docs/public/install` there): POSIX sh, curl or wget, SHA-256 checked before unpacking, `~/.local/bin` by default, `--version`, `--to`, `--uninstall`, never edits a shell profile, warns when another `itero` comes first on PATH. On macOS it also installs `Itero.app` to `~/Applications`. Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Archive `itero-<tag>-<target>.tar.gz`, checksum `itero-<tag>-<target>.sha256`. The repository is set in one line, `REPO=`, to change when the org's repo exists.
- **Not in this iteration**: Homebrew tap, the service (launchd/systemd) and watchdog, NATS, the inbox folder, primum as an app, workspace `triggers/`.

## File structure (new and changed)

```
src/engine/events.rs     emit: validate an event and write it to the journal
src/engine/http.rs       the axum router: health, events, runs, workflows, memory; Host guard
src/engine/daemon.rs     Health; run_loop with wake and health; app events start runs
src/engine/journal.rs    subject "app"; start_runs_for_event; run_events
src/workflow/format.rs   Trigger { schedule?, on?, args }
src/workflow/validate.rs a trigger has exactly one of schedule and on
src/cli.rs               daemon serves (--no-serve); itero emit
install.sh               the installer
scripts/test-install.sh  installs from a local "release" through the test seam
Makefile                 dist: the release archive and its checksum
.github/workflows/release.yml   builds the four targets with make dist (runs once the repo exists)
tests/http.rs, tests/events.rs, tests/daemon.rs, tests/schedule.rs
```

---

### Task 1: The server: health, the loop's wake, the Host guard

**Files:**
- Create: `src/engine/http.rs`, `tests/http.rs`
- Modify: `Cargo.toml`, `src/engine/mod.rs` (`pub mod http;`), `src/engine/daemon.rs` (`Health`, `run_loop`), `src/cli.rs` (`daemon` serves unless `--no-serve`), `src/workspace.rs` (`#[derive(Debug, Clone)]` on `Workspace`)

**Interfaces:**
- Produces: `engine::daemon::Health { started: String, passes: u64, last_pass: Option<String>, last_error: Option<String> }` with `Health::starting(at: &str)`; `engine::daemon::run_loop(ws: &Workspace, interval: Duration, exe: &Path, wake: &Receiver<()>, health: &Mutex<Health>, report: impl Fn(&Result<Pass>))` (returns when `wake`'s sender is gone); `engine::http::Engine { health: Mutex<Health>, wake: Sender<()> }`; `engine::http::router(ws: Workspace, engine: Arc<Engine>) -> axum::Router`; `engine::http::serve(router: Router, port: u16) -> anyhow::Result<()>` (binds 127.0.0.1:port, blocks).

- [ ] **Step 1: Add the crates** (check each with `cargo info`: MIT)

```toml
axum = "~0.8.9"
tokio = { version = "~1.53.1", features = ["rt-multi-thread", "net", "macros", "sync"] }

[dev-dependencies]
tower = { version = "~0.5.3", features = ["util"] }
```

- [ ] **Step 2: Write the failing tests** (`tests/http.rs`; requests go through `tower::ServiceExt::oneshot`, no socket)

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use itero::engine::daemon::Health;
use itero::engine::http::{Engine, router};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

/// A workspace on port 8790 with no workflows, its router, and the receiver
/// the router wakes.
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
    Request::get(path).header("host", "127.0.0.1:8790").body(Body::empty()).unwrap()
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let code = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
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
    for host in ["evil.örnek.test:8790", "127.0.0.1:9999", "localhost"] {
        let req = Request::get("/health").header("host", host).body(Body::empty()).unwrap();
        let (code, v) = send(&app, req).await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{host}");
        assert!(v["error"].as_str().unwrap().contains("Host"), "{v}");
    }
    let req = Request::get("/health").header("host", "localhost:8790").body(Body::empty()).unwrap();
    assert_eq!(send(&app, req).await.0, StatusCode::OK);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test --test http`
Expected: FAIL to compile (no `itero::engine::http`).

- [ ] **Step 4: Implement**

`daemon.rs`: `Health` as above (`#[derive(Debug, Clone, PartialEq, serde::Serialize)]`), and `run_loop` from primum (commit f20debf, `src/engine/daemon.rs`): each pass `run_once(ws, Utc::now(), exe)`, record `passes`, `last_pass` (`stamp(now)`), `last_error` (`format!("{e:#}")` or None) in `health`, call `report`, then `wake.recv_timeout(interval)`: `Ok` or `Timeout` go on, `Disconnected` returns; drain `wake.try_recv()` before the next pass.

`http.rs`:

```rust
//! The daemon's HTTP API on 127.0.0.1:<port>: its health, events in, and
//! what the journal holds (runs, workflows, memory). Local only: the Host
//! header must name this port on 127.0.0.1 or localhost.
use super::daemon::Health;
use crate::workspace::Workspace;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

/// What the server shares with the daemon's loop.
pub struct Engine {
    pub health: Mutex<Health>,
    /// Wakes the loop for an event that just arrived.
    pub wake: Sender<()>,
}

#[derive(Clone)]
pub(crate) struct Shared {
    pub(crate) ws: Arc<Workspace>,
    pub(crate) engine: Arc<Engine>,
}

pub(crate) type Reply = (StatusCode, Json<Value>);

pub(crate) fn fail(code: StatusCode, message: impl Into<String>) -> Reply {
    (code, Json(json!({ "error": message.into() })))
}

pub fn router(ws: Workspace, engine: Arc<Engine>) -> Router {
    let s = Shared { ws: Arc::new(ws), engine };
    Router::new()
        .route("/health", get(health))
        .layer(middleware::from_fn_with_state(s.clone(), local_only))
        .with_state(s)
}

/// Refuses a request whose Host is not this server's own address: a web
/// page that rebinds a name to 127.0.0.1 still sends its own name.
async fn local_only(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let port = s.ws.port;
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let ok = [format!("127.0.0.1:{port}"), format!("localhost:{port}")].iter().any(|h| h == host);
    if !ok {
        return fail(StatusCode::FORBIDDEN, format!("Host {host:?} is not this server: use 127.0.0.1:{port}")).into_response();
    }
    next.run(req).await
}

async fn health(State(s): State<Shared>) -> Json<Value> {
    let h = s.engine.health.lock().unwrap_or_else(PoisonError::into_inner).clone();
    Json(json!({
        "ok": true,
        "pid": std::process::id(),
        "workspace": s.ws.root.display().to_string(),
        "started": h.started,
        "passes": h.passes,
        "last_pass": h.last_pass,
        "last_error": h.last_error,
    }))
}

/// Serves `router` on 127.0.0.1:`port` until the process ends.
pub fn serve(router: Router, port: u16) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1:{port}: {e} (another itero daemon? set port in itero.toml)"))?;
        axum::serve(listener, router).await?;
        Ok(())
    })
}
```

`cli.rs`, `Daemon { .. , no_serve: bool }` (`--no-serve`: the passes only). Without `--once`: make the channel and `Engine`, spawn the loop thread (`run_loop` with `&engine.health`), then `http::serve(router(ws.clone(), engine), ws.port)` on the main thread, or `join` the loop thread with `--no-serve`. Print `itero: serving on http://127.0.0.1:<port>/` before serving.

- [ ] **Step 5: Run the tests**

Run: `cargo test --test http && cargo test`
Expected: PASS (2 new), the rest still passing.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/engine/http.rs src/engine/mod.rs src/engine/daemon.rs src/cli.rs src/workspace.rs tests/http.rs
git commit -m "The daemon serves its health on its port, local only"
```

---

### Task 2: Events in: `POST /events` and `itero emit`

**Files:**
- Create: `src/engine/events.rs`, `tests/events.rs`
- Modify: `src/engine/mod.rs` (`pub mod events;`), `src/engine/journal.rs` (subject `"app"`), `src/engine/http.rs` (route), `src/cli.rs` (`Emit`)
- Test: `tests/events.rs`, `tests/http.rs`

**Interfaces:**
- Consumes: `Journal::open`, `Journal::run`, `Journal::publish`, `Envelope::new`, `journal::subject`, `workflow::validate::is_word`.
- Produces: `engine::events::emit(ws: &Workspace, kind: &str, data: Option<Value>, run: Option<&str>, now: &str) -> Result<String, EmitError>` (the event's id); `enum EmitError { Bad(String), NoRun(String), Ended(String), Internal(String) }` with `Display`; `journal::subject("app") == "itero.app"`. HTTP: `POST /events` with JSON `{"type", "data"?, "run"?}` returns 202 `{"id"}`, and 400 / 404 / 409 / 415 / 500 with `{"error"}`. CLI: `itero emit <type> [--data JSON] [--run ID]` prints the id.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/events.rs
use itero::engine::events::{EmitError, emit};
use itero::engine::journal::Journal;
use serde_json::json;

fn ws(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

const NOW: &str = "2026-10-15T14:00:00Z";

#[test]
fn an_app_event_is_saved_for_the_workspace() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let id = emit(&ws, "foto.eklendi", Some(json!({"yol": "tatil/çarşı.jpg"})), None, NOW).unwrap();
    let j = Journal::open(&ws).unwrap();
    let r = j.events(Some("itero.app")).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].event.id, id);
    assert_eq!(r[0].event.kind, "foto.eklendi");
    assert_eq!(r[0].event.source, "outside");
    assert_eq!(r[0].event.data["yol"], "tatil/çarşı.jpg");
}

#[test]
fn bad_events_are_refused_and_nothing_is_saved() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let bad = |kind: &str, data: Option<serde_json::Value>, run: Option<&str>| {
        emit(&ws, kind, data, run, NOW).unwrap_err()
    };
    assert!(matches!(bad("Foto Eklendi", None, None), EmitError::Bad(_)));
    for own in ["action.finished", "timer.fired", "run.started"] {
        let e = bad(own, None, None);
        assert!(e.to_string().contains("engine's own"), "{e}");
    }
    assert!(matches!(bad("foto.eklendi", Some(json!([1, 2])), None), EmitError::Bad(_)));
    assert!(matches!(bad("onay.geldi", None, Some("yok")), EmitError::NoRun(_)));
    assert!(Journal::open(&ws).unwrap().events(None).unwrap().is_empty());
}

#[test]
fn an_event_for_a_run_goes_to_that_run_until_it_ends() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let flow = itero::workflow::parse(r#"{ "name": "onay", "states": ["bekle", "bitti"], "start": "bekle", "end": ["bitti"],
      "transitions": [ { "from": "bekle", "on": "onay.geldi", "to": "bitti" } ] }"#).unwrap();
    let mut j = Journal::open(&ws).unwrap();
    let run = j.start_run(&flow, &Default::default(), NOW, None).unwrap();
    let id = emit(&ws, "onay.geldi", Some(json!({"kişi": "yönetici"})), Some(&run), NOW).unwrap();
    let r = j.events(Some(&format!("itero.run.{run}"))).unwrap();
    assert!(r.iter().any(|r| r.event.id == id));
    // once ended, refused
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
    assert_eq!(j.run(&run).unwrap().unwrap().runtime.state, "bitti");
    assert!(matches!(emit(&ws, "onay.geldi", None, Some(&run), NOW).unwrap_err(), EmitError::Ended(_)));
}
```

and in `tests/http.rs`:

```rust
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
    let (code, v) = send(&app, post("/events", r#"{"type": "foto.eklendi", "data": {"yol": "çarşı.jpg"}}"#)).await;
    assert_eq!(code, StatusCode::ACCEPTED, "{v}");
    assert!(v["id"].is_string());
    assert!(woken.try_recv().is_ok(), "the loop is woken");
    let (code, v) = send(&app, post("/events", r#"{"type": "timer.fired"}"#)).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("engine's own"));
    let (code, _) = send(&app, post("/events", r#"{"type": "onay.geldi", "run": "yok"}"#)).await;
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
    assert_eq!(code, StatusCode::UNSUPPORTED_MEDIA_TYPE, "a cross-site form post is refused");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test events --test http`
Expected: FAIL to compile (no `itero::engine::events`).

- [ ] **Step 3: Implement**

`journal::subject`: `"worker" => "itero.worker"`, `"app" => "itero.app"`, else `itero.run.<id>`. `Journal::next` stays as it is (it already skips only `itero.worker`).

`events.rs`:

```rust
//! Events from outside (an app, a script, a person): checked, then written
//! to the journal and nothing more. The daemon's next pass takes them from
//! there, so an event sent while it is stopped waits for it.
use super::journal::{self, Envelope, Journal};
use crate::workflow::validate::is_word;
use crate::workspace::Workspace;
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum EmitError {
    /// The event itself is wrong.
    Bad(String),
    /// It names a run there is not.
    NoRun(String),
    /// It names a run that has ended.
    Ended(String),
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EmitError::Bad(m) | EmitError::NoRun(m) | EmitError::Ended(m) | EmitError::Internal(m) => f.write_str(m),
        }
    }
}

/// Types the engine sends itself: from outside they would forge a worker's
/// result, a timer, or a run's start.
fn engine_own(kind: &str) -> bool {
    ["action.", "timer.", "run."].iter().any(|p| kind.starts_with(p))
}

/// Saves an event for a run, or (with no run) for the workspace, where it
/// may start runs; returns its id.
pub fn emit(ws: &Workspace, kind: &str, data: Option<Value>, run: Option<&str>, now: &str) -> Result<String, EmitError> {
    if !kind.split('.').all(is_word) {
        return Err(EmitError::Bad(format!("an event type is lowercase words joined by dots, like photo.added, not {kind}")));
    }
    if engine_own(kind) {
        return Err(EmitError::Bad(format!("{kind} is the engine's own: it cannot be sent from outside")));
    }
    let data = match data {
        None => Value::Object(Map::new()),
        Some(v @ Value::Object(_)) => v,
        Some(_) => return Err(EmitError::Bad("data must be a JSON object".into())),
    };
    let internal = |e: anyhow::Error| EmitError::Internal(format!("{e:#}"));
    let j = Journal::open(ws).map_err(internal)?;
    let target = match run {
        None => "app".to_string(),
        Some(id) => {
            let r = j.run(id).map_err(internal)?.ok_or_else(|| EmitError::NoRun(format!("no run {id}")))?;
            if r.ended.is_some() {
                return Err(EmitError::Ended(format!("run {id} has ended")));
            }
            id.to_string()
        }
    };
    let e = Envelope::new("outside", kind, &journal::subject(&target), now, None, data);
    j.publish(&e).map_err(internal)?;
    Ok(e.id)
}
```

`http.rs`: `.route("/events", post(event))`, where

```rust
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Posted {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    run: Option<String>,
}

async fn event(State(s): State<Shared>, body: Result<Json<Posted>, JsonRejection>) -> Result<Reply, Reply> {
    // Json refuses any Content-Type but application/json (415)
    let Json(p) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    let ws = s.ws.clone();
    let now = super::daemon::stamp(chrono::Utc::now());
    // the journal may wait on a busy SQLite: off the async threads
    let id = tokio::task::spawn_blocking(move || super::events::emit(&ws, &p.kind, p.data, p.run.as_deref(), &now))
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| {
            use super::events::EmitError::*;
            let code = match &e {
                Bad(_) => StatusCode::BAD_REQUEST,
                NoRun(_) => StatusCode::NOT_FOUND,
                Ended(_) => StatusCode::CONFLICT,
                Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            fail(code, e.to_string())
        })?;
    // a loop that has stopped finds the event in the journal when it starts
    let _ = s.engine.wake.send(());
    Ok((StatusCode::ACCEPTED, Json(json!({ "id": id }))))
}
```

(`stamp` in `daemon.rs` becomes `pub`; `Workspace` is behind an `Arc`, so `spawn_blocking` takes `s.ws.clone()` and calls `emit(&ws, ...)`.)

`cli.rs`: `Emit { #[arg(value_name = "TYPE")] kind: String, #[arg(long)] data: Option<String>, #[arg(long)] run: Option<String> }`: parse `--data` as JSON (error `--data takes a JSON object`), call `emit` with `stamp(Utc::now())`, print the id.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test events --test http`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/events.rs src/engine/mod.rs src/engine/journal.rs src/engine/http.rs src/engine/daemon.rs src/cli.rs tests/events.rs tests/http.rs
git commit -m "Events in: POST /events and itero emit, for a run or the workspace"
```

---

### Task 3: Event triggers: an app's event starts runs

**Files:**
- Modify: `src/workflow/format.rs` (`Trigger`), `src/workflow/validate.rs`, `src/engine/journal.rs` (`insert_run`, `start_runs_for_event`), `src/engine/daemon.rs` (app events; `fire_schedules` skips `on` triggers)
- Test: `tests/daemon.rs`, `tests/schedule.rs`

**Interfaces:**
- Consumes: `emit` (task 2), `Journal::start_run` (iteration 1).
- Produces: `Trigger { schedule: Option<String>, on: Option<String>, args: Map<String, Value> }`; `Journal::start_runs_for_event(&mut self, seq: i64, starts: &[(&File, &Map<String, Value>)], input: &Value, at: &str) -> Result<Vec<String>>` (starts the runs, each with `run.started` carrying `input`, and marks event `seq` handled, with `error` NULL and the note in a new `note` column when `starts` is empty; one transaction).

- [ ] **Step 1: Write the failing tests**

In `tests/schedule.rs`:

```rust
#[test]
fn a_trigger_has_a_schedule_or_an_event_not_both() {
    let with = |t: &str| format!(r#"{{ "name": "akis", "triggers": [ {t} ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ {{ "from": "basla", "to": "bitti" }} ] }}"#);
    itero::workflow::parse(&with(r#"{ "on": "foto.eklendi" }"#)).unwrap();
    for bad in [r#"{ "schedule": "* * * * *", "on": "foto.eklendi" }"#, r#"{ "args": { "a": 1 } }"#] {
        let e = itero::workflow::parse(&with(bad)).unwrap_err().to_string();
        assert!(e.contains("schedule or on"), "{e}");
    }
    let e = itero::workflow::parse(&with(r#"{ "on": "Foto Eklendi" }"#)).unwrap_err().to_string();
    assert!(e.contains("Foto Eklendi"), "{e}");
}
```

In `tests/daemon.rs`:

```rust
#[test]
fn an_app_event_starts_the_workflows_waiting_for_it_once() {
    let d = tempfile::tempdir().unwrap();
    let ws = workspace(d.path(), "kucuk-resim", r#"{ "name": "kucuk-resim",
      "triggers": [ { "on": "foto.eklendi", "args": { "boyut": 256 } } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "son": "${input.yol}", "boyut": "${args.boyut}" } } ] } ] }"#);
    let now = "2026-10-15T14:00:00Z";
    itero::engine::events::emit(&ws, "foto.eklendi", Some(serde_json::json!({"yol": "çarşı.jpg"})), None, now).unwrap();
    itero::engine::events::emit(&ws, "baska.olay", None, None, now).unwrap();
    run_once(&ws, t(now), exe()).unwrap();
    run_once(&ws, t(now), exe()).unwrap();
    let j = Journal::open(&ws).unwrap();
    let runs = j.runs().unwrap();
    assert_eq!(runs.len(), 1, "one run, however many passes");
    assert_eq!(runs[0].runtime.state, "bitti");
    let m = j.memory("kucuk-resim").unwrap();
    assert_eq!((m["son"].clone(), m["boyut"].clone()), (serde_json::json!("çarşı.jpg"), serde_json::json!(256)));
    // the event nothing waits for is handled, with a note, not an error
    let other = j.events(Some("itero.app")).unwrap().into_iter().find(|r| r.event.kind == "baska.olay").unwrap();
    assert!(other.handled.is_some() && other.error.is_none());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --test schedule --test daemon`
Expected: FAIL (`on` is an unknown field; the app event is given up as "no run").

- [ ] **Step 3: Implement**

- `format.rs`: `Trigger { #[serde(default, skip_serializing_if = "Option::is_none")] schedule: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] on: Option<String>, args }`.
- `validate.rs`: for each trigger, `match (&t.schedule, &t.on)`: `(Some(s), None)` checks the schedule; `(None, Some(on))` checks `on.split('.').all(is_word)` (message `trigger on "<on>" is an event type, like photo.added`); otherwise `a trigger has a schedule or on, one of them`.
- `journal.rs`: add `note TEXT` to `events` (in `SCHEMA`; still version 1: no journal has shipped). Factor the run insert of `start_run` into `fn insert_run(tx: &Connection, f: &File, args: &Map<String, Value>, input: &Value, now: &str) -> Result<String>` (its `run.started` event carries `input`); `start_run` calls it with `{}`. Then

```rust
/// Starts a run for each of `starts` on app event `seq`, the event's data
/// their input, and marks the event handled: one transaction, so an event
/// delivered again starts nothing twice. No starts is noted, not an error.
pub fn start_runs_for_event(&mut self, seq: i64, starts: &[(&File, &Map<String, Value>)], input: &Value, at: &str) -> Result<Vec<String>> {
    let tx = self.db.transaction()?;
    let mut ids = vec![];
    for (f, args) in starts {
        ids.push(insert_run(&tx, f, args, input, at)?);
    }
    let note = starts.is_empty().then_some("no workflow starts on it");
    tx.execute("UPDATE events SET handled = ?2, note = ?3 WHERE seq = ?1 AND handled IS NULL", params![seq, at, note])?;
    tx.commit()?;
    Ok(ids)
}
```

  (`Recorded` gains `note: Option<String>`; `COLUMNS` and `row` read it.)
- `daemon.rs`: `fire_schedules` takes only triggers with `Some(schedule)` (the key stays `<workflow>#<index in triggers>`). In the delivery loop, before `deliver`, an event whose subject is `itero.app` goes to

```rust
/// An app's event: a run of every workflow with an `on` trigger for its
/// type, the trigger's args and the event's data as input.
fn start_triggered(ws: &Workspace, j: &mut Journal, r: &Recorded, now: DateTime<Utc>) -> Result<()> {
    let flows = ws.workflows()?;
    let starts: Vec<(&crate::workflow::File, &Map<String, Value>)> = flows
        .iter()
        .flat_map(|(_, f)| f.triggers.iter().filter(|t| t.on.as_deref() == Some(r.event.kind.as_str())).map(move |t| (f, &t.args)))
        .collect();
    let ids = j.start_runs_for_event(r.seq, &starts, &r.event.data, &stamp(now))?;
    if ids.is_empty() {
        eprintln!("{}: no workflow starts on it", r.event.kind);
    }
    Ok(())
}
```

  and counts in `pass.delivered`.
- `manager.rs` needs no change: `run.started` already enters the start state with the event's data as `input`.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: PASS (the mail example's schedule trigger still parses: `schedule` is now optional only in the type).

- [ ] **Step 5: Commit**

```bash
git add src/workflow/format.rs src/workflow/validate.rs src/engine/journal.rs src/engine/daemon.rs tests/schedule.rs tests/daemon.rs
git commit -m "Event triggers: an app's event starts the workflows waiting for it"
```

---

### Task 4: Reading the journal over HTTP: runs, workflows, memory

**Files:**
- Modify: `src/engine/journal.rs` (`runs_page`), `src/engine/http.rs` (four routes)
- Test: `tests/http.rs`

**Interfaces:**
- Consumes: `Journal::runs`, `Journal::run`, `Journal::events`, `Journal::memory`, `Workspace::workflows`, `workflow::version`.
- Produces: `Journal::runs_page(workflow: Option<&str>, limit: usize) -> Result<Vec<Run>>` (newest first); `GET /runs?workflow=&limit=` → `[{"id","workflow","state","started","ended"}]`; `GET /runs/{id}` → `{"id","workflow","state","args","started","ended","events": [{"seq","type","time","handled","error","data"}]}` or 404; `GET /workflows` → `[{"name","version","triggers"}]`; `GET /workflows/{name}/memory` → the memory object, or 404 for a workflow there is not. `itero runs` uses `runs_page(None, usize::MAX)`.

- [ ] **Step 1: Write the failing test** (`tests/http.rs`; the workspace gets one workflow and one finished run first)

```rust
#[tokio::test]
async fn runs_workflows_and_memory_can_be_read() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("workflows")).unwrap();
    std::fs::write(d.path().join("workflows/sayac.json"), r#"{ "name": "sayac",
      "triggers": [ { "on": "say" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti",
        "do": [ { "action": "memory.set@1", "with": { "son": "çay" } } ] } ] }"#).unwrap();
    let (app, _w) = app(d.path());
    let ws = itero::workspace::find(d.path()).unwrap();
    itero::engine::events::emit(&ws, "say", None, None, "2026-10-15T14:00:00Z").unwrap();
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, "2026-10-15T14:00:00Z".parse().unwrap(), exe).unwrap();

    let (code, runs) = send(&app, get("/runs?workflow=sayac&limit=10")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["state"], "bitti");
    let id = runs[0]["id"].as_str().unwrap();
    let (code, run) = send(&app, get(&format!("/runs/{id}"))).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(run["events"][0]["type"], "run.started");
    assert_eq!(send(&app, get("/runs/yok")).await.0, StatusCode::NOT_FOUND);
    let (_, flows) = send(&app, get("/workflows")).await;
    assert_eq!(flows[0]["name"], "sayac");
    assert_eq!(flows[0]["version"].as_str().unwrap().len(), 64);
    let (_, m) = send(&app, get("/workflows/sayac/memory")).await;
    assert_eq!(m, json!({"son": "çay"}));
    assert_eq!(send(&app, get("/workflows/yok/memory")).await.0, StatusCode::NOT_FOUND);
}
```

(The `app` helper writes `itero.toml`; write the workflow before calling it, as above.)

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --test http runs_workflows`
Expected: FAIL (404 for `/runs`).

- [ ] **Step 3: Implement** `runs_page` (`SELECT id FROM runs WHERE ?1 IS NULL OR workflow = ?1 ORDER BY started DESC, id DESC LIMIT ?2`, then `run(id)` each) and the routes. Each handler opens the journal in `spawn_blocking`, like `event`. Errors are `{"error"}` with 404 or 500.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/engine/journal.rs src/engine/http.rs src/cli.rs tests/http.rs
git commit -m "Read the journal over HTTP: runs, a run's events, workflows, memory"
```

---

### Task 5: Release archives and the installer

**Files:**
- Create: `install.sh`, `scripts/test-install.sh`, `.github/workflows/release.yml`
- Modify: `Makefile` (`dist`)

**Interfaces:**
- Produces: `make dist TAG=v0.1.0` writes `dist/itero-<tag>-<target>.tar.gz` (the `itero` binary at its root, and `Itero.app/` on macOS) and `dist/itero-<tag>-<target>.sha256` (`<hex>  <archive name>`), for the host's target (`rustc -vV | sed -n 's/^host: //p'`). `install.sh [--version X.Y.Z] [--to DIR] [--uninstall] [--help]`; test seam `ITERO_INSTALL_BASE_URL` (undocumented, like videre's).

- [ ] **Step 1: Write the failing test** (`scripts/test-install.sh`, POSIX sh, run by `make test-install`)

It builds a local release and installs from it, in temp folders only, never on PATH:

```sh
#!/bin/sh
# Installs itero from a local "release" made by make dist, through the
# installer's test seam, into temp folders. Checks: a good archive installs
# and runs; a corrupted one is refused before anything is written; uninstall
# removes what install put there.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tag=v0.0.0-deneme
make -C "$root" dist TAG="$tag" DIST="$tmp/rel/$tag" >/dev/null
fail() { echo "FAIL: $*" >&2; exit 1; }

# a good install
HOME="$tmp/ev" ITERO_INSTALL_BASE_URL="file://$tmp/rel" sh "$root/install.sh" --version "$tag" --to "$tmp/bin" > "$tmp/out" 2>&1 || { cat "$tmp/out"; fail "install"; }
"$tmp/bin/itero" --version | grep -q itero || fail "the installed itero does not run"
if [ "$(uname)" = Darwin ]; then
  [ -x "$tmp/ev/Applications/Itero.app/Contents/MacOS/itero-notifier" ] || fail "no Itero.app"
fi

# a corrupted archive is refused, and nothing is installed
cp -R "$tmp/rel" "$tmp/bozuk"
for a in "$tmp/bozuk/$tag"/*.tar.gz; do printf 'bozuk' >> "$a"; done
if HOME="$tmp/ev2" ITERO_INSTALL_BASE_URL="file://$tmp/bozuk" sh "$root/install.sh" --version "$tag" --to "$tmp/bin2" > "$tmp/out2" 2>&1; then
  fail "a corrupted archive was installed"
fi
grep -q "checksum mismatch" "$tmp/out2" || { cat "$tmp/out2"; fail "no checksum message"; }
[ ! -e "$tmp/bin2/itero" ] || fail "something was installed from a corrupted archive"

# uninstall
HOME="$tmp/ev" sh "$root/install.sh" --uninstall --to "$tmp/bin" > "$tmp/out3" 2>&1 || { cat "$tmp/out3"; fail "uninstall"; }
[ ! -e "$tmp/bin/itero" ] || fail "uninstall left the binary"
[ ! -e "$tmp/ev/Applications/Itero.app" ] || fail "uninstall left Itero.app"
echo "install tests: ok"
```

Makefile:

```make
.PHONY: test-install
test-install: ## Install from a local release through the installer, in temp folders
	sh scripts/test-install.sh
```

- [ ] **Step 2: Run it to see it fail**

Run: `make test-install`
Expected: FAIL (no `dist` target, no `install.sh`).

- [ ] **Step 3: `make dist`**

```make
# A release archive for one target (this machine's unless TARGET is given),
# and its checksum: what install.sh downloads and release.yml publishes.
# Always built with --target, so a host build and a cross build are one
# recipe. TAG names the release.
TAG ?= v$(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
TARGET_DIR ?= $(or $(CARGO_TARGET_DIR),target)
DIST ?= dist
NAME = itero-$(TAG)-$(TARGET)
OUT = $(TARGET_DIR)/$(TARGET)/release
.PHONY: dist
dist: ## Build the release archive and checksum (TAG=vX.Y.Z, TARGET=triple)
	$(CARGO) build --release --target $(TARGET) --bin itero
	rm -rf $(DIST)/stage && mkdir -p $(DIST)/stage
	cp $(OUT)/itero $(DIST)/stage/itero
	@case "$(TARGET)" in *-apple-darwin) \
	  $(CARGO) build --release --target $(TARGET) --bin itero-notifier && \
	  mkdir -p $(DIST)/stage/Itero.app/Contents/MacOS && \
	  cp packaging/macos/Info.plist $(DIST)/stage/Itero.app/Contents/Info.plist && \
	  cp $(OUT)/itero-notifier $(DIST)/stage/Itero.app/Contents/MacOS/itero-notifier && \
	  codesign --force --sign - $(DIST)/stage/Itero.app ;; \
	esac
	tar czf $(DIST)/$(NAME).tar.gz -C $(DIST)/stage .
	cd $(DIST) && (shasum -a 256 $(NAME).tar.gz 2>/dev/null || sha256sum $(NAME).tar.gz) > $(NAME).sha256
	rm -rf $(DIST)/stage
```

(`dist/` goes in `.gitignore`. `make notifier` stays as it is, for development: it puts `Itero.app` beside `target/debug/itero`, where the worker looks.)

- [ ] **Step 4: `install.sh`**

Start from videre's installer (`~/projects/videre/docs/public/install`) and change only what differs:
- `REPO="erhangundogan/itero"` (one line; the org's name replaces it when the repo is made), seam `ITERO_INSTALL_BASE_URL`, names `itero-$tag-$target.tar.gz` / `.sha256`.
- `detect_target`: Intel Macs are supported (`x86_64-apple-darwin`); Rosetta check kept; Linux x86_64 and aarch64.
- After verifying and unpacking: move `itero` to `$DEST`; on Darwin, when the archive has `Itero.app`, replace `$HOME/Applications/Itero.app` with it (`rm -rf` then `mv`), and say so: `Itero.app in ~/Applications: notifications come from itero (macOS asks once to allow them)`. A downloaded file has no quarantine attribute when fetched with curl or wget, so Gatekeeper does not stop the ad hoc signed app.
- `--uninstall`: removes `$DEST/itero` (refusing a cargo or Homebrew copy, as videre does) and `$HOME/Applications/Itero.app`; reports, never deletes, workspaces' `.itero/` (it cannot know where they are: it says so).
- Leftovers and next steps: `Next:  mkdir my-workflows && cd my-workflows && echo 'port = 8787' > itero.toml && itero daemon`.
- Keep: POSIX sh, curl or wget, checksum before unpacking, latest tag from the `/releases/latest` redirect, never edits a shell profile, PATH advice, the shadowing warning.

- [ ] **Step 5: `.github/workflows/release.yml`**

On a `v*` tag: a draft release; a matrix (`macos-latest` for both Apple targets with `--target`, `ubuntu-latest` for x86_64 Linux, `ubuntu-24.04-arm` for aarch64 Linux) running `make dist TAG=${{ github.ref_name }}` with the target's `TARGET`; upload `dist/*.tar.gz` and `dist/*.sha256`; then undraft. Pin actions by commit SHA as videre does. It first runs when the repository exists; until then it is checked only by reading it against `make dist` and `install.sh` names.

(Each matrix entry runs `rustup target add $TARGET` first, then `make dist TAG=... TARGET=...`.)

- [ ] **Step 6: Run the install tests, on macOS and in Linux**

Run: `make test-install`
Expected: `install tests: ok`.

Linux (Docker Desktop):

```bash
docker run --rm -v ~/projects/itero:/src -v itero-target:/target -v itero-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.96 bash -c "make test-install"
```

(`dist` finds the build under `$(CARGO_TARGET_DIR)` when it is set, as it is in the container.)

Expected: `install tests: ok`.

- [ ] **Step 7: Commit**

```bash
git add install.sh scripts/test-install.sh .github/workflows/release.yml Makefile .gitignore
git commit -m "Release archives and the installer: curl | sh, checked, with Itero.app on macOS"
```

---

### Task 6: README, design, and a real try

**Files:**
- Modify: `README.md`, `docs/design.md`

- [ ] **Step 1: README**: the API in a table (routes, one line each), `itero emit`, event triggers with a short example, the install line (`curl -fsSL https://raw.githubusercontent.com/erhangundogan/itero/main/install.sh | sh`, to change with the org), and that the server is local only. No em dash, no AI attribution.

- [ ] **Step 2: design.md**: section 4 (triggers: `schedule` or `on`; the event's data is the run's input; workspace `triggers/` in iteration 3), section 7 (the API's routes, `itero emit`, the Host and Content-Type guards), section 9 gets a line for iteration 2.

- [ ] **Step 3: Everything, both systems**

Run: `cargo test --no-fail-fast && make fmt-check && make lint-check && make test-install`, then the same `cargo test` and `make test-install` in the Linux container.
Expected: all pass.

- [ ] **Step 4: A real try**: a workspace in the scratchpad with a workflow `{"on": "deneme.geldi"}` whose one action is `notify@1` with `${input.metin}`; `itero daemon` in it (background); then

```bash
curl -s -X POST http://127.0.0.1:8787/events -H 'content-type: application/json' -d '{"type": "deneme.geldi", "data": {"metin": "itero: olay geldi"}}'
curl -s http://127.0.0.1:8787/runs
```

Expected: a notification from itero within a second (the post wakes the loop), and the run `done` in `/runs`. Stop the daemon.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/design.md
git commit -m "README and design: the API, event triggers, the installer"
```
