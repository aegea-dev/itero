//! `server.json`: how the machine's daemon serves, in itero's config
//! folder (or the file `--config` names), read at the daemon's start.
//! None there is the daemon as it has always been.
use anyhow::{Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The settings `server.json` may have.
const SETTINGS: &[&str] = &["port"];

/// How the machine's daemon serves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Server {
    /// The local API's port on 127.0.0.1; none: the one it kept, else
    /// one the OS gives.
    pub port: Option<u16>,
}

impl Server {
    /// `<config folder>/server.json`.
    pub fn default_path() -> Result<PathBuf> {
        Ok(crate::machine::config_dir()?.join("server.json"))
    }

    /// The file `config` names, else the default one; the default one not
    /// there is no settings, a named one not there is a mistake.
    pub fn read(config: Option<&Path>) -> Result<Server> {
        let (path, given) = match config {
            Some(p) => (p.to_path_buf(), true),
            None => (Server::default_path()?, false),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !given => {
                return Ok(Server::default());
            }
            Err(e) => bail!("{}: {e}", path.display()),
        };
        parse(&text).map_err(|problems| {
            let at = path.display();
            anyhow::anyhow!(
                "{}",
                problems
                    .iter()
                    .map(|p| format!("{at}: {p}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
    }
}

/// The settings in `text`, or every problem in it.
fn parse(text: &str) -> Result<Server, Vec<String>> {
    let v: Value = serde_json::from_str(text).map_err(|e| vec![format!("not JSON: {e}")])?;
    let Value::Object(m) = v else {
        return Err(vec!["server.json is a JSON object of settings".into()]);
    };
    let mut problems = vec![];
    let mut s = Server::default();
    for (k, v) in &m {
        match k.as_str() {
            "port" => match v.as_u64().filter(|p| (1..=65535).contains(p)) {
                Some(p) => s.port = Some(p as u16),
                None => problems.push(format!("port: a number from 1 to 65535, not {v}")),
            },
            _ => problems.push(format!(
                "{k}: not a setting of server.json ({})",
                SETTINGS.join(", ")
            )),
        }
    }
    match problems.is_empty() {
        true => Ok(s),
        false => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_is_read() {
        assert_eq!(parse(r#"{"port": 8787}"#).unwrap().port, Some(8787));
        assert_eq!(parse("{}").unwrap(), Server::default());
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
        let e = Server::read(Some(&d.path().join("yok.json"))).unwrap_err();
        assert!(e.to_string().contains("yok.json"), "{e}");
        let f = d.path().join("sunucu.json");
        std::fs::write(&f, r#"{"port": 9}"#).unwrap();
        assert_eq!(Server::read(Some(&f)).unwrap().port, Some(9));
    }
}
