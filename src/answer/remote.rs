//! The CLI's questions sent to a context's server: each to the route that
//! computes its answer there, read back into the same typed answer, so a
//! command says the same here and there.
use super::Fault;
use anyhow::anyhow;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::time::Duration;

/// How long a question waits for its answer, unless it waits for runs.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// A workspace on a context's server.
#[derive(Debug, Clone)]
pub struct Remote {
    /// The context's name, for messages.
    pub name: String,
    /// The server's root, ending in `/`.
    pub url: String,
    pub token: Option<String>,
    /// The workspace's name on the server's list.
    pub ws: String,
}

impl Remote {
    /// `GET` the workspace's route at `path` (its segments, each encoded).
    pub fn get<T: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, String)],
    ) -> Result<T, Fault> {
        self.send(Method::GET, path, query, None, TIMEOUT)
    }

    pub fn post<T: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &[(&str, String)],
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<T, Fault> {
        self.send(Method::POST, path, query, body, timeout)
    }

    pub fn delete<T: DeserializeOwned>(&self, path: &[&str]) -> Result<T, Fault> {
        self.send(Method::DELETE, path, &[], None, TIMEOUT)
    }

    fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, String)],
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<T, Fault> {
        let mut at = vec!["ws", self.ws.as_str()];
        at.extend(path);
        ask(
            &self.name,
            &self.url,
            self.token.as_deref(),
            method,
            &at,
            query,
            body,
            timeout,
        )
    }
}

/// `GET` the server's own route at `path` (not a workspace's): its
/// `/workspaces`, its `/health`.
pub fn server<T: DeserializeOwned>(
    name: &str,
    url: &str,
    token: Option<&str>,
    path: &[&str],
) -> Result<T, Fault> {
    ask(name, url, token, Method::GET, path, &[], None, TIMEOUT)
}

#[allow(clippy::too_many_arguments)]
fn ask<T: DeserializeOwned>(
    name: &str,
    url: &str,
    token: Option<&str>,
    method: Method,
    path: &[&str],
    query: &[(&str, String)],
    body: Option<&Value>,
    timeout: Duration,
) -> Result<T, Fault> {
    let mut to = reqwest::Url::parse(url)
        .map_err(|e| Fault::Internal(anyhow!("{name}: its URL {url}: {e}")))?;
    to.path_segments_mut()
        .map_err(|_| Fault::Internal(anyhow!("{name}: its URL {url} cannot have a path")))?
        .pop_if_empty()
        .extend(path);
    if !query.is_empty() {
        to.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
    }
    let http = crate::net::client(timeout).map_err(|e| Fault::Internal(e.into()))?;
    let mut req = http.request(method, to).header(
        crate::engine::http::VERSION_HEADER,
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(b) = body {
        req = req.json(b);
    }
    let r = req.send().map_err(|e| {
        Fault::Internal(anyhow!(
            "cannot reach {name} ({url}): {}",
            crate::net::cause(&e)
        ))
    })?;
    let status = r.status().as_u16();
    if let Some(theirs) = r.headers().get(crate::engine::http::VERSION_HEADER) {
        differs(name, &String::from_utf8_lossy(theirs.as_bytes()));
    }
    let text = r
        .text()
        .map_err(|e| Fault::Internal(anyhow!("{name}: {}", crate::net::cause(&e))))?;
    let answer: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(refusal(name, status, answer));
    }
    serde_json::from_value(answer)
        .map_err(|e| Fault::Internal(anyhow!("{name}: an answer this itero cannot read: {e}")))
}

/// Says on stderr, once a command, that the server runs another itero.
fn differs(name: &str, theirs: &str) {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let me = env!("CARGO_PKG_VERSION");
    if theirs != me && !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("{name} runs itero {theirs}, this itero is {me}");
    }
}

/// A refusal from the server as the same `Fault` the route made of it.
fn refusal(name: &str, status: u16, answer: Value) -> Fault {
    let mut fields = match answer {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let message = match fields.remove("error") {
        Some(Value::String(m)) => m,
        _ => format!("{name} answered {status}"),
    };
    match status {
        400 => Fault::Bad(message),
        404 => Fault::Missing(message),
        409 => Fault::Conflict(message, fields),
        401 | 403 => Fault::Internal(anyhow!(
            "{name} refused its token ({message}): itero context add {name} again with a new one"
        )),
        _ => Fault::Internal(anyhow!("{name}: {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_refusal_is_the_fault_the_route_made() {
        let f = refusal("üretim", 404, json!({ "error": "no run yok" }));
        assert!(matches!(&f, Fault::Missing(m) if m == "no run yok"));
        let f = refusal(
            "üretim",
            409,
            json!({ "error": "it started koşu", "started": ["koşu"] }),
        );
        match f {
            Fault::Conflict(m, why) => {
                assert_eq!(m, "it started koşu");
                assert_eq!(Value::Object(why), json!({ "started": ["koşu"] }));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            refusal("üretim", 400, json!({ "error": "x" })),
            Fault::Bad(_)
        ));
        let f = refusal("üretim", 401, json!({ "error": "no token" }));
        assert!(f.to_string().contains("üretim refused its token"), "{f}");
        let f = refusal("üretim", 502, Value::Null);
        assert_eq!(f.to_string(), "üretim: üretim answered 502");
    }
}
