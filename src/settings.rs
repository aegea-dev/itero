//! `daemon.json`: how the machine's daemon serves, in itero's config
//! folder (or the file `--config` names), read at the daemon's start.
//! None there is the daemon as it has always been.
use anyhow::{Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The settings `daemon.json` may have.
const SETTINGS: &[&str] = &["port", "listen", "tls", "secrets", "notify"];

/// The settings `tls` may have.
const TLS: &[&str] = &["cert", "key"];

/// The settings a channel in `notify` may have.
const CHANNEL: &[&str] = &["url", "format"];

/// How the machine's daemon serves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    /// The local API's port on 127.0.0.1; none: the one it kept, else
    /// one the OS gives.
    pub port: Option<u16>,
    /// Where the API is opened too, beside the local one: there, every
    /// request needs a token.
    pub listen: Option<std::net::SocketAddr>,
    /// The certificate `listen` serves HTTPS with; none: plain HTTP.
    pub tls: Option<Tls>,
    /// A folder of secrets the platform mounts, a file each
    /// (`/run/secrets/API_KEY`): `$$API_KEY` reads it.
    pub secrets: Option<PathBuf>,
    /// Where `#notify` sends instead of the desktop: none, the desktop.
    pub notify: Vec<Channel>,
}

/// A place `#notify` POSTs to: an http(s) address, or `$$NAME`, a secret
/// of the machine's holding one; and the body it takes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub url: String,
    pub format: Format,
}

/// The body a channel takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// `{"title", "subtitle", "text", "url", "workspace", "workflow"}`
    #[default]
    Json,
    /// ntfy's JSON publishing
    Ntfy,
    /// a Slack incoming webhook's (Mattermost and Google Chat read its text)
    Slack,
    /// a Discord webhook's
    Discord,
}

/// The formats, as `format` names them.
const FORMATS: &[(&str, Format)] = &[
    ("json", Format::Json),
    ("ntfy", Format::Ntfy),
    ("slack", Format::Slack),
    ("discord", Format::Discord),
];

static CHANNELS: std::sync::OnceLock<Vec<Channel>> = std::sync::OnceLock::new();

/// Keeps the machine daemon's channels, once, at its start.
pub fn set_channels(c: Vec<Channel>) {
    let _ = CHANNELS.set(c);
}

/// The channels `#notify` sends to here: none outside `itero daemon`.
pub fn channels() -> &'static [Channel] {
    CHANNELS.get().map_or(&[], Vec::as_slice)
}

/// A certificate (its chain) and its private key, PEM files.
#[derive(Debug, Clone, PartialEq)]
pub struct Tls {
    pub cert: PathBuf,
    pub key: PathBuf,
}

impl Settings {
    /// `<config folder>/daemon.json`.
    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::machine::config_dir()?.join("daemon.json"))
    }

    /// The file `config` names, else the default one; the default one not
    /// there is no settings, a named one not there is a mistake. Every
    /// problem is told at once, a `server.json` beside it among them.
    pub fn read(config: Option<&Path>) -> Result<Settings> {
        let (path, given) = match config {
            Some(p) => (p.to_path_buf(), true),
            None => (Settings::default_path()?, false),
        };
        let mut problems = left_from_before(&path).into_iter().collect::<Vec<_>>();
        let settings = match std::fs::read_to_string(&path) {
            Ok(text) => {
                // the secrets the channels it could read name, beside the
                // file's own problems: every problem in one run
                let (s, mut found) = parse_all(&text);
                found.extend(missing_secrets(&s));
                let at = path.display();
                problems.extend(found.iter().map(|p| format!("{at}: {p}")));
                Some(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !given => {
                Some(Settings::default())
            }
            Err(e) => {
                problems.push(format!("{}: {e}", path.display()));
                None
            }
        };
        match settings {
            Some(s) if problems.is_empty() => Ok(s),
            _ => bail!("{}", problems.join("\n")),
        }
    }
}

/// A `server.json` beside `path`: what itero read before 0.41. Left
/// alone, its settings would be ignored without a word (a daemon meant
/// to listen on 0.0.0.0 staying local), so it stops the daemon.
fn left_from_before(path: &Path) -> Option<String> {
    let old = path.with_file_name("server.json");
    (old != path && old.exists()).then(|| format!("{}: rename it to daemon.json", old.display()))
}

/// The settings in `text`, or every problem in it.
#[cfg(test)]
fn parse(text: &str) -> Result<Settings, Vec<String>> {
    match parse_all(text) {
        (s, p) if p.is_empty() => Ok(s),
        (_, p) => Err(p),
    }
}

/// The settings in `text` that could be read, and every problem in it.
fn parse_all(text: &str) -> (Settings, Vec<String>) {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return (Settings::default(), vec![format!("not JSON: {e}")]),
    };
    let Value::Object(m) = v else {
        return (
            Settings::default(),
            vec!["daemon.json is a JSON object of settings".into()],
        );
    };
    let mut problems = vec![];
    let mut s = Settings::default();
    for (k, v) in &m {
        match k.as_str() {
            "port" => match v.as_u64().filter(|p| (1..=65535).contains(p)) {
                Some(p) => s.port = Some(p as u16),
                None => problems.push(format!("port: a number from 1 to 65535, not {v}")),
            },
            "listen" => match v.as_str().and_then(|a| a.parse().ok()) {
                Some(a) => s.listen = Some(a),
                None => problems.push(format!(
                    "listen: an address and a port, like 0.0.0.0:8443, not {v}"
                )),
            },
            "secrets" => match v.as_str() {
                Some(p) if Path::new(p).is_dir() => s.secrets = Some(PathBuf::from(p)),
                Some(p) => problems.push(format!(
                    "secrets: {p}: not a folder there (the folder of secrets, a file each)"
                )),
                None => problems.push(format!("secrets: a folder's path, not {v}")),
            },
            "notify" => {
                let (c, p) = channels_in(v);
                s.notify = c;
                problems.extend(p);
            }
            "tls" => match tls(v) {
                Ok(t) => s.tls = Some(t),
                Err(p) => problems.extend(p),
            },
            _ => problems.push(format!(
                "{k}: not a setting of daemon.json ({})",
                SETTINGS.join(", ")
            )),
        }
    }
    if m.contains_key("tls") && !m.contains_key("listen") {
        problems.push("tls: for listen, which is not given".into());
    }
    (s, problems)
}

/// The secret a channel's url names (`$$NAME`), if it names one.
pub fn secret_of(url: &str) -> Option<&str> {
    url.strip_prefix("$$")
}

/// `notify`'s channels that could be read, and every problem in them. A
/// url is never echoed: one written in the file may be a secret.
fn channels_in(v: &Value) -> (Vec<Channel>, Vec<String>) {
    let Value::Array(list) = v else {
        return (
            vec![],
            vec![format!(
                "notify: a list of channels, each with url and format, not {}",
                kind(v)
            )],
        );
    };
    if list.is_empty() {
        return (
            vec![],
            vec!["notify: an empty list: leave notify out for the desktop".into()],
        );
    }
    let mut problems = vec![];
    let mut out = vec![];
    for (i, c) in list.iter().enumerate() {
        let Value::Object(m) = c else {
            problems.push(format!(
                "notify[{i}]: a channel is an object with url and format"
            ));
            continue;
        };
        for k in m.keys().filter(|k| !CHANNEL.contains(&k.as_str())) {
            problems.push(format!(
                "notify[{i}].{k}: not a setting of a channel ({})",
                CHANNEL.join(", ")
            ));
        }
        let url = match m.get("url") {
            None => {
                problems.push(format!("notify[{i}].url: not given"));
                None
            }
            Some(Value::String(u))
                if u.starts_with("http://")
                    || u.starts_with("https://")
                    || secret_of(u).is_some_and(crate::engine::secrets::valid_name) =>
            {
                Some(u.clone())
            }
            Some(_) => {
                problems.push(format!(
                    "notify[{i}].url: an http:// or https:// address, or $$NAME"
                ));
                None
            }
        };
        let format = match m.get("format") {
            None => Some(Format::default()),
            Some(f) => match FORMATS.iter().find(|(n, _)| Some(*n) == f.as_str()) {
                Some((_, f)) => Some(*f),
                None => {
                    let names: Vec<&str> = FORMATS.iter().map(|(n, _)| *n).collect();
                    problems.push(format!(
                        "notify[{i}].format: one of {}, not {f}",
                        names.join(", ")
                    ));
                    None
                }
            },
        };
        if let (Some(url), Some(format)) = (url, format) {
            out.push(Channel { url, format });
        }
    }
    (out, problems)
}

/// What kind of JSON `v` is, to say it without its value.
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// The channels whose `$$NAME` names no secret of the machine's.
fn missing_secrets(s: &Settings) -> Vec<String> {
    s.notify
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let name = secret_of(&c.url)?;
            crate::engine::secrets::machine(s.secrets.as_deref(), name)
                .is_none()
                .then(|| {
                    format!(
                        "notify[{i}].url: no secret {name} (the secrets folder, the store, or the environment)"
                    )
                })
        })
        .collect()
}

/// `tls`'s two files, each there and readable; or every problem.
fn tls(v: &Value) -> Result<Tls, Vec<String>> {
    let Value::Object(m) = v else {
        return Err(vec![format!("tls: an object with cert and key, not {v}")]);
    };
    let mut problems = vec![];
    for k in m.keys().filter(|k| !TLS.contains(&k.as_str())) {
        problems.push(format!(
            "tls.{k}: not a setting of tls ({})",
            TLS.join(", ")
        ));
    }
    let mut file = |k: &str, what: &str| -> Option<PathBuf> {
        match m.get(k).map(|v| v.as_str()) {
            None => {
                problems.push(format!("tls.{k}: {what} PEM file, not given"));
                None
            }
            Some(None) => {
                problems.push(format!("tls.{k}: {what} PEM file's path, not {}", m[k]));
                None
            }
            Some(Some(p)) => match std::fs::metadata(p) {
                Ok(m) if !m.is_file() => {
                    problems.push(format!("tls.{k}: {p} is not a file"));
                    None
                }
                Ok(_) => match std::fs::File::open(p) {
                    Ok(_) => Some(PathBuf::from(p)),
                    Err(e) => {
                        problems.push(format!("tls.{k}: {p}: {e}"));
                        None
                    }
                },
                Err(e) => {
                    problems.push(format!("tls.{k}: {p}: {e}"));
                    None
                }
            },
        }
    };
    let cert = file("cert", "the certificate's");
    let key = file("key", "the private key's");
    match (cert, key, problems.is_empty()) {
        (Some(cert), Some(key), true) => Ok(Tls { cert, key }),
        _ => Err(problems),
    }
}

/// What is worth saying about `t` and not wrong: a private key others on
/// this machine can read. Not refused: a Kubernetes secret is mounted
/// 0644 by default, readable by the pod's own user alone.
pub fn tls_warnings(t: &Tls) -> Vec<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(&t.key)
            && m.permissions().mode() & 0o077 != 0
        {
            return vec![format!(
                "{}: readable by others (mode {:o}): chmod 600 it, unless only this server's user can reach it",
                t.key.display(),
                m.permissions().mode() & 0o777
            )];
        }
    }
    vec![]
}

/// The TLS settings for `t`: its certificate chain and its key, read
/// now, the key checked against the certificate; through ring, and
/// HTTP/1.1, which the API speaks.
pub fn tls_config(t: &Tls) -> Result<std::sync::Arc<rustls::ServerConfig>> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let (cert, key) = (t.cert.display(), t.key.display());
    let chain = CertificateDer::pem_file_iter(&t.cert)
        .and_then(|i| i.collect::<Result<Vec<_>, _>>())
        .map_err(|e| anyhow::anyhow!("{cert}: not a PEM certificate: {e}"))?;
    if chain.is_empty() {
        bail!("{cert}: no certificate in it");
    }
    let private = PrivateKeyDer::from_pem_file(&t.key)
        .map_err(|e| anyhow::anyhow!("{key}: not a PEM private key: {e}"))?;
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(chain, private)
        .map_err(|e| anyhow::anyhow!("{key} is not the key of {cert}: {e}"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(std::sync::Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_is_read() {
        assert_eq!(parse(r#"{"port": 8787}"#).unwrap().port, Some(8787));
        assert_eq!(parse("{}").unwrap(), Settings::default());
    }

    #[test]
    fn every_problem_is_told() {
        let p = parse(r#"{"port": -1, "dinle": "0.0.0.0:1", "çok": 2}"#).unwrap_err();
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(
            p[0].starts_with("port: a number from 1 to 65535, not -1"),
            "{p:?}"
        );
        assert!(p[1].starts_with("dinle: not a setting"), "{p:?}");
        assert_eq!(parse("[]").unwrap_err().len(), 1);
        assert!(parse("{").unwrap_err()[0].starts_with("not JSON"));
    }

    #[test]
    fn notify_is_a_list_of_channels_every_problem_told() {
        let s = parse(
            r#"{ "notify": [
                { "url": "https://ntfy.sh/uyarılar", "format": "ntfy" },
                { "url": "$$KANAL" } ] }"#,
        )
        .unwrap();
        assert_eq!(
            s.notify,
            vec![
                Channel {
                    url: "https://ntfy.sh/uyarılar".into(),
                    format: Format::Ntfy
                },
                Channel {
                    url: "$$KANAL".into(),
                    format: Format::Json
                },
            ]
        );
        let e = parse(
            r#"{ "notify": [
                { "url": "ftp://örnek.test/gizli", "format": "teams", "renk": 1 },
                { "format": "slack" }, "düz", { "url": "$$1yanlış" } ] }"#,
        )
        .unwrap_err();
        for p in [
            // the value is never echoed: a URL written in the file may be
            // a secret
            "notify[0].url: an http:// or https:// address, or $$NAME",
            "notify[0].format: one of json, ntfy, slack, discord, not \"teams\"",
            "notify[0].renk: not a setting of a channel (url, format)",
            "notify[1].url: not given",
            "notify[2]: a channel is an object with url and format",
            "notify[3].url: an http:// or https:// address, or $$NAME",
        ] {
            assert!(e.iter().any(|x| x == p), "{p} in {e:#?}");
        }
        assert_eq!(e.len(), 6, "{e:#?}");
        assert!(!e.iter().any(|x| x.contains("gizli")), "{e:#?}");
        assert_eq!(
            parse(r#"{ "notify": [] }"#).unwrap_err(),
            vec!["notify: an empty list: leave notify out for the desktop".to_string()]
        );
        assert!(
            parse(r#"{ "notify": "x" }"#).unwrap_err()[0].starts_with("notify: a list of channels"),
        );
    }

    #[test]
    fn a_named_file_must_be_there_and_the_default_need_not() {
        let d = tempfile::tempdir().unwrap();
        let e = Settings::read(Some(&d.path().join("yok.json"))).unwrap_err();
        assert!(e.to_string().contains("yok.json"), "{e}");
        let f = d.path().join("sunucu.json");
        std::fs::write(&f, r#"{"port": 9}"#).unwrap();
        assert_eq!(Settings::read(Some(&f)).unwrap().port, Some(9));
    }
}
