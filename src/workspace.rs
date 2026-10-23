//! The workspace: the folder with itero.config.json, and the folders it
//! lists for workflows, actions and sources. One daemon runs one workspace.
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The workspace's manifest: what it is, how it runs, and what its
/// workflows, actions and sources are.
pub const MANIFEST: &str = "itero.config.json";

/// The manifest's keys for how the workspace runs, and for its entries:
/// every other key at its top says what the workspace is.
const RUNNING: [&str; 6] = ["port", "folders", "limits", "workflow", "action", "source"];

/// `folders` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Folders {
    #[serde(default = "default_workflows")]
    workflows: Vec<String>,
    #[serde(default = "default_actions")]
    actions: Vec<String>,
    #[serde(default = "default_sources")]
    sources: Vec<String>,
}

impl Default for Folders {
    fn default() -> Folders {
        Folders {
            workflows: default_workflows(),
            actions: default_actions(),
            sources: default_sources(),
        }
    }
}

/// `limits` as written: sizes and durations are text, `"1GB"`, `"1d"`.
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
    quiet: Option<String>,
    most: Option<String>,
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
    /// How long a step may print nothing before it is stopped.
    pub quiet: std::time::Duration,
    /// How long a step may run at most, printing or not.
    pub most: std::time::Duration,
}

impl RawLimits {
    fn read(self) -> Result<Limits> {
        let count = |name: &str, v: Option<usize>, default: usize| match v {
            Some(0) => bail!("limits: {name} is at least 1"),
            v => Ok(v.unwrap_or(default)),
        };
        const DAY: u64 = 24 * 60 * 60;
        Ok(Limits {
            workflows: count("workflows", self.workflows, 100)?,
            runs: count("runs", self.runs, 1000)?,
            steps: count("steps", self.steps, 8)?,
            space: size("space", self.space, 1 << 30)?,
            keep: duration("keep", self.keep, DAY)?,
            log: size("log", self.log, 10 << 20)?,
            logs: count("logs", self.logs, 3)?,
            quiet: duration("quiet", self.quiet, 10 * 60)?,
            most: duration("most", self.most, DAY)?,
        })
    }
}

/// `"30m"`: a duration of at least a second; `default` is in seconds.
fn duration(name: &str, v: Option<String>, default: u64) -> Result<std::time::Duration> {
    let Some(t) = v else {
        return Ok(std::time::Duration::from_secs(default));
    };
    let d = humantime::parse_duration(&t)
        .map_err(|_| anyhow!("limits: {name}: a duration is like 30m, 2h or 1d, not \"{t}\""))?;
    if d.as_secs() == 0 {
        bail!("limits: {name} is at least 1s");
    }
    Ok(d)
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
    /// The daemon's port, when the manifest sets one; else the OS gives
    /// one (`daemon_port`).
    pub port: Option<u16>,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
    /// The folders with the workspace's sources.
    pub sources: Vec<PathBuf>,
    /// What the manifest says the workspace is: `name`, `version`,
    /// `description` and the person's own keys.
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

/// The manifest at `path`: a JSON object.
fn read_manifest(path: &Path) -> Result<Map<String, Value>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    match serde_json::from_str(&text).map_err(|e| anyhow!("{}: {e}", path.display()))? {
        Value::Object(m) => Ok(m),
        _ => bail!(
            "{}: the manifest is an object: {{}} is a whole workspace",
            path.display()
        ),
    }
}

/// The workspace `cwd` is in: the nearest folder upwards with
/// itero.config.json.
pub fn find(cwd: &Path) -> Result<Workspace> {
    let Some(root) = cwd.ancestors().find(|d| d.join(MANIFEST).is_file()) else {
        if let Some(old) = cwd.ancestors().find(|d| d.join("itero.toml").is_file()) {
            bail!(
                "{}: itero reads {MANIFEST} now: write its port, [limits] and [meta] there (meta's keys at the top)",
                old.join("itero.toml").display()
            );
        }
        bail!(
            "no {MANIFEST} here or above: write one ({{}} will do) to make this folder a workspace"
        );
    };
    let path = root.join(MANIFEST);
    let mut m = read_manifest(&path)?;
    let at = |e: serde_json::Error, key: &str| anyhow!("{}: {key}: {e}", path.display());
    let port = match m.remove("port") {
        Some(v) => Some(serde_json::from_value(v).map_err(|e| at(e, "port"))?),
        None => None,
    };
    let folders: Folders = match m.remove("folders") {
        Some(v) => serde_json::from_value(v).map_err(|e| at(e, "folders"))?,
        None => Folders::default(),
    };
    let limits: RawLimits = match m.remove("limits") {
        Some(v) => serde_json::from_value(v).map_err(|e| at(e, "limits"))?,
        None => RawLimits::default(),
    };
    let meta: Map<String, Value> = m
        .into_iter()
        .filter(|(k, _)| !RUNNING.contains(&k.as_str()))
        .collect();
    for k in meta.keys() {
        if matches!(k.as_str(), "id" | "created" | "path") {
            bail!(
                "{}: {k} is given by itero and cannot be set",
                path.display()
            );
        }
    }
    Ok(Workspace {
        root: root.to_path_buf(),
        port,
        workflows: folders.workflows.iter().map(|w| folder(root, w)).collect(),
        actions: folders.actions.iter().map(|a| folder(root, a)).collect(),
        sources: folders.sources.iter().map(|x| folder(root, x)).collect(),
        meta,
        limits: limits
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

/// Writes `path` through a temporary file beside it and a rename, so a
/// reader never sees half of it.
fn write_through(path: &Path, text: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))
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
            if v.get("meta").is_some() {
                bail!(
                    "{}: meta goes in {MANIFEST} now: \"workflow\": {{\"{name}\": {{\"description\": ...}}}}",
                    f.display()
                );
            }
            crate::flow::from_value(&v).map_err(|e| anyhow!("{}: {e}", f.display()))?;
            out.insert(name, v);
        }
        for (name, text) in BUILT_IN {
            if !out.contains_key(name) {
                out.insert(name.to_string(), serde_json::from_str(text)?);
            }
        }
        let entries = self.entries("workflow")?;
        let internal = |name: &str, v: &Value| {
            let yes = Some(&Value::Bool(true));
            v.pointer("/meta/internal") == yes
                || entries.get(name).and_then(|e| e.get("internal")) == yes
        };
        let counted = out.iter().filter(|(n, v)| !internal(n, v)).count();
        if counted > self.limits.workflows {
            bail!(
                "{counted} workflows: limits.workflows in itero.config.json allows {}",
                self.limits.workflows
            );
        }
        Ok(out)
    }

    /// Every workflow, read, by name (its file name).
    pub fn flows(&self) -> Result<Vec<(String, crate::flow::Flow)>> {
        let entries = self.entries("workflow")?;
        self.flow_values()?
            .into_iter()
            .map(|(n, v)| Ok((n.clone(), self.read_flow(&n, &v, &entries)?)))
            .collect()
    }

    /// The workflow `name`, its file read as `v`, with its entry in the
    /// manifest (`entries`, the `workflow` ones): what it is, its settings.
    pub fn read_flow(
        &self,
        name: &str,
        v: &Value,
        entries: &Map<String, Value>,
    ) -> Result<crate::flow::Flow> {
        let mut f = crate::flow::from_value(v).map_err(|e| anyhow!("{name}: {e}"))?;
        // a built-in workflow says what it is in its own JSON; the entry
        // goes on top
        let mut entry = match v.get("meta") {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        if let Some(Value::Object(e)) = entries.get(name) {
            entry.extend(e.clone());
        }
        let (meta, settings) = crate::flow::entry(&Value::Object(entry)).map_err(|e| {
            anyhow!(
                "{}: workflow.{name}: {e}",
                self.root.join(MANIFEST).display()
            )
        })?;
        f.meta = meta;
        f.settings = settings;
        Ok(f)
    }

    /// The manifest as written now: read again each time, so an edit
    /// counts from the next pass, without a restart.
    pub fn manifest(&self) -> Result<Map<String, Value>> {
        read_manifest(&self.root.join(MANIFEST))
    }

    /// The manifest's entries of one kind (`workflow`, `action` or
    /// `source`), by name; each an object.
    pub fn entries(&self, kind: &str) -> Result<Map<String, Value>> {
        let path = self.root.join(MANIFEST);
        match self.manifest()?.remove(kind) {
            None => Ok(Map::new()),
            Some(Value::Object(m)) => {
                if let Some((name, _)) = m.iter().find(|(_, e)| !e.is_object()) {
                    bail!(
                        "{}: {kind}.{name} is an object: {{\"description\": \"...\"}}",
                        path.display()
                    );
                }
                Ok(m)
            }
            Some(_) => bail!(
                "{}: {kind} is an object: {{\"<name>\": {{...}}}}",
                path.display()
            ),
        }
    }

    /// Sets `key` of the `kind` entry `name` to `v` in the manifest,
    /// keeping the rest as it was, written through a temporary file; the
    /// manifest's path.
    pub fn write_entry(&self, kind: &str, name: &str, key: &str, v: Value) -> Result<PathBuf> {
        let path = self.root.join(MANIFEST);
        let mut m = self.manifest()?;
        let Value::Object(all) = m.entry(kind).or_insert_with(|| Value::Object(Map::new())) else {
            bail!("{}: {kind} is an object", path.display());
        };
        let Value::Object(e) = all.entry(name).or_insert_with(|| Value::Object(Map::new())) else {
            bail!("{}: {kind}.{name} is an object", path.display());
        };
        e.insert(key.into(), v);
        write_through(
            &path,
            &(serde_json::to_string_pretty(&Value::Object(m))? + "\n"),
        )?;
        Ok(path)
    }

    /// Where the daemon keeps the port the OS gave it.
    fn port_file(&self) -> PathBuf {
        self.root.join(".itero").join("port")
    }

    /// The port the OS last gave the daemon, when the manifest sets none.
    pub fn saved_port(&self) -> Option<u16> {
        std::fs::read_to_string(self.port_file())
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// The daemon's port: the manifest's, else the one it was last given.
    pub fn daemon_port(&self) -> Option<u16> {
        self.port.or_else(|| self.saved_port())
    }

    /// Keeps the port the OS gave the daemon, for its next start and for
    /// whoever asks it (`itero info`, the watchdog).
    pub fn save_port(&self, port: u16) -> Result<()> {
        let f = self.port_file();
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot make {}", dir.display()))?;
        }
        write_through(&f, &format!("{port}\n"))
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

    /// An action's args, `action.<name>.args` in the manifest: an object,
    /// or a list of objects (the step runs once per item). None when its
    /// entry has none.
    pub fn args(&self, action: &str) -> Result<Option<Value>> {
        let Some(v) = self.entry_args("action", action)? else {
            return Ok(None);
        };
        let ok = match &v {
            Value::Object(_) => true,
            Value::Array(items) => items.iter().all(|i| i.is_object()),
            _ => false,
        };
        if !ok {
            bail!(
                "{}: action.{action}.args are an object, or a list of objects",
                self.root.join(MANIFEST).display()
            );
        }
        Ok(Some(v))
    }

    /// A source's args, `source.<name>.args` in the manifest: an object (a
    /// source runs once). None when its entry has none.
    pub fn source_args(&self, source: &str) -> Result<Option<Map<String, Value>>> {
        match self.entry_args("source", source)? {
            None => Ok(None),
            Some(Value::Object(o)) => Ok(Some(o)),
            Some(_) => bail!(
                "{}: source.{source}.args are an object",
                self.root.join(MANIFEST).display()
            ),
        }
    }

    /// `<kind>.<name>.args` in the manifest, as written; `null` is none.
    fn entry_args(&self, kind: &str, name: &str) -> Result<Option<Value>> {
        Ok(match self.entries(kind)?.remove(name) {
            Some(Value::Object(mut e)) => e.remove("args").filter(|v| !v.is_null()),
            _ => None,
        })
    }
}
