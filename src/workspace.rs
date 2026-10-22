//! The workspace: the folder with itero.toml, and the folders it lists for
//! workflows, actions and sources. One daemon runs one workspace.
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
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
    #[serde(default)]
    meta: Map<String, Value>,
    #[serde(default)]
    limits: RawLimits,
}

/// `[limits]` as written: sizes and durations are text, `"1GB"`, `"1d"`.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    workflows: Option<usize>,
    runs: Option<usize>,
    steps: Option<usize>,
    space: Option<String>,
    keep: Option<String>,
    log: Option<String>,
    logs: Option<usize>,
}

/// How much a workspace may hold and do at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// Workflows in its folders.
    pub workflows: usize,
    /// Runs under way at once; past it, messages wait their turn.
    pub runs: usize,
    /// Steps running at once.
    pub steps: usize,
    /// Bytes `.itero/` may take.
    pub space: u64,
    /// How long ended runs and their messages are kept.
    pub keep: std::time::Duration,
    /// Bytes the log may reach before it is rolled.
    pub log: u64,
    /// Rolled logs kept.
    pub logs: usize,
}

impl RawLimits {
    fn read(self) -> Result<Limits> {
        let count = |name: &str, v: Option<usize>, default: usize| match v {
            Some(0) => bail!("limits: {name} is at least 1"),
            v => Ok(v.unwrap_or(default)),
        };
        let keep = match self.keep {
            None => std::time::Duration::from_secs(24 * 60 * 60),
            Some(t) => humantime::parse_duration(&t).map_err(|_| {
                anyhow!("limits: keep: a duration is like 30m, 2h or 1d, not \"{t}\"")
            })?,
        };
        Ok(Limits {
            workflows: count("workflows", self.workflows, 100)?,
            runs: count("runs", self.runs, 1000)?,
            steps: count("steps", self.steps, 8)?,
            space: size("space", self.space, 1 << 30)?,
            keep,
            log: size("log", self.log, 10 << 20)?,
            logs: count("logs", self.logs, 3)?,
        })
    }
}

/// `"10MB"`: a number and KB, MB or GB (of 1024).
fn size(name: &str, v: Option<String>, default: u64) -> Result<u64> {
    let Some(s) = v else {
        return Ok(default);
    };
    let wrong = || anyhow!("limits: {name}: a size is like 10MB or 1GB, not \"{s}\"");
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = n.parse().map_err(|_| wrong())?;
    let shift = match unit.trim() {
        "KB" => 10,
        "MB" => 20,
        "GB" => 30,
        _ => return Err(wrong()),
    };
    n.checked_mul(1 << shift).ok_or_else(wrong)
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

/// The workflows itero carries, run as if they were in `workflows/`; a
/// file of the same name there is used instead.
pub const BUILT_IN: [(&str, &str); 2] = [
    ("keep", include_str!("builtin/keep.json")),
    ("logs", include_str!("builtin/logs.json")),
];

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub port: u16,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
    /// The folders with the workspace's sources.
    pub sources: Vec<PathBuf>,
    /// `[meta]`: what a person says about the workspace.
    pub meta: Map<String, Value>,
    pub limits: Limits,
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
    for k in s.meta.keys() {
        crate::flow::given_by_itero(k).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    }
    Ok(Workspace {
        root: root.to_path_buf(),
        port: s.port,
        workflows: s.workflows.iter().map(|w| folder(root, w)).collect(),
        actions: s.actions.iter().map(|a| folder(root, a)).collect(),
        sources: s.sources.iter().map(|x| folder(root, x)).collect(),
        meta: s.meta,
        limits: s
            .limits
            .read()
            .with_context(|| format!("{}", path.display()))?,
    })
}

/// The files in `dir` and its subfolders that `keep` picks, sorted; a
/// missing folder is an error naming it. Subfolders only group: hidden ones
/// (`.venv`, `.git`) and `node_modules` belong to tools and are skipped.
fn listed(dir: &Path, keep: impl Fn(&Path) -> Option<PathBuf>) -> Result<Vec<PathBuf>> {
    fn walk(
        dir: &Path,
        keep: &dyn Fn(&Path) -> Option<PathBuf>,
        out: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let entries =
            std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
        for e in entries {
            let p = e?.path();
            if p.is_dir() {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                let tools = name.is_some_and(|n| n.starts_with('.') || n == "node_modules");
                if !tools {
                    walk(&p, keep, out)?;
                }
            } else if let Some(p) = keep(&p) {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut out = vec![];
    walk(dir, &keep, &mut out)?;
    out.sort();
    Ok(out)
}

/// `files` by name, one per name: a second file of one name is an error
/// naming both.
fn named(
    kind: &str,
    files: impl IntoIterator<Item = (String, PathBuf)>,
) -> Result<BTreeMap<String, PathBuf>> {
    let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (name, f) in files {
        if let Some(first) = out.get(&name) {
            bail!(
                "two {kind}s named {name}: {} and {}",
                first.display(),
                f.display()
            );
        }
        out.insert(name, f);
    }
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
    /// A path as people read it: from the workspace when it is in it.
    pub fn shown(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .display()
            .to_string()
    }

    /// Where the journal lives: local, never synced.
    pub fn journal(&self) -> PathBuf {
        self.root.join(".itero").join("journal.sqlite")
    }

    /// The bytes `.itero/` takes: the journal, the logs, memory.
    pub fn used(&self) -> u64 {
        fn size(p: &Path) -> u64 {
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => std::fs::read_dir(p)
                    .map(|es| es.flatten().map(|e| size(&e.path())).sum())
                    .unwrap_or(0),
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        }
        size(&self.root.join(".itero"))
    }

    /// Every workflow's file, by name (its file name), in any subfolder of
    /// the workflows folders. Two with one name is an error naming both.
    pub fn flow_files(&self) -> Result<BTreeMap<String, PathBuf>> {
        let mut files = vec![];
        for dir in self.workflows.iter().filter(|d| d.exists()) {
            for f in listed(dir, |p| {
                (p.extension().is_some_and(|x| x == "json") && p.is_file()).then(|| p.to_path_buf())
            })? {
                files.push((stem(&f), f));
            }
        }
        named("workflow", files)
    }

    /// Every workflow, as its file's JSON, by name, each checked, and the
    /// built-in ones no file replaces. More than `limits.workflows`, the
    /// internal ones apart, is an error.
    pub fn flow_values(&self) -> Result<BTreeMap<String, Value>> {
        let mut out = BTreeMap::new();
        for (name, f) in self.flow_files()? {
            let text = std::fs::read_to_string(&f)
                .with_context(|| format!("cannot read {}", f.display()))?;
            let v: Value =
                serde_json::from_str(&text).map_err(|e| anyhow!("{}: {e}", f.display()))?;
            crate::flow::from_value(&v).map_err(|e| anyhow!("{}: {e}", f.display()))?;
            out.insert(name, v);
        }
        for (name, text) in BUILT_IN {
            if !out.contains_key(name) {
                out.insert(name.to_string(), serde_json::from_str(text)?);
            }
        }
        let counted = out
            .values()
            .filter(|v| v.pointer("/meta/internal") != Some(&Value::Bool(true)))
            .count();
        if counted > self.limits.workflows {
            bail!(
                "{counted} workflows: workflows in itero.toml's [limits] allows {}",
                self.limits.workflows
            );
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

    /// The names of the internal workflows: whose runs lists leave out
    /// unless asked.
    pub fn internal(&self) -> Result<Vec<String>> {
        Ok(self
            .flows()?
            .into_iter()
            .filter(|(_, f)| f.meta.internal)
            .map(|(n, _)| n)
            .collect())
    }

    /// Every action's file, by its whole file name (`notify@0.2`), in any
    /// subfolder of the actions folders. Only executables are actions: the
    /// files beside them (a `package.json`, a `README.md`) are left alone.
    pub fn action_files(&self) -> Result<BTreeMap<String, PathBuf>> {
        named("action", self.files_in(&self.actions, true)?)
    }

    /// The files in `dirs` and their subfolders, by whole file name: the
    /// executable ones, or the others.
    fn files_in(&self, dirs: &[PathBuf], runnable: bool) -> Result<Vec<(String, PathBuf)>> {
        let mut files = vec![];
        for dir in dirs.iter().filter(|d| d.exists()) {
            for f in listed(dir, |p| {
                (p.is_file() && executable(p) == runnable).then(|| p.to_path_buf())
            })? {
                files.push((stem_with_ext(&f), f));
            }
        }
        Ok(files)
    }

    /// An action's file: `name@version` when pinned, else the highest
    /// version, else the plain `name`. It must be executable.
    pub fn action_file(&self, name: &str, version: Option<&str>) -> Result<PathBuf> {
        let mut found: Option<(Vec<u64>, PathBuf)> = None;
        for (file, f) in self.action_files()? {
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
        let Some((_, f)) = found else {
            // a file of that name that is not executable: most likely a
            // forgotten chmod
            let wanted = match version {
                Some(v) => format!("{name}@{v}"),
                None => name.to_string(),
            };
            if let Some((_, f)) = self
                .files_in(&self.actions, false)?
                .into_iter()
                .find(|(file, _)| *file == wanted)
            {
                bail!("{} is not executable: chmod +x it", f.display());
            }
            bail!("no action {wanted} in the actions folders");
        };
        Ok(f)
    }

    /// Every source in the simple form: an executable whose file name is
    /// the type of the messages it sends.
    pub fn source_files(&self) -> Result<Vec<(String, PathBuf)>> {
        // only executables are sources: notes beside them are left alone
        let files = self.files_in(&self.sources, true)?;
        for (kind, f) in &files {
            if !crate::flow::is_message_type(kind) || crate::engine::events::engine_own(kind) {
                bail!(
                    "{}: a source's name is the message type it sends, two or more words joined by . (like mail.received)",
                    f.display()
                );
            }
        }
        Ok(named("source", files)?.into_iter().collect())
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
