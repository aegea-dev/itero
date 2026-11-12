//! The listener: one request a connection, and what it answers.
use super::check::{self, Check};
use super::{BadBody, Hook};
use crate::workspace::Workspace;
use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// The most a body is, in bytes.
const BODY_MAX: usize = 5 * 1024 * 1024;
/// The most requests at once on one hook.
const PLACES: usize = 32;
/// How long a connection lives: all of one request and its answer.
const LIFE: Duration = Duration::from_secs(30);
/// How often one refusal is told.
const TELL_EVERY: Duration = Duration::from_secs(60);

struct Place {
    hook: Hook,
    places: Arc<Semaphore>,
}

/// A refusal's path and why.
type Refusal = (String, &'static str);

/// Refusals seen: when each was last told, and how many since. Told at most
/// once in `TELL_EVERY`. The keys are this server's own words, so a stranger
/// cannot grow them.
#[derive(Default)]
struct Told(Mutex<HashMap<Refusal, (Option<Instant>, u64)>>);

impl Told {
    fn tell(&self, path: &str, why: &'static str) {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (last, count) = seen.entry((path.to_string(), why)).or_default();
        *count += 1;
        if last.is_none_or(|t| t.elapsed() >= TELL_EVERY) {
            eprintln!("itero webhook: {path}: {why} ({count} since last told)");
            *last = Some(Instant::now());
            *count = 0;
        }
    }
}

struct Server {
    ws: Arc<Workspace>,
    places: Vec<Place>,
    told: Told,
}

impl Server {
    fn refuse(&self, path: &str, code: StatusCode, why: &'static str) -> Response {
        self.told.tell(path, why);
        code.into_response()
    }
}

/// Meta's handshake: its `hub.challenge`, digits only, as text.
fn handshake(s: &Server, path: &str, query: Option<&str>) -> Response {
    let fields: Vec<(String, String)> = form_urlencoded::parse(query.unwrap_or("").as_bytes())
        .into_owned()
        .collect();
    let field = |n: &str| fields.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    if field("hub.mode") != Some("subscribe") {
        return s.refuse(
            path,
            StatusCode::METHOD_NOT_ALLOWED,
            "a GET that is no handshake",
        );
    }
    match field("hub.challenge") {
        Some(c) if !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_digit()) => {
            ([(header::CONTENT_TYPE, "text/plain")], c.to_string()).into_response()
        }
        _ => s.refuse(path, StatusCode::BAD_REQUEST, "a handshake without digits"),
    }
}

async fn receive(State(s): State<Arc<Server>>, req: Request) -> Response {
    let asked = req.uri().path().to_string();
    let Some(place) = s.places.iter().find(|p| p.hook.path == asked) else {
        // never the path asked for in the log: it is a stranger's text
        return s.refuse("another path", StatusCode::NOT_FOUND, "not a listed path");
    };
    let hook = &place.hook;
    if req.method() == Method::GET && hook.check == Check::Hmac {
        return handshake(&s, &hook.path, req.uri().query());
    }
    if req.method() != Method::POST {
        return s.refuse(&hook.path, StatusCode::METHOD_NOT_ALLOWED, "not a POST");
    }
    let Ok(_place) = place.places.clone().try_acquire_owned() else {
        return s.refuse(
            &hook.path,
            StatusCode::SERVICE_UNAVAILABLE,
            "too many at once",
        );
    };
    let (parts, body) = req.into_parts();
    if hook.check == Check::Twilio && !super::is_form(&parts.headers) {
        return s.refuse(&hook.path, StatusCode::BAD_REQUEST, "twilio sends a form");
    }
    let Ok(body) = to_bytes(body, BODY_MAX).await else {
        return s.refuse(&hook.path, StatusCode::PAYLOAD_TOO_LARGE, "a body too big");
    };
    // as UTF-8: to_str takes only ASCII, and a token may be any text
    let text = |name: &str| {
        let value = parts.headers.get(name)?;
        std::str::from_utf8(value.as_bytes()).ok()
    };
    let given = text(hook.check.header());
    let host = text(header::HOST.as_str()).unwrap_or("");
    let query = parts.uri.path_and_query().map_or("", |q| q.as_str());
    let url = format!("https://{host}{query}");
    if !check::passes(hook.check, &hook.secret, given, &url, &body) {
        return s.refuse(&hook.path, StatusCode::UNAUTHORIZED, "the check failed");
    }
    let data = match super::event_data(&hook.path, hook.check, &parts.headers, &body) {
        Ok(d) => d,
        Err(BadBody::NotJson) => {
            return s.refuse(
                &hook.path,
                StatusCode::BAD_REQUEST,
                "a body that is not JSON",
            );
        }
        Err(BadBody::NotText) => {
            return s.refuse(
                &hook.path,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "a body that is not text",
            );
        }
    };
    // the journal may wait on a busy SQLite: off the async threads
    let ws = s.ws.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let now = crate::engine::daemon::stamp(chrono::Utc::now());
        crate::engine::events::webhook_received(&ws, data, &now)?;
        if let Err(e) = crate::engine::events::nudge(&ws) {
            // saved: the daemon takes it at its next wake or start
            eprintln!("itero webhook: saved, but could not wake the daemon: {e}");
        }
        anyhow::Ok(())
    })
    .await;
    match saved {
        Ok(Ok(())) => StatusCode::OK.into_response(),
        Ok(Err(e)) => {
            eprintln!("itero webhook: {}: not saved: {e:#}", hook.path);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            eprintln!("itero webhook: {}: not saved: {e}", hook.path);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Serves `hooks` on `listener` until the process ends.
pub fn serve(
    ws: Workspace,
    hooks: Vec<Hook>,
    listener: std::net::TcpListener,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let life = std::env::var("ITERO_WEBHOOK_LIFE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(LIFE, Duration::from_secs);
    let server = Server {
        ws: Arc::new(ws),
        places: hooks
            .into_iter()
            .map(|hook| Place {
                hook,
                places: Arc::new(Semaphore::new(PLACES)),
            })
            .collect(),
        told: Told::default(),
    };
    let app = Router::new().fallback(receive).with_state(Arc::new(server));
    tokio::runtime::Runtime::new()?.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                // out of file descriptors, say: those open end within `life`
                Err(e) => {
                    eprintln!("itero webhook: cannot accept: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let service = TowerToHyperService::new(app.clone());
            tokio::spawn(async move {
                let conn = crate::net::http1()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(stream), service);
                let _ = tokio::time::timeout(life, conn).await;
            });
        }
    })
}
