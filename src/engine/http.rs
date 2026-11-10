//! The daemon's HTTP API on 127.0.0.1:<port>: its health, events in,
//! what the journal holds (runs, workflows, memory), and the events and
//! the log followed live. Local only: the Host header must name this port
//! on 127.0.0.1 or localhost.
use super::daemon::Health;
use super::journal::Journal;
use crate::answer::workspace::{RunBy, Runner};
use crate::workspace::Workspace;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

/// What the server shares with the daemon's loop.
pub struct Engine {
    pub health: Mutex<Health>,
    /// Wakes the loop for an event that just arrived.
    pub wake: Sender<()>,
    /// Who follows the workspace now (`GET /events/stream`, `/log/stream`).
    pub streams: Arc<super::follow::Streams>,
}

#[derive(Clone)]
struct Shared {
    ws: Arc<Workspace>,
    engine: Arc<Engine>,
    /// The port it serves on: the only Host it answers.
    port: u16,
    /// Its address, ending in `/`, and whose daemon serves it.
    api: String,
    run_by: RunBy,
}

type Reply = (StatusCode, Json<Value>);

fn fail(code: StatusCode, message: impl Into<String>) -> Reply {
    (code, Json(json!({ "error": message.into() })))
}

/// The API's routes, for the workspace `ws`, served on `port` at `api`
/// (ending in `/`) by the daemon `run_by` says.
pub fn router(ws: Workspace, engine: Arc<Engine>, port: u16, api: String, run_by: RunBy) -> Router {
    let s = Shared {
        ws: Arc::new(ws),
        engine,
        port,
        api,
        run_by,
    };
    Router::new()
        .route("/health", get(health))
        .route("/events", post(event).get(queued))
        .route("/events/stream", get(follow_events))
        .route("/log/stream", get(follow_log))
        .route("/events/{id}", delete(cancel_event))
        .route("/runs", get(runs))
        .route("/runs/{id}", get(run))
        .route("/runs/{id}/stop", post(stop_run))
        .route("/workflows", get(workflows))
        .route("/actions", get(actions))
        .route("/actions/errors", get(action_errors))
        .route("/connectors", get(connectors))
        .route("/check", get(check))
        .route("/info", get(info))
        .route("/which/{name}", get(which))
        .route("/browser/tabs", post(open_watch))
        .layer(middleware::from_fn_with_state(s.clone(), local_only))
        .with_state(s)
}

/// `POST /browser/tabs`, `{"headless": true}` or `{}`: a browser window
/// for a connector to watch (`itero browser watch`), answered with one
/// line, its `id`, `ws` (its DevTools address) and `mode`. The response
/// then stays open, and the window lives as long as it does: when the
/// connection closes, however the connector ended, the window is closed.
async fn open_watch(
    State(s): State<Shared>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Response, Reply> {
    let Json(b) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    let headless = match b.get("headless") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(h)) => Some(*h),
        Some(v) => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                format!("headless is true or false, not {v}"),
            ));
        }
    };
    let ws = (*s.ws).clone();
    let opened = tokio::task::spawn_blocking(move || {
        crate::browser::keeper::Keeper::global().open_watch(&ws, headless)
    })
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|why| fail(StatusCode::INTERNAL_SERVER_ERROR, why))?;
    let (mode, tab) = opened;
    let line = json!({ "id": tab.id, "ws": tab.ws, "mode": mode.name() }).to_string() + "\n";
    let held = Held {
        line: Some(line.into()),
        _window: Window(tab.id),
    };
    Ok((
        StatusCode::CREATED,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        axum::body::Body::from_stream(held),
    )
        .into_response())
}

/// A watched window's response: its line, then nothing, for as long as
/// the connection lasts. The server drops it when the connection closes.
struct Held {
    line: Option<axum::body::Bytes>,
    _window: Window,
}

impl futures_core::Stream for Held {
    type Item = Result<axum::body::Bytes, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.get_mut().line.take() {
            Some(line) => std::task::Poll::Ready(Some(Ok(line))),
            // nothing more, for good: the connection's end ends it
            None => std::task::Poll::Pending,
        }
    }
}

/// A watched window, closed when dropped (the connection that held it has
/// closed); closing talks to Chrome, so not on the server's own threads.
struct Window(String);

impl Drop for Window {
    fn drop(&mut self) {
        let id = std::mem::take(&mut self.0);
        std::thread::spawn(move || {
            if let Err(e) = crate::browser::keeper::Keeper::global().close_watch(&id) {
                eprintln!("itero: cannot close a watched window: {e}");
            }
        });
    }
}

/// Refuses a request whose Host is not this server's own address: a web
/// page that rebinds a name to 127.0.0.1 still sends its own name.
async fn local_only(State(s): State<Shared>, req: Request, next: Next) -> Response {
    local_only_on(State(s.port), req, next).await
}

/// `local_only`, for a server on `port`.
async fn local_only_on(State(port): State<u16>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let ok = [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
        .iter()
        .any(|h| h == host);
    if !ok {
        return fail(
            StatusCode::FORBIDDEN,
            format!("Host {host:?} is not this server: use 127.0.0.1:{port}"),
        )
        .into_response();
    }
    next.run(req).await
}

async fn health(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    let h = s
        .engine
        .health
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let workspace = blocking(&s, |ws| {
        let w = Journal::open(ws)
            .and_then(|j| j.workspace())
            .map_err(internal)?;
        let mut v = json!(w);
        v["path"] = json!(ws.root.display().to_string());
        v["meta"] = json!(ws.meta);
        Ok(v)
    })
    .await?;
    Ok(Json(json!({
        "ok": true,
        "pid": h.daemon.pid,
        "workspace": workspace.0,
        "daemon": h.daemon,
        "started": h.started,
        "passes": h.passes,
        "last_pass": h.last_pass,
        "last_error": h.last_error,
        "connectors": h.connectors,
        "streams": s.engine.streams.health(),
        "ai": crate::ai::keeper::Keeper::global().health(),
        "browser": crate::browser::keeper::Keeper::global().health(),
    })))
}

/// An event as it is posted.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Posted {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    data: Option<Value>,
}

#[derive(serde::Deserialize)]
struct EventQuery {
    /// Seconds to wait for the runs it starts to end.
    wait: Option<u64>,
}

/// The longest an answer is waited for, in seconds.
const MAX_WAIT: u64 = 600;

/// Asks a run to stop: 202; 409 when it has ended, 404 when there is none.
async fn stop_run(State(s): State<Shared>, Path(id): Path<String>) -> Result<Reply, Reply> {
    let wake = s.engine.wake.clone();
    let asked = answered(&s, move |ws| crate::answer::runs::stop(ws, &id)).await?;
    let _ = wake.send(());
    Ok((StatusCode::ACCEPTED, asked))
}

/// `GET /events`: the events for the workspace no daemon has handled yet.
async fn queued(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    answered(&s, crate::answer::events::queued).await
}

/// `DELETE /events/{id}`: takes back an event no daemon has handled yet.
async fn cancel_event(
    State(s): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Reply> {
    answered(&s, move |ws| crate::answer::events::cancel(ws, &id)).await
}

/// `POST /events`: an event sent, 202 with what it will do; with `?wait=`,
/// its runs once they ended (200), or those so far when the time ran out
/// (202).
async fn event(
    State(s): State<Shared>,
    Query(q): Query<EventQuery>,
    body: Result<Json<Posted>, JsonRejection>,
) -> Result<Reply, Reply> {
    use crate::answer::events::{Emit, Emitted};
    // Json refuses any Content-Type but application/json (415): a page on
    // another site cannot post one without a preflight, never granted here
    let Json(p) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    let e = Emit {
        kind: p.kind,
        data: p.data,
        wait: q
            .wait
            .map(|secs| std::time::Duration::from_secs(secs.min(MAX_WAIT))),
    };
    let (ws, wake) = (s.ws.clone(), s.engine.wake.clone());
    // the journal may wait on a busy SQLite: off the async threads; the
    // loop is woken before any waiting (one that has stopped finds the
    // event in the journal when it starts)
    let sent = tokio::task::spawn_blocking(move || {
        crate::answer::events::emit(&ws, &e, || {
            let _ = wake.send(());
        })
    })
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(fault)?;
    let code = match &sent {
        Emitted::Waited(w) if w.how() != super::events::Waited::TimedOut => StatusCode::OK,
        _ => StatusCode::ACCEPTED,
    };
    let v = serde_json::to_value(&sent).map_err(|e| internal(e.into()))?;
    Ok((code, Json(v)))
}

/// `GET /events/stream`: each event that comes into the workspace, one
/// JSON line, as it comes; see `follow`.
async fn follow_events(
    State(s): State<Shared>,
    Query(q): Query<Vec<(String, String)>>,
) -> Result<Response, Reply> {
    follow(s, &q, super::follow::Kind::Events).await
}

/// `GET /log/stream`: each new line of the workspace's log, as it is
/// written.
async fn follow_log(
    State(s): State<Shared>,
    Query(q): Query<Vec<(String, String)>>,
) -> Result<Response, Reply> {
    follow(s, &q, super::follow::Kind::Log).await
}

async fn follow(
    s: Shared,
    q: &[(String, String)],
    kind: super::follow::Kind,
) -> Result<Response, Reply> {
    use super::follow::{self, Kind, Refused};
    let asked = follow::asked(q, kind).map_err(|why| fail(StatusCode::BAD_REQUEST, why))?;
    let (ws, streams) = (s.ws.clone(), s.engine.streams.clone());
    // the journal may wait on a busy SQLite: off the async threads
    let body = tokio::task::spawn_blocking(move || match kind {
        Kind::Events => follow::events(&ws, asked, streams),
        Kind::Log => follow::log(&ws, asked, streams),
    })
    .await
    .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|r| match r {
        Refused::Gone(why) => fail(StatusCode::GONE, why),
        Refused::Internal(e) => internal(e),
    })?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        axum::body::Body::from_stream(body),
    )
        .into_response())
}

/// An event that was not sent, as a reply.
fn not_sent(e: super::events::EmitError) -> Reply {
    use super::events::EmitError::*;
    let code = match &e {
        Bad(_) => StatusCode::BAD_REQUEST,
        Nowhere(_) => StatusCode::CONFLICT,
        Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    fail(code, e.to_string())
}

/// Runs `f` on the workspace off the async threads (the journal may wait
/// on a busy SQLite): its value as JSON, or its error as a reply.
async fn blocking(
    s: &Shared,
    f: impl FnOnce(&Workspace) -> Result<Value, Reply> + Send + 'static,
) -> Result<Json<Value>, Reply> {
    let ws = s.ws.clone();
    tokio::task::spawn_blocking(move || f(&ws))
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
}

fn internal(e: anyhow::Error) -> Reply {
    fail(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
}

/// A command's answer as a route gives it: computed by `f` off the async
/// threads, the same function the CLI calls.
async fn answered<T: serde::Serialize>(
    s: &Shared,
    f: impl FnOnce(&Workspace) -> Result<T, crate::answer::Fault> + Send + 'static,
) -> Result<Json<Value>, Reply> {
    blocking(s, move |ws| {
        let a = f(ws).map_err(fault)?;
        serde_json::to_value(a).map_err(|e| internal(e.into()))
    })
    .await
}

/// No answer, as a reply: its status and its message.
fn fault(f: crate::answer::Fault) -> Reply {
    let code = StatusCode::from_u16(f.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let (code, mut body) = fail(code, f.to_string());
    // a conflict's fields: what is in the way
    if let crate::answer::Fault::Conflict(_, why) = f
        && let Value::Object(b) = &mut body.0
    {
        b.extend(why);
    }
    (code, body)
}

/// `GET /runs`: the runs `q` asks for, newest first; 50 unless `limit`.
async fn runs(
    State(s): State<Shared>,
    Query(mut q): Query<crate::answer::runs::Query>,
) -> Result<Json<Value>, Reply> {
    q.limit = q.limit.or(Some(50));
    answered(&s, move |ws| crate::answer::runs::list(ws, &q)).await
}

/// `GET /runs/{id}`: a run's story, by id or name.
async fn run(State(s): State<Shared>, Path(id): Path<String>) -> Result<Json<Value>, Reply> {
    answered(&s, move |ws| crate::answer::runs::one(ws, &id)).await
}

/// `GET /actions`: each action and its files, the built-in ones too.
async fn actions(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    answered(&s, crate::answer::kinds::actions).await
}

/// `GET /connectors`: each connector, once for each set of args.
async fn connectors(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    answered(&s, crate::answer::kinds::connectors).await
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorsQuery {
    /// One action's failures, instead of each action's.
    action: Option<String>,
    /// At most this many: as many as `itero ws ac errors` unless given.
    limit: Option<usize>,
}

/// `GET /actions/errors`: which actions break runs; with `action`, that
/// action's failures, newest first.
async fn action_errors(
    State(s): State<Shared>,
    Query(q): Query<ErrorsQuery>,
) -> Result<Json<Value>, Reply> {
    use crate::answer::kinds;
    let limit = q.limit.unwrap_or(kinds::ERRORS_SHOWN);
    match q.action {
        Some(a) => answered(&s, move |ws| kinds::failures_of(ws, &a, limit)).await,
        None => answered(&s, move |ws| kinds::errors_by_action(ws, limit)).await,
    }
}

/// `GET /workflows`: each workflow, and whether a daemon takes events.
async fn workflows(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    answered(&s, crate::answer::workflows::list).await
}

/// `GET /check`: what is wrong in the workspace, every problem, and what
/// it found. A report with errors is still 200: the request did not fail.
async fn check(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    answered(&s, crate::answer::workspace::check).await
}

/// `GET /info`: the workspace, its limits, and this daemon running it.
async fn info(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    let daemon = Runner::Running {
        daemon: s
            .engine
            .health
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .daemon
            .clone(),
        running: true,
        port: s.port,
        api: s.api.clone(),
        run_by: s.run_by,
    };
    answered(&s, move |ws| crate::answer::workspace::info(ws, daemon)).await
}

/// `GET /which/{name}`: where the workflow, action and connector so named
/// are; `%23name` (`#name`) looks among the actions only.
async fn which(State(s): State<Shared>, Path(name): Path<String>) -> Result<Json<Value>, Reply> {
    answered(&s, move |ws| crate::answer::workspace::which(ws, &name)).await
}

/// The daemon's socket on 127.0.0.1, bound before anything else starts:
/// the manifest's port; else the one kept from the last start; else (none
/// kept, or it is taken) one the OS gives, which is kept. With a note when
/// the kept one was taken.
pub fn listen(ws: &Workspace) -> anyhow::Result<(std::net::TcpListener, Option<String>)> {
    let bind = |p: u16| std::net::TcpListener::bind(("127.0.0.1", p));
    if let Some(p) = ws.port {
        let l = bind(p).map_err(|e| {
            anyhow::anyhow!(
                "cannot listen on 127.0.0.1:{p}: {e} (another itero daemon? set another port in itero.config.json, or none)"
            )
        })?;
        return Ok((l, None));
    }
    // port 0: the OS gives a free one, held from then on
    let given = || bind(0).map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1: {e}"));
    let (listener, note) = match ws.saved_port().map(|p| (p, bind(p))) {
        Some((_, Ok(l))) => (l, None),
        Some((p, Err(e))) => (
            given()?,
            Some(format!("port {p} is taken ({e}): the OS gave another")),
        ),
        None => (given()?, None),
    };
    ws.save_port(listener.local_addr()?.port())?;
    Ok((listener, note))
}

/// Serves `router` on `listener` until the process ends.
pub fn serve(router: Router, listener: std::net::TcpListener) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        axum::serve(listener, router).await?;
        Ok(())
    })
}

/// What the machine's routes share.
#[derive(Clone)]
struct MachineShared {
    machine: Arc<super::machine::Machine>,
    port: u16,
}

/// The machine's routes, for `itero daemon --all` on `port`: its health,
/// its workspaces, and each running workspace's own routes under
/// `/ws/<name>/`.
pub fn machine_router(machine: Arc<super::machine::Machine>, port: u16) -> Router {
    let s = MachineShared { machine, port };
    Router::new()
        .route("/health", get(machine_health))
        .route("/workspaces", get(workspaces))
        .route("/events", post(machine_event))
        .route("/ws/{name}", axum::routing::any(in_workspace))
        .route("/ws/{name}/{*rest}", axum::routing::any(in_workspace))
        .layer(middleware::from_fn_with_state(port, local_only_on))
        .with_state(s)
}

async fn machine_health(State(s): State<MachineShared>) -> Json<Value> {
    let d = super::daemon::Daemon::current();
    Json(json!({
        "ok": true,
        "pid": d.pid,
        "daemon": d,
        "workspaces": s.machine.health(),
        "ai": crate::ai::keeper::Keeper::global().health(),
        "browser": crate::browser::keeper::Keeper::global().health(),
    }))
}

async fn workspaces(State(s): State<MachineShared>) -> Json<Value> {
    Json(Value::Array(s.machine.states()))
}

/// `POST /events`: an event for every workspace on the machine's list, as
/// `itero emit` sends it. Its answer has the id it has in each, and why it
/// was not sent to any that could not take it: 202 when it reached one,
/// 409 when none could take it (an empty list, every folder missing), 500
/// when a journal there could not save it.
async fn machine_event(
    Query(q): Query<EventQuery>,
    body: Result<Json<Posted>, JsonRejection>,
) -> Result<Reply, Reply> {
    let Json(p) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    if q.wait.is_some() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "wait is for one workspace: POST /ws/<name>/events?wait=",
        ));
    }
    let now = super::daemon::stamp(chrono::Utc::now());
    let sent = tokio::task::spawn_blocking(move || super::machine::emit_all(&p.kind, p.data, &now))
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(not_sent)?;
    let (mut ids, mut errors) = (serde_json::Map::new(), serde_json::Map::new());
    let mut failed = false;
    for s in sent {
        use super::machine::To;
        match s.to {
            To::Sent(_, id) => ids.insert(s.name, json!(id)),
            To::Missing(why) => errors.insert(s.name, json!(why)),
            To::Failed(why) => {
                failed = true;
                errors.insert(s.name, json!(why))
            }
        };
    }
    // sent nowhere: a journal that failed is the daemon's error; only
    // missing folders, the list's
    let code = match (ids.is_empty(), failed) {
        (false, _) => StatusCode::ACCEPTED,
        (true, true) => StatusCode::INTERNAL_SERVER_ERROR,
        (true, false) => StatusCode::CONFLICT,
    };
    let mut answer = json!({ "ids": ids });
    if !errors.is_empty() {
        answer["errors"] = json!(errors);
    }
    Ok((code, Json(answer)))
}

/// A request to `/ws/<name>/<rest>`, answered by that workspace's routes
/// as `/<rest>`.
async fn in_workspace(
    State(s): State<MachineShared>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    req: Request,
) -> Response {
    use tower::ServiceExt;
    let name = params.get("name").cloned().unwrap_or_default();
    let Some((ws, engine)) = s.machine.find(&name) else {
        return fail(
            StatusCode::NOT_FOUND,
            format!("no workspace {name} running here: itero workspace list"),
        )
        .into_response();
    };
    // the rest of the path as it was sent, still percent-encoded: decoded,
    // a `%23` would become `#` and end the path there
    let rest = req.uri().path().splitn(4, '/').nth(3).unwrap_or_default();
    let path = match req.uri().query() {
        Some(q) => format!("/{rest}?{q}"),
        None => format!("/{rest}"),
    };
    let uri: axum::http::Uri = match path.parse() {
        Ok(uri) => uri,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    // a request of its own, as if sent to the workspace's port: the
    // parameters this route took (`name`, `rest`) stay behind, or the
    // workspace's route would see them beside its own
    let (parts, body) = req.into_parts();
    let mut inner = Request::new(body);
    *inner.method_mut() = parts.method;
    *inner.uri_mut() = uri;
    *inner.version_mut() = parts.version;
    *inner.headers_mut() = parts.headers;
    let api = format!("http://127.0.0.1:{}/ws/{name}/", s.port);
    match router(ws, engine, s.port, api, RunBy::Machine)
        .oneshot(inner)
        .await
    {
        Ok(r) => r,
        Err(e) => match e {},
    }
}

/// The machine's port: `port` when given, else the one kept from the last
/// start, else (none kept, or taken) one the OS gives; the one bound is
/// kept in itero's state folder.
pub fn listen_machine(port: Option<u16>) -> anyhow::Result<std::net::TcpListener> {
    let bind = |p: u16| std::net::TcpListener::bind(("127.0.0.1", p));
    let file = crate::machine::state_dir()?.join("port");
    let listener = match port {
        Some(p) => bind(p).map_err(|e| {
            anyhow::anyhow!("cannot listen on 127.0.0.1:{p}: {e} (another itero daemon?)")
        })?,
        None => {
            let kept = std::fs::read_to_string(&file)
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok());
            match kept.map(bind) {
                Some(Ok(l)) => l,
                _ => bind(0).map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1: {e}"))?,
            }
        }
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, listener.local_addr()?.port().to_string())?;
    Ok(listener)
}

/// The machine daemon's port, when one answers there.
pub fn machine_port() -> Option<u16> {
    let file = crate::machine::state_dir().ok()?.join("port");
    std::fs::read_to_string(file).ok()?.trim().parse().ok()
}
