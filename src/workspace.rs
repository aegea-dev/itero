//! The workspace: the folder with itero.toml, and the folders it lists for
//! workflows, actions and sources. One daemon runs one workspace.
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_workflows")]
    workflows: Vec<String>,
    #[serde(default = "default_actions")]
    actions: Vec<String>,
    #[serde(default = "default_sources")]
    sources: Vec<String>,
}

fn default_sources() -> Vec<String> {
    vec!["sources".into()]
}

fn default_port() -> u16 {
    8787
}

fn default_workflows() -> Vec<String> {
    vec!["workflows".into()]
}

fn default_actions() -> Vec<String> {
    vec!["actions".into()]
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub port: u16,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
    /// The folders with the workspace's sources.
    pub sources: Vec<PathBuf>,
}

/// A listed folder: `~/` is the home folder, a relative path is under the
/// workspace.
fn folder(root: &Path, s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => root.join(s),
    }
}

/// The workspace `cwd` is in: the nearest folder upwards with itero.toml.
pub fn find(cwd: &Path) -> Result<Workspace> {
    let Some(root) = cwd.ancestors().find(|d| d.join("itero.toml").is_file()) else {
        bail!(
            "no itero.toml here or above: write one (port, workflows, actions) to make this folder a workspace"
        );
    };
    let path = root.join("itero.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let s: Settings = toml::from_str(&text).with_context(|| format!("{}", path.display()))?;
    Ok(Workspace {
        root: root.to_path_buf(),
        port: s.port,
        workflows: s.workflows.iter().map(|w| folder(root, w)).collect(),
        actions: s.actions.iter().map(|a| folder(root, a)).collect(),
        sources: s.sources.iter().map(|x| folder(root, x)).collect(),
    })
}

/// The files in `dir` that `keep` picks, sorted; a missing folder is an
/// error naming it.
fn listed(dir: &Path, keep: impl Fn(&Path) -> Option<PathBuf>) -> Result<Vec<PathBuf>> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    let mut out = vec![];
    for e in entries {
        if let Some(p) = keep(&e?.path()) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// A file's name without its extension.
fn stem(f: &Path) -> String {
    f.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A file's whole name: actions and sources have no extension, and their
/// names have dots (`notify@0.2`, `mail.received`).
fn stem_with_ext(f: &Path) -> String {
    f.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(unix)]
fn executable(f: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(f).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(f: &Path) -> bool {
    f.is_file()
}

impl Workspace {
    /// Where the journal lives: local, never synced.
    pub fn journal(&self) -> PathBuf {
        self.root.join(".itero").join("journal.sqlite")
    }

    /// Every workflow, as its file's JSON, by name (its file name), each
    /// checked. Two with one name is an error naming the second.
    pub fn flow_values(&self) -> Result<BTreeMap<String, Value>> {
        let mut out = BTreeMap::new();
        for dir in &self.workflows {
            if !dir.exists() {
                continue;
            }
            let files = listed(dir, |p| {
                (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf())
            })?;
            for f in files {
                let text = std::fs::read_to_string(&f)
                    .with_context(|| format!("cannot read {}", f.display()))?;
                let v: Value =
                    serde_json::from_str(&text).map_err(|e| anyhow!("{}: {e}", f.display()))?;
                crate::flow::from_value(&v).map_err(|e| anyhow!("{}: {e}", f.display()))?;
                let name = stem(&f);
                if out.contains_key(&name) {
                    bail!("two workflows named {name}: the second is {}", f.display());
                }
                out.insert(name, v);
            }
        }
        Ok(out)
    }

    /// Every workflow, read, by name (its file name).
    pub fn flows(&self) -> Result<Vec<(String, crate::flow::Flow)>> {
        self.flow_values()?
            .into_iter()
            .map(|(n, v)| {
                let f = crate::flow::from_value(&v).map_err(|e| anyhow!("{n}: {e}"))?;
                Ok((n, f))
            })
            .collect()
    }

    /// An action's file: `name@version` when pinned, else the highest
    /// version, else the plain `name`. It must be executable.
    pub fn action_file(&self, name: &str, version: Option<&str>) -> Result<PathBuf> {
        let mut found: Option<(Vec<u64>, PathBuf)> = None;
        for dir in self.actions.iter().filter(|d| d.exists()) {
            for f in listed(dir, |p| p.is_file().then(|| p.to_path_buf()))? {
                let file = stem_with_ext(&f);
                let key = match (version, file.split_once('@')) {
                    (Some(v), Some((n, fv))) if n == name && fv == v => vec![],
                    (None, Some((n, fv))) if n == name => crate::flow::version_key(fv),
                    // a plain file is the latest only when nothing has a version
                    (None, None) if file == name => vec![],
                    _ => continue,
                };
                if found.as_ref().is_none_or(|(k, _)| key > *k) {
                    found = Some((key, f));
                }
            }
        }
        let Some((_, f)) = found else {
            return Err(match version {
                Some(v) => anyhow!("no action {name}@{v} in the actions folders"),
                None => anyhow!("no action {name} in the actions folders"),
            });
        };
        if !executable(&f) {
            bail!("{} is not executable: chmod +x it", f.display());
        }
        Ok(f)
    }

    /// Every source in the simple form: an executable whose file name is
    /// the type of the messages it sends.
    pub fn source_files(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut out: Vec<(String, PathBuf)> = vec![];
        for dir in self.sources.iter().filter(|d| d.exists()) {
            for f in listed(dir, |p| p.is_file().then(|| p.to_path_buf()))? {
                let kind = stem_with_ext(&f);
                if !crate::flow::is_message_type(&kind) || crate::engine::events::engine_own(&kind)
                {
                    bail!(
                        "{}: a source's name is the message type it sends, two or more words joined by . (like mail.received)",
                        f.display()
                    );
                }
                if !executable(&f) {
                    bail!("{} is not executable: chmod +x it", f.display());
                }
                if out.iter().any(|(k, _)| *k == kind) {
                    bail!("two sources named {kind}: the second is {}", f.display());
                }
                out.push((kind, f));
            }
        }
        Ok(out)
    }

    /// An action's args, `args/<action>.json`: an object, or a list of
    /// objects (the step runs once per item). None when there is no file.
    pub fn args(&self, action: &str) -> Result<Option<serde_json::Value>> {
        let f = self.root.join("args").join(format!("{action}.json"));
        if !f.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&f).with_context(|| format!("cannot read {}", f.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| anyhow!("{}: {e}", f.display()))?;
        let ok = match &v {
            serde_json::Value::Object(_) => true,
            serde_json::Value::Array(items) => items.iter().all(|i| i.is_object()),
            _ => false,
        };
        if !ok {
            bail!("{}: args are an object, or a list of objects", f.display());
        }
        Ok(Some(v))
    }
}
