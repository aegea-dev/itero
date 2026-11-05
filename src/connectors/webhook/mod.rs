//! `itero webhook`: takes requests from outside, each checked against a
//! secret, and saves them as `itero.webhook.received`. A server of its own,
//! apart from the daemon's API: it has only the paths it is given.
pub mod check;
mod serve;

use axum::http::{HeaderMap, header};
use check::Check;
use serde_json::{Map, Value, json};

/// The fewest bytes a secret has.
const SECRET_MIN: usize = 16;

/// One path, how a request to it is checked, and the secret.
pub struct Hook {
    pub path: String,
    pub check: Check,
    pub secret: String,
}

/// The `--hook`s, `<path>:<check>:<NAME>`, `secret` looking a name up.
/// Every problem is told, by its `--hook` and its name, never by a value.
pub fn hooks(
    given: &[String],
    secret: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<Hook>, Vec<String>> {
    if given.is_empty() {
        return Err(vec![
            "no --hook: give at least one, as /path:check:SECRET_NAME".to_string(),
        ]);
    }
    let mut out: Vec<Hook> = vec![];
    let mut problems = vec![];
    for g in given {
        let parts: Vec<&str> = g.split(':').collect();
        let [path, check, name] = parts[..] else {
            problems.push(format!("--hook {g}: it is /path:check:SECRET_NAME"));
            continue;
        };
        let before = problems.len();
        if !path.starts_with('/') || path.contains(['?', '#']) {
            problems.push(format!(
                "--hook {g}: a path starts with / and has no ? or #"
            ));
        }
        if out.iter().any(|h| h.path == path) {
            problems.push(format!("--hook {g}: {path} is given twice"));
        }
        let named = Check::named(check);
        if named.is_none() {
            problems.push(format!(
                "--hook {g}: {check} is no check: token, hmac or twilio"
            ));
        }
        let value = secret_of(g, name, secret, &mut problems);
        if let (Some(check), Some(secret), true) = (named, value, problems.len() == before) {
            out.push(Hook {
                path: path.to_string(),
                check,
                secret,
            });
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems)
    }
}

/// The secret a `--hook` names; a problem when it cannot be used.
fn secret_of(
    hook: &str,
    name: &str,
    secret: &dyn Fn(&str) -> Option<String>,
    problems: &mut Vec<String>,
) -> Option<String> {
    if !crate::engine::secrets::valid_name(name) {
        problems.push(format!(
            "--hook {hook}: {name}: a secret's name is a letter or _, then letters, digits or _"
        ));
        return None;
    }
    let value = secret(name);
    match &value {
        None => problems.push(format!(
            "--hook {hook}: {name} is not set: itero ws secret {name}, or a line in .env"
        )),
        Some(v) if v.len() < SECRET_MIN => problems.push(format!(
            "--hook {hook}: {name} is under {SECRET_MIN} bytes, too short to be secret"
        )),
        Some(_) => return value,
    }
    None
}

/// A body that cannot be an event's.
#[derive(Debug)]
pub enum BadBody {
    NotJson,
    NotText,
}

fn media_type(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Whether the body says it is a form.
pub fn is_form(headers: &HeaderMap) -> bool {
    media_type(headers) == "application/x-www-form-urlencoded"
}

/// A form as an object of texts, a repeated field as a list of them.
fn form(body: &[u8]) -> Value {
    let mut out = Map::new();
    for (k, v) in form_urlencoded::parse(body).into_owned() {
        match out.get_mut(&k) {
            None => {
                out.insert(k, Value::String(v));
            }
            Some(Value::Array(a)) => a.push(Value::String(v)),
            Some(first) => *first = json!([first.clone(), v]),
        }
    }
    Value::Object(out)
}

/// The event's data: `{"path", "headers", "body"}`, without what proves the
/// request.
pub fn event_data(
    path: &str,
    check: Check,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Value, BadBody> {
    let body = match media_type(headers).as_str() {
        "application/json" => serde_json::from_slice(body).map_err(|_| BadBody::NotJson)?,
        "application/x-www-form-urlencoded" => form(body),
        _ => Value::String(
            std::str::from_utf8(body)
                .map_err(|_| BadBody::NotText)?
                .to_string(),
        ),
    };
    let hidden = [
        header::AUTHORIZATION.as_str(),
        header::COOKIE.as_str(),
        header::PROXY_AUTHORIZATION.as_str(),
        check.header(),
    ];
    let mut kept = Map::new();
    for name in headers.keys().filter(|n| !hidden.contains(&n.as_str())) {
        let values: Vec<String> = headers
            .get_all(name)
            .iter()
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
            .collect();
        kept.insert(name.as_str().to_string(), Value::String(values.join(", ")));
    }
    Ok(json!({ "path": path, "headers": kept, "body": body }))
}

/// `itero webhook`, ready to take requests.
pub struct Webhook {
    ws: crate::workspace::Workspace,
    hooks: Vec<Hook>,
    listener: std::net::TcpListener,
}

impl Webhook {
    /// Reads the secrets and binds the port: what can go wrong goes wrong
    /// here.
    pub fn new(
        ws: crate::workspace::Workspace,
        host: std::net::IpAddr,
        port: u16,
        given: &[String],
    ) -> anyhow::Result<Webhook> {
        let found = hooks(given, &|n| crate::engine::secrets::get_in(&ws, n))
            .map_err(|problems| anyhow::anyhow!(problems.join("\n")))?;
        let listener = std::net::TcpListener::bind((host, port)).map_err(|e| {
            anyhow::anyhow!(
                "cannot listen on {host}:{port}: {e} (another webhook on this port? a workflow giving this connector args runs a second copy of it)"
            )
        })?;
        if !host.is_loopback() {
            eprintln!(
                "itero webhook: {host}:{port} takes requests from other machines, not encrypted: put a proxy with TLS in front"
            );
        }
        let paths: Vec<&str> = found.iter().map(|h| h.path.as_str()).collect();
        eprintln!(
            "itero webhook: listening on {host}:{port}: {}",
            paths.join(" ")
        );
        Ok(Webhook {
            ws,
            hooks: found,
            listener,
        })
    }

    /// Takes requests until the process ends.
    pub fn run(self) -> anyhow::Result<()> {
        serve::serve(self.ws, self.hooks, self.listener)
    }
}
