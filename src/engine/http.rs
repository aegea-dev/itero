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
    /// The port it serves on: the only Host it answers.
    port: u16,
}

type Reply = (StatusCode, Json<Value>);

fn fail(code: StatusCode, message: impl Into<String>) -> Reply {
    (code, Json(json!({ "error": message.into() })))
}

/// The API's routes, for the workspace `ws`, served on `port`.
pub fn router(ws: Workspace, engine: Arc<Engine>, port: u16) -> Router {
    let s = Shared {
        ws: Arc::new(ws),
        engine,
        port,
    };
    Router::new()
        .route("/health", get(health))
        .route("/events", post(event))
        .route("/runs", get(runs))
        .route("/runs/{id}", get(run))
        .route("/workflows", get(workflows))
        .layer(middleware::from_fn_with_state(s.clone(), local_only))
        .with_state(s)
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
        "ai": crate::ai::keeper::Keeper::global().health(),
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

async fn event(
    State(s): State<Shared>,
    Query(q): Query<EventQuery>,
    body: Result<Json<Posted>, JsonRejection>,
) -> Result<Reply, Reply> {
    // Json refuses any Content-Type but application/json (415): a page on
    // another site cannot post one without a preflight, never granted here
    let Json(p) = body.map_err(|r| fail(r.status(), r.body_text()))?;
    let ws = s.ws.clone();
    let now = super::daemon::stamp(chrono::Utc::now());
    // the journal may wait on a busy SQLite: off the async threads
    let id = tokio::task::spawn_blocking(move || super::events::emit(&ws, &p.kind, p.data, &now))
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| {
            use super::events::EmitError::*;
            let code = match &e {
                Bad(_) => StatusCode::BAD_REQUEST,
                Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            fail(code, e.to_string())
        })?;
    // a loop that has stopped finds the event in the journal when it starts
    let _ = s.engine.wake.send(());
    let Some(secs) = q.wait else {
        return Ok((StatusCode::ACCEPTED, Json(json!({ "id": id }))));
    };
    // its runs' end: 200 once they all ended, 202 with those so far when
    // the time ran out
    let ws = s.ws.clone();
    let wait = std::time::Duration::from_secs(secs.min(MAX_WAIT));
    let (mut answer, how) =
        tokio::task::spawn_blocking(move || super::events::wait_for(&ws, &id, wait))
            .await
            .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(internal)?;
    answer["id"] = answer["message"].clone();
    let code = match how {
        super::events::Waited::TimedOut => StatusCode::ACCEPTED,
        _ => StatusCode::OK,
    };
    Ok((code, Json(answer)))
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
        "name": r.name,
        "workflow": r.workflow,
        "status": super::steps::status(&r.runtime),
        "started": r.started,
        "ended": r.ended,
    })
}

#[derive(serde::Deserialize)]
struct RunsQuery {
    workflow: Option<String>,
    /// `running`, `waiting`, `done`, `failed`, `error` or `split`.
    status: Option<String>,
    limit: Option<usize>,
    /// The internal workflows' runs too.
    #[serde(default)]
    all: bool,
}

async fn runs(State(s): State<Shared>, Query(q): Query<RunsQuery>) -> Result<Json<Value>, Reply> {
    blocking(&s, move |ws| {
        let j = Journal::open(ws).map_err(internal)?;
        let limit = q.limit.unwrap_or(50);
        let apart = if q.all {
            vec![]
        } else {
            ws.internal().map_err(internal)?
        };
        let runs = match q.status.as_deref() {
            // under way: few, and their status is not a column
            Some(st @ ("running" | "waiting")) => j
                .active_runs()
                .map_err(internal)?
                .into_iter()
                .rev()
                .filter(|r| super::steps::status(&r.runtime) == st)
                .filter(|r| q.workflow.as_ref().is_none_or(|w| *w == r.workflow))
                .filter(|r| !apart.contains(&r.workflow))
                .take(limit)
                .collect(),
            ended => j
                .runs_page(q.workflow.as_deref(), ended, &apart, limit)
                .map_err(internal)?,
        };
        Ok(Value::Array(runs.iter().map(run_row).collect()))
    })
    .await
}

async fn run(State(s): State<Shared>, Path(id): Path<String>) -> Result<Json<Value>, Reply> {
    blocking(&s, move |ws| {
        let j = Journal::open(ws).map_err(internal)?;
        // by id or name; of two with one name, the newest
        let r = j
            .run_by(&id)
            .map_err(internal)?
            .into_iter()
            .next()
            .ok_or_else(|| fail(StatusCode::NOT_FOUND, format!("no run {id}")))?;
        let id = r.id.clone();
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
        if v["status"] == "running" {
            v["progress"] = Value::Array(super::progress::of_run(&ws.root, &id));
        }
        v["input"] = r.input.clone();
        v["state"] = Value::Object(r.runtime.state.clone());
        v["events"] = Value::Array(events);
        Ok(v)
    })
    .await
}

async fn workflows(State(s): State<Shared>) -> Result<Json<Value>, Reply> {
    blocking(&s, |ws| {
        let flows = ws.flows().map_err(internal)?;
        let files = ws.flow_files().map_err(internal)?;
        Ok(Value::Array(
            flows
                .iter()
                .map(|(name, f)| {
                    json!({
                        "name": name,
                        // none for a built-in one
                        "path": files.get(name).map(|p| ws.shown(p)),
                        "internal": f.meta.internal,
                        "on": {
                            "events": f.on.events,
                            "schedules": f.on.schedules,
                        },
                    })
                })
                .collect(),
        ))
    })
    .await
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
    }))
}

async fn workspaces(State(s): State<MachineShared>) -> Json<Value> {
    Json(Value::Array(s.machine.states()))
}

/// A request to `/ws/<name>/<rest>`, answered by that workspace's routes
/// as `/<rest>`.
async fn in_workspace(
    State(s): State<MachineShared>,
    Path(params): Path<std::collections::HashMap<String, String>>,
    mut req: Request,
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
    let rest = params.get("rest").cloned().unwrap_or_default();
    let path = match req.uri().query() {
        Some(q) => format!("/{rest}?{q}"),
        None => format!("/{rest}"),
    };
    match path.parse() {
        Ok(uri) => *req.uri_mut() = uri,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
    match router(ws, engine, s.port).oneshot(req).await {
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
