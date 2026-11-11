//! `daemon.json`: how the machine's daemon serves, in itero's config
//! folder (or the file `--config` names), read at the daemon's start.
//! None there is the daemon as it has always been.
use anyhow::{Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The settings `daemon.json` may have.
const SETTINGS: &[&str] = &["port", "listen", "tls", "secrets"];

/// The settings `tls` may have.
const TLS: &[&str] = &["cert", "key"];

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
            Ok(text) => match parse(&text) {
                Ok(s) => Some(s),
                Err(p) => {
                    let at = path.display();
                    problems.extend(p.iter().map(|p| format!("{at}: {p}")));
                    None
                }
            },
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
fn parse(text: &str) -> Result<Settings, Vec<String>> {
    let v: Value = serde_json::from_str(text).map_err(|e| vec![format!("not JSON: {e}")])?;
    let Value::Object(m) = v else {
        return Err(vec!["daemon.json is a JSON object of settings".into()]);
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
    match problems.is_empty() {
        true => Ok(s),
        false => Err(problems),
    }
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
    fn a_named_file_must_be_there_and_the_default_need_not() {
        let d = tempfile::tempdir().unwrap();
        let e = Settings::read(Some(&d.path().join("yok.json"))).unwrap_err();
        assert!(e.to_string().contains("yok.json"), "{e}");
        let f = d.path().join("sunucu.json");
        std::fs::write(&f, r#"{"port": 9}"#).unwrap();
        assert_eq!(Settings::read(Some(&f)).unwrap().port, Some(9));
    }
}
