//! The daemon's HTTP API on 127.0.0.1:<port>: its health, events in, and
//! what the journal holds (runs, workflows, memory). Local only: the Host
//! header must name this port on 127.0.0.1 or localhost.
use super::daemon::Health;
use super::journal::Journal;
use crate::workspace::Workspace;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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
struct Shared {
    ws: Arc<Workspace>,
    engine: Arc<Engine>,
}

type Reply = (StatusCode, Json<Value>);

fn fail(code: StatusCode, message: impl Into<String>) -> Reply {
    (code, Json(json!({ "error": message.into() })))
}

/// The API's routes, for the workspace `ws`.
pub fn router(ws: Workspace, engine: Arc<Engine>) -> Router {
    let s = Shared {
        ws: Arc::new(ws),
        engine,
    };
    Router::new()
        .route("/health", get(health))
        .route("/events", post(event))
        .route("/runs", get(runs))
        .route("/runs/{id}", get(run))
        .route("/workflows", get(workflows))
        .route("/workflows/{name}/memory", get(memory))
        .layer(middleware::from_fn_with_state(s.clone(), local_only))
        .with_state(s)
}

/// Refuses a request whose Host is not this server's own address: a web
/// page that rebinds a name to 127.0.0.1 still sends its own name.
async fn local_only(State(s): State<Shared>, req: Request, next: Next) -> Response {
    let port = s.ws.port;
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

async fn health(State(s): State<Shared>) -> Json<Value> {
    let h = s
        .engine
        .health
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
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

/// An event as it is posted.
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

async fn event(
    State(s): State<Shared>,
    body: Result<Json<Posted>, JsonRejection>,
) -> Result<Reply, Reply> {
    // Json refuses any Content-Type but application/json (415): a page on
    // another site cannot post one without a preflight, never granted here
    let Json(p) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    let ws = s.ws.clone();
    let now = super::daemon::stamp(chrono::Utc::now());
    // the journal may wait on a busy SQLite: off the async threads
    let id = tokio::task::spawn_blocking(move || {
        super::events::emit(&ws, &p.kind, p.data, p.run.as_deref(), &now)
    })
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

/// A run as lists show it.
fn run_row(r: &super::journal::Run) -> Value {
    json!({
        "id": r.id,
        "workflow": r.workflow,
        "state": r.runtime.state,
        "started": r.started,
        "ended": r.ended,
    })
}

#[derive(serde::Deserialize)]
struct RunsQuery {
    workflow: Option<String>,
    limit: Option<usize>,
}

async fn runs(State(s): State<Shared>, Query(q): Query<RunsQuery>) -> Result<Json<Value>, Reply> {
    blocking(&s, move |ws| {
        let j = Journal::open(ws).map_err(internal)?;
        let runs = j
            .runs_page(q.workflow.as_deref(), q.limit.unwrap_or(50))
            .map_err(internal)?;
        Ok(Value::Array(runs.iter().map(run_row).collect()))
    })
    .await
}

async fn run(State(s): State<Shared>, Path(id): Path<String>) -> Result<Json<Value>, Reply> {
    blocking(&s, move |ws| {
        let j = Journal::open(ws).map_err(internal)?;
        let r = j
            .run(&id)
            .map_err(internal)?
            .ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no run {id}")))?;
        let events: Vec<Value> = j
            .events(Some(&super::journal::subject(&id)))
            .map_err(internal)?
            .iter()
            .map(|e| {
                json!({
                    "seq": e.seq,
                    "type": e.event.kind,
                    "time": e.event.time,
                    "handled": e.handled,
                    "error": e.error,
                    "data": e.event.data,
                })
            })
            .collect();
        let mut v = run_row(&r);
        v["args"] = Value::Object(r.args.clone());
        v["events"] = Value::Array(events);
        Ok(v)
    })
    .await
}

async fn workflows(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    blocking(&s, |ws| {
        let flows = ws.workflows().map_err(internal)?;
        let paused = Journal::open(ws)
            .and_then(|j| j.paused())
            .map_err(internal)?;
        Ok(Value::Array(
            flows
                .iter()
                .map(|(_, f)| {
                    json!({
                        "name": f.name,
                        "version": crate::workflow::version(f),
                        "triggers": f.triggers,
                        "paused": paused.iter().find(|p| p.workflow == f.name),
                    })
                })
                .collect(),
        ))
    })
    .await
}

async fn memory(State(s): State<Shared>, Path(name): Path<String>) -> Result<Json<Value>, Reply> {
    blocking(&s, move |ws| {
        let known = ws
            .workflows()
            .map_err(internal)?
            .iter()
            .any(|(_, f)| f.name == name);
        if !known {
            return Err(fail(StatusCode::NOT_FOUND, format!("no workflow {name}")));
        }
        let j = Journal::open(ws).map_err(internal)?;
        Ok(Value::Object(j.memory(&name).map_err(internal)?))
    })
    .await
}

/// Serves `router` on 127.0.0.1:`port` until the process ends.
pub fn serve(router: Router, port: u16) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "cannot listen on 127.0.0.1:{port}: {e} (another itero daemon? set port in itero.toml)"
                )
            })?;
        axum::serve(listener, router).await?;
        Ok(())
    })
}
