//! The OpenAI API on a Unix socket. One engine thread owns the model and
//! answers one request at a time; the others wait their turn. It ends
//! when nothing has been asked for `idle`.
use crate::api::{self, ApiError};
use crate::engine::{Engine, EngineError, Finish, Piece, Prompt};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc as tmpsc;

/// What it is told by whoever starts it: where to listen, how long to
/// stay up with nothing to do, and the index of the machine's models.
#[derive(Debug)]
pub struct Config {
    pub socket: PathBuf,
    pub idle: Duration,
    pub index: PathBuf,
}

/// The config from the environment: `ITERO_AI_SOCKET`, `ITERO_AI_IDLE`
/// (seconds) and `ITERO_AI_INDEX`, all needed.
pub fn config_from_env() -> anyhow::Result<Config> {
    config_from(|k| std::env::var(k).ok())
}

/// The config from `get`, which reads a variable.
pub fn config_from(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Config> {
    let need = |k: &str| {
        get(k).ok_or_else(|| anyhow::anyhow!("{k} is not set: itero starts itero-ai with it"))
    };
    let idle = need("ITERO_AI_IDLE")?;
    Ok(Config {
        socket: need("ITERO_AI_SOCKET")?.into(),
        idle: Duration::from_secs(
            idle.parse()
                .map_err(|_| anyhow::anyhow!("ITERO_AI_IDLE is seconds, not {idle}"))?,
        ),
        index: need("ITERO_AI_INDEX")?.into(),
    })
}

enum Job {
    Complete(Prompt, tmpsc::UnboundedSender<Result<Step, EngineError>>),
    Inspect(PathBuf, tmpsc::UnboundedSender<Result<Value, EngineError>>),
}

enum Step {
    Piece(Piece),
    Done(Finish),
}

#[derive(Clone)]
struct Shared {
    index: PathBuf,
    jobs: mpsc::Sender<Job>,
    /// Requests asked and not answered yet.
    waiting: Arc<AtomicUsize>,
    /// When the last request came or ended (seconds).
    last: Arc<AtomicU64>,
    loaded: Arc<Mutex<Option<String>>>,
}

impl Shared {
    fn touch(&self) {
        self.last.store(now(), Ordering::SeqCst);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Says one thing to whoever started it: a JSON object on a line of
/// stdout (`started`, `model.loaded`).
pub fn tell(v: Value) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn engine_thread(mut engine: Box<dyn Engine>, jobs: mpsc::Receiver<Job>, s: Shared) {
    for job in jobs {
        match job {
            Job::Complete(p, tx) => {
                let before = engine.loaded();
                let started = std::time::Instant::now();
                let r = engine.complete(&p, &mut |piece| tx.send(Ok(Step::Piece(piece))).is_ok());
                let after = engine.loaded();
                if after != before
                    && let Some(m) = &after
                {
                    tell(json!({ "event": "model.loaded", "model": m,
                                 "seconds": started.elapsed().as_secs_f64() }));
                }
                *s.loaded.lock().unwrap_or_else(|e| e.into_inner()) = after;
                let _ = tx.send(r.map(Step::Done));
            }
            Job::Inspect(path, tx) => {
                let _ = tx.send(engine.inspect(&path));
            }
        }
    }
}

fn status(e: &EngineError) -> u16 {
    match e {
        EngineError::NotFound(_) => 404,
        EngineError::BadRequest(_) => 400,
        EngineError::Failed(_) => 500,
    }
}

fn error(status: u16, message: String, param: Option<&str>) -> Response {
    let e = ApiError {
        status,
        message,
        param: param.map(String::from),
    };
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(api::error_body(&e))).into_response()
}

fn engine_error(e: &EngineError) -> Response {
    let param = matches!(e, EngineError::NotFound(_)).then_some("model");
    error(status(e), e.to_string(), param)
}

const STOPPED: &str = "the engine stopped";

/// Counts a request as waiting while it lives.
struct Waiting(Shared);

impl Waiting {
    fn new(s: &Shared) -> Waiting {
        s.waiting.fetch_add(1, Ordering::SeqCst);
        s.touch();
        Waiting(s.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

async fn chat(State(s): State<Shared>, Json(raw): Json<Value>) -> Response {
    let waiting = Waiting::new(&s);
    let (prompt, stream) = match api::validate(raw) {
        Ok(v) => v,
        Err(e) => return error(e.status, e.message, e.param.as_deref()),
    };
    let model = prompt.model.clone();
    let (tx, mut rx) = tmpsc::unbounded_channel();
    if s.jobs.send(Job::Complete(prompt, tx)).is_err() {
        return error(500, STOPPED.into(), None);
    }
    let created = now();
    let id = format!("chatcmpl-{}", uuid_ish());
    if stream {
        // a missing model is told before any chunk: as an error status
        let first = rx.recv().await;
        if let Some(Err(e)) = &first {
            return engine_error(e);
        }
        return Sse::new(stream_of(first, rx, id, created, model, waiting)).into_response();
    }
    let (mut content, mut reasoning) = (String::new(), String::new());
    let finish = loop {
        match rx.recv().await {
            Some(Ok(Step::Piece(Piece::Content(t)))) => content.push_str(&t),
            Some(Ok(Step::Piece(Piece::Reasoning(t)))) => reasoning.push_str(&t),
            Some(Ok(Step::Done(f))) => break f,
            Some(Err(e)) => return engine_error(&e),
            None => return error(500, STOPPED.into(), None),
        }
    };
    drop(waiting);
    Json(api::completion(
        &id, created, &model, &content, &reasoning, &finish,
    ))
    .into_response()
}

/// The answer's steps as server-sent events, ending `[DONE]`. When the
/// client goes, the events stop and the engine's next piece finds nobody.
fn stream_of(
    first: Option<Result<Step, EngineError>>,
    mut rx: tmpsc::UnboundedReceiver<Result<Step, EngineError>>,
    id: String,
    created: u64,
    model: String,
    waiting: Waiting,
) -> impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>> {
    let (etx, erx) = tmpsc::unbounded_channel();
    tokio::spawn(async move {
        let _waiting = waiting;
        let send = |v: String| etx.send(Ok(Event::default().data(v))).is_ok();
        let mut next = first;
        loop {
            let sent = match next {
                Some(Ok(Step::Piece(p))) => {
                    send(api::chunk(&id, created, &model, Some(&p), None).to_string())
                }
                Some(Ok(Step::Done(f))) => {
                    send(api::chunk(&id, created, &model, None, Some(&f)).to_string());
                    send("[DONE]".into());
                    break;
                }
                Some(Err(e)) => {
                    send(
                        api::error_body(&ApiError {
                            status: status(&e),
                            message: e.to_string(),
                            param: None,
                        })
                        .to_string(),
                    );
                    break;
                }
                None => break,
            };
            if !sent {
                break;
            }
            next = rx.recv().await;
        }
    });
    tokio_stream::wrappers::UnboundedReceiverStream::new(erx)
}

/// An id for an answer: the time to the nanosecond, as hex.
fn uuid_ish() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

async fn models(State(s): State<Shared>) -> Response {
    s.touch();
    match itero_ai_index::Index::read(&s.index) {
        Ok(ix) => {
            let data: Vec<Value> = ix
                .models
                .iter()
                .filter(|e| e.removed.is_none())
                .map(|e| {
                    let mut v = serde_json::to_value(e).unwrap_or_default();
                    v["id"] = json!(e.name);
                    v["object"] = json!("model");
                    v["owned_by"] = json!(e.maker);
                    v
                })
                .collect();
            Json(json!({ "object": "list", "data": data })).into_response()
        }
        Err(e) => error(500, format!("{e:#}"), None),
    }
}

/// How it is: what is loaded, what waits, how long since the last
/// request. Asking does not count as a request.
async fn health(State(s): State<Shared>) -> Json<Value> {
    let loaded = s.loaded.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Json(json!({
        "loaded": loaded,
        "waiting": s.waiting.load(Ordering::SeqCst),
        "idle": now().saturating_sub(s.last.load(Ordering::SeqCst)),
    }))
}

async fn inspect(State(s): State<Shared>, Json(v): Json<Value>) -> Response {
    let _waiting = Waiting::new(&s);
    let Some(path) = v.get("path").and_then(Value::as_str) else {
        return error(
            400,
            "the body is {\"path\": \"<file.gguf>\"}".into(),
            Some("path"),
        );
    };
    let (tx, mut rx) = tmpsc::unbounded_channel();
    if s.jobs.send(Job::Inspect(path.into(), tx)).is_err() {
        return error(500, STOPPED.into(), None);
    }
    match rx.recv().await {
        Some(Ok(v)) => Json(v).into_response(),
        Some(Err(e)) => error(status(&e), e.to_string(), Some("path")),
        None => error(500, STOPPED.into(), None),
    }
}

/// Serves until idle. A socket something answers on is refused; a file
/// nobody answers on is removed first.
pub async fn serve(cfg: Config, engine: Box<dyn Engine>) -> anyhow::Result<()> {
    if cfg.socket.exists() {
        if std::os::unix::net::UnixStream::connect(&cfg.socket).is_ok() {
            anyhow::bail!(
                "{}: an itero-ai answers there already",
                cfg.socket.display()
            );
        }
        std::fs::remove_file(&cfg.socket)
            .map_err(|e| anyhow::anyhow!("{}: {e}", cfg.socket.display()))?;
    }
    let listener = tokio::net::UnixListener::bind(&cfg.socket)
        .map_err(|e| anyhow::anyhow!("{}: {e}", cfg.socket.display()))?;
    let (jobs, rx) = mpsc::channel();
    let shared = Shared {
        index: cfg.index.clone(),
        jobs,
        waiting: Arc::new(AtomicUsize::new(0)),
        last: Arc::new(AtomicU64::new(now())),
        loaded: Arc::new(Mutex::new(None)),
    };
    let s = shared.clone();
    std::thread::spawn(move || engine_thread(engine, rx, s));
    let app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .route("/v1/models", get(models))
        .route("/health", get(health))
        .route("/itero/inspect", post(inspect))
        .with_state(shared.clone());
    tell(json!({ "event": "started" }));
    let idle = cfg.idle.as_secs();
    let quiet = async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let since = now().saturating_sub(shared.last.load(Ordering::SeqCst));
            if shared.waiting.load(Ordering::SeqCst) == 0 && since >= idle {
                break;
            }
        }
    };
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(quiet)
        .await;
    let _ = std::fs::remove_file(&cfg.socket);
    served.map_err(|e| anyhow::anyhow!("serving: {e}"))
}
