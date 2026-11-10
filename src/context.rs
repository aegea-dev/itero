//! Contexts: the itero deployments this CLI talks to, each its server's
//! URL, a token and a default workspace, as kubeconfig's. Kept in the
//! config folder's `contexts.json`; `local` is this machine, built in.
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The built-in context: this machine, answered in-process.
pub const LOCAL: &str = "local";

/// One deployment: where its server is, how to sign in, which workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub name: String,
    /// The server's root, ending in `/`.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The workspace a command means when `--ws` names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ws: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contexts {
    /// The context in use; none is `local`.
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub contexts: Vec<Context>,
}

impl Contexts {
    /// `<config folder>/contexts.json`.
    pub fn path() -> Result<PathBuf> {
        Ok(crate::machine::config_dir()?.join("contexts.json"))
    }

    /// The contexts at `p`; none there is none added.
    pub fn read(p: &Path) -> Result<Contexts> {
        match std::fs::read_to_string(p) {
            Ok(t) => serde_json::from_str(&t).with_context(|| p.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Contexts::default()),
            Err(e) => Err(e).with_context(|| p.display().to_string()),
        }
    }

    /// Writes it whole, readable by its owner only (it holds tokens).
    pub fn write(&self, p: &Path) -> Result<()> {
        crate::machine::write_private(p, &(serde_json::to_string_pretty(self)? + "\n"))
    }

    pub fn get(&self, name: &str) -> Option<&Context> {
        self.contexts.iter().find(|c| c.name == name)
    }

    /// Adds `c`, or replaces the one of its name: whether it replaced one.
    pub fn upsert(&mut self, c: Context) -> bool {
        match self.contexts.iter_mut().find(|x| x.name == c.name) {
            Some(x) => {
                *x = c;
                true
            }
            None => {
                self.contexts.push(c);
                false
            }
        }
    }

    /// Takes the one named `name` out, and the current one with it.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.contexts.len();
        self.contexts.retain(|c| c.name != name);
        if self.current.as_deref() == Some(name) {
            self.current = None;
        }
        self.contexts.len() != before
    }
}

/// The context a command uses: `flag` (`--context`), else
/// `ITERO_CONTEXT`, else the one in use, else this machine (none).
pub fn chosen(flag: Option<&str>) -> Result<Option<Context>> {
    let env = std::env::var("ITERO_CONTEXT")
        .ok()
        .filter(|e| !e.is_empty());
    let all = Contexts::read(&Contexts::path()?)?;
    let name = flag.map(String::from).or(env).or(all.current.clone());
    match name.as_deref() {
        None | Some(LOCAL) => Ok(None),
        Some(n) => match all.get(n) {
            Some(c) => Ok(Some(c.clone())),
            None => bail!("no context {n}: itero context list lists them"),
        },
    }
}

/// Why `name` cannot be a context's name, if it cannot.
pub fn bad_name(name: &str) -> Option<String> {
    if name == LOCAL {
        return Some(format!("{LOCAL} is this machine: it is built in"));
    }
    (!crate::names::plain(name)).then(|| format!("{name}: {}", crate::names::PLAIN))
}

/// `url` as a server's root, ending in `/`; or why it is not one.
pub fn server_url(url: &str) -> Result<String, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("{url}: a URL starts with http:// or https://"));
    }
    match url.ends_with('/') {
        true => Ok(url.to_string()),
        false => Ok(format!("{url}/")),
    }
}

/// Asks the server at `url` (its root) for its `/health`, with `token`:
/// the itero it runs, or why it cannot be used, told as context `name`.
pub fn check(name: &str, url: &str, token: Option<&str>) -> Result<String> {
    let http = crate::net::client(std::time::Duration::from_secs(10))?;
    let mut req = http.get(format!("{url}health")).header(
        crate::engine::http::VERSION_HEADER,
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let r = match req.send() {
        Ok(r) => r,
        Err(e) => bail!("cannot reach {name} ({url}): {}", crate::net::cause(&e)),
    };
    match r.status().as_u16() {
        200 => {}
        401 | 403 => {
            bail!("{name} refused the token: give it one from itero token create on its server")
        }
        s => {
            // an itero that refuses says why (this itero too old for it)
            let said: Option<String> = r
                .json::<serde_json::Value>()
                .ok()
                .and_then(|v| v["error"].as_str().map(String::from));
            match said {
                Some(why) => bail!("{name} ({url}): {why}"),
                None => bail!("{name} ({url}) answered {s} to /health: is it an itero server?"),
            }
        }
    }
    let v: serde_json::Value = r
        .json()
        .map_err(|e| anyhow::anyhow!("{name} ({url}): /health is not itero's: {e}"))?;
    match v["daemon"]["itero"].as_str() {
        Some(itero) => Ok(itero.to_string()),
        None => bail!("{name} ({url}): /health does not say its itero: is it an itero server?"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Context {
        Context {
            name: name.into(),
            url: "https://örnek.test/".into(),
            token: None,
            ws: None,
        }
    }

    #[test]
    fn names_are_letters_digits_dash_and_underscore() {
        assert_eq!(bad_name("üretim-2_a"), None);
        assert!(bad_name("a b").unwrap().contains("a name is letters"));
        assert!(bad_name("").is_some());
        assert!(bad_name("local").unwrap().contains("built in"));
    }

    #[test]
    fn a_server_url_ends_in_a_slash() {
        assert_eq!(server_url("http://h:1").unwrap(), "http://h:1/");
        assert_eq!(server_url("https://h/").unwrap(), "https://h/");
        assert!(server_url("h:1").is_err());
    }

    #[test]
    fn written_contexts_read_back_and_removing_the_current_one_clears_it() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ayar/contexts.json");
        assert_eq!(Contexts::read(&p).unwrap(), Contexts::default());
        let mut all = Contexts::default();
        assert!(!all.upsert(c("bir")));
        assert!(all.upsert(c("bir")));
        all.upsert(c("iki"));
        all.current = Some("iki".into());
        all.write(&p).unwrap();
        assert_eq!(Contexts::read(&p).unwrap(), all);
        assert!(all.remove("iki"));
        assert_eq!(all.current, None);
        assert!(!all.remove("iki"));
    }
}
