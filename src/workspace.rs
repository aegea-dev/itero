//! The workspace: the folder with itero.toml, and the folders it lists for
//! workflows and actions. One daemon runs one workspace.
use crate::workflow::{self, actions};
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
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
    #[serde(default = "default_triggers")]
    triggers: String,
    #[serde(default = "default_sources")]
    sources: Vec<String>,
}

fn default_sources() -> Vec<String> {
    vec!["sources".into()]
}

/// A workspace's `triggers/<workflow>.json`: triggers added to the
/// workflow's own, or (`replace`) used instead of them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerFile {
    #[serde(default)]
    replace: bool,
    triggers: Vec<workflow::format::Trigger>,
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

fn default_triggers() -> String {
    "triggers".into()
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub port: u16,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
    /// The workspace's own triggers for its workflows.
    pub triggers: PathBuf,
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
        triggers: folder(root, &s.triggers),
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

    /// Every action in the actions folders: each folder in them with an
    /// `action.json`. A name found twice is the first folder's.
    pub fn actions(&self) -> Result<Vec<(PathBuf, actions::Definition)>> {
        let mut out: Vec<(PathBuf, actions::Definition)> = vec![];
        for dir in &self.actions {
            if !dir.exists() {
                continue;
            }
            let files = listed(dir, |p| Some(p.join("action.json")).filter(|f| f.is_file()))?;
            for f in files {
                let d = actions::load_file(&f)?;
                if !out.iter().any(|(_, o)| o.name == d.name) {
                    let folder = f.parent().expect("a folder").to_path_buf();
                    out.push((folder, d));
                }
            }
        }
        Ok(out)
    }

    /// A workspace action by name, and its folder.
    pub fn action(&self, name: &str) -> Result<(PathBuf, actions::Definition)> {
        self.actions()?
            .into_iter()
            .find(|(_, d)| d.name == name)
            .ok_or_else(|| anyhow!("no action {name} in the actions folders"))
    }

    /// Every workflow in the workflows folders, checked, and the actions
    /// it names found. Two workflows with one name is an error naming both.
    /// A folder that is not there holds none: a workspace without
    /// workflows is idle (`itero check` says which folders are missing).
    pub fn workflows(&self) -> Result<Vec<(PathBuf, workflow::File)>> {
        let defs: Vec<actions::Definition> = self.actions()?.into_iter().map(|(_, d)| d).collect();
        let mut out: Vec<(PathBuf, workflow::File)> = vec![];
        for dir in &self.workflows {
            if !dir.exists() {
                continue;
            }
            let files = listed(dir, |p| {
                (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf())
            })?;
            for f in files {
                let w = workflow::load_file(&f)?;
                workflow::validate::actions(&f.display().to_string(), &w, &defs)?;
                if let Some((other, _)) = out.iter().find(|(_, o)| o.name == w.name) {
                    bail!(
                        "two workflows named {}: {} and {}",
                        w.name,
                        other.display(),
                        f.display()
                    );
                }
                out.push((f, w));
            }
        }
        self.merge_triggers(&mut out)?;
        Ok(out)
    }

    /// Every source in the sources folders, checked: its name, its folder,
    /// its definition. A name found twice is an error naming both files.
    pub fn sources(&self) -> Result<Vec<(String, PathBuf, crate::sources::Definition)>> {
        let mut out: Vec<(String, PathBuf, crate::sources::Definition)> = vec![];
        for dir in &self.sources {
            if !dir.exists() {
                continue;
            }
            let files = listed(dir, |p| {
                (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf())
            })?;
            for f in files {
                let d = crate::sources::load_file(&f)?;
                let name = f
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if let Some((_, other, _)) = out.iter().find(|(n, _, _)| *n == name) {
                    bail!(
                        "two sources named {name}: in {} and {}",
                        other.display(),
                        dir.display()
                    );
                }
                out.push((name, dir.clone(), d));
            }
        }
        Ok(out)
    }

    /// Every workflow in the simple form (`docs/design-simple.md`), named
    /// after its file. Two with one name is an error naming both.
    pub fn flows(&self) -> Result<Vec<(String, crate::flow::Flow)>> {
        let mut out: Vec<(String, crate::flow::Flow)> = vec![];
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
                let flow =
                    crate::flow::parse(&text).map_err(|e| anyhow!("{}: {e}", f.display()))?;
                let name = stem(&f);
                if out.iter().any(|(n, _)| *n == name) {
                    bail!("two workflows named {name}: the second is {}", f.display());
                }
                out.push((name, flow));
            }
        }
        Ok(out)
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
                if !crate::flow::is_message_type(&kind) {
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

    /// Applies each `triggers/<workflow>.json` to its workflow. A folder
    /// that is not there holds none.
    fn merge_triggers(&self, flows: &mut [(PathBuf, workflow::File)]) -> Result<()> {
        if !self.triggers.exists() {
            return Ok(());
        }
        let files = listed(&self.triggers, |p| {
            (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf())
        })?;
        for f in files {
            let name = f.display();
            let text =
                std::fs::read_to_string(&f).with_context(|| format!("cannot read {name}"))?;
            let tf: TriggerFile =
                serde_json::from_str(&text).map_err(|e| anyhow!("{name}: {e}"))?;
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let Some((_, w)) = flows.iter_mut().find(|(_, w)| w.name == stem) else {
                bail!("{name}: no workflow {stem}");
            };
            for t in &tf.triggers {
                workflow::validate::check_trigger(t).map_err(|e| anyhow!("{name}: {e}"))?;
            }
            if tf.replace {
                w.triggers = tf.triggers;
            } else {
                w.triggers.extend(tf.triggers);
            }
        }
        Ok(())
    }
}
