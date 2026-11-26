//! The workspace: the folder with itero.config.json, and the folders it
//! lists for workflows, actions and connectors. One daemon runs one workspace.
use crate::manifest::{Folders, RawLimits};
use crate::problem::Problem;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Workflows read, by name (their file names).
pub type Flows = Vec<(String, crate::flow::Flow)>;

/// Workflows as their files' JSON, by name.
type Values = BTreeMap<String, Value>;

/// What reading every workflow found: those that could be read, the
/// problems of the rest, and every file that is JSON as written (with
/// the built-in ones), so its steps can be checked even when the
/// workflow as a whole cannot be read.
#[derive(Default)]
pub struct ReadFlows {
    pub flows: Flows,
    pub problems: Vec<Problem>,
    pub written: BTreeMap<String, Value>,
    /// Each workflow read, as its JSON (each file an arg names inlined).
    pub values: BTreeMap<String, Value>,
}

/// The workspace's manifest: what it is, how it runs, and what its
/// workflows, actions and connectors are.
pub const MANIFEST: &str = "itero.config.json";

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
            steps: count("steps", self.steps, 64)?,
            space: size("space", self.space, 1 << 30)?,
            keep: duration("keep", self.keep, DAY)?,
            log: size("log", self.log, 10 << 20)?,
            logs: count("logs", self.logs, 3)?,
            quiet: duration("quiet", self.quiet, 10 * 60)?,
            most: duration("most", self.most, DAY)?,
        })
    }
}

impl Limits {
    /// What is allowed but cannot work: `ws check` and the daemon's start
    /// say it. The log and its rolled copies count in `.itero/`, and
    /// `#forget` cannot make them smaller: at `space`, it stays full.
    pub fn warnings(&self) -> Vec<String> {
        let log = self.log.saturating_mul(self.logs as u64 + 1);
        let than = match log.cmp(&self.space) {
            std::cmp::Ordering::Less => return vec![],
            std::cmp::Ordering::Equal => "as much as",
            std::cmp::Ordering::Greater => "more than",
        };
        let copies = match self.logs {
            1 => "1 rolled copy".to_string(),
            n => format!("{n} rolled copies"),
        };
        vec![format!(
            "the log and its {copies} may take {}, {than} space ({}): .itero/ would stay full, and only internal runs would start",
            size_text(log),
            size_text(self.space)
        )]
    }
}

/// A size as `limits` writes it: in the largest unit it is whole in.
fn size_text(bytes: u64) -> String {
    for (shift, unit) in [(30, "GB"), (20, "MB"), (10, "KB")] {
        if bytes.is_multiple_of(1 << shift) {
            return format!("{}{unit}", bytes >> shift);
        }
    }
    format!("{bytes} bytes")
}

/// `"30m"`: a duration of at least a second; `default` is in seconds.
pub(crate) fn duration(name: &str, v: Option<String>, default: u64) -> Result<std::time::Duration> {
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
pub(crate) fn size(name: &str, v: Option<String>, default: u64) -> Result<u64> {
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

/// The workflows itero carries, run as if they were in `workflows/`; a
/// file of the same name there is used instead.
pub const BUILT_IN: [(&str, &str); 3] = [
    ("keep", include_str!("builtin/keep.json")),
    ("logs", include_str!("builtin/logs.json")),
    ("models", include_str!("builtin/models.json")),
];

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    /// The daemon's port, when the manifest sets one; else the OS gives
    /// one (`daemon_port`).
    pub port: Option<u16>,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
    /// The folders with the workspace's connectors.
    pub connectors: Vec<PathBuf>,
    /// What the manifest says the workspace is: `name`, `version`,
    /// `description` and the person's own keys.
    pub meta: Map<String, Value>,
    pub limits: Limits,
    /// On the machine's list: run by `itero daemon`, whose one scan
    /// for models stands for the built-in `models` workflow.
    pub machine: bool,
    /// Its id, once read (`id`).
    known_id: std::sync::OnceLock<String>,
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

/// The workspace `cwd` is in, the nearest folder upwards with
/// itero.config.json, as far as its config can be read: what `ws check`
/// looks into when the config has problems. Every other use goes through
/// `find`, which refuses them.
pub fn find_as_is(cwd: &Path) -> Result<Workspace> {
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
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let (v, _) = crate::manifest::problems(&text);
    let m = match v {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    // what can be read of it makes the workspace: enough to check its
    // entries against its folders; a config with a problem is refused below
    let port = m
        .get("port")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let folders: Folders = m
        .get("folders")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let limits = m
        .get("limits")
        .and_then(|v| serde_json::from_value::<RawLimits>(v.clone()).ok())
        .and_then(|l| l.read().ok())
        .unwrap_or_else(|| RawLimits::default().read().expect("the defaults read"));
    let meta: Map<String, Value> = m
        .into_iter()
        .filter(|(k, _)| ABOUT.contains(&k.as_str()))
        .collect();
    Ok(Workspace {
        root: root.to_path_buf(),
        port,
        workflows: folders.workflows.iter().map(|w| folder(root, w)).collect(),
        actions: folders.actions.iter().map(|a| folder(root, a)).collect(),
        connectors: folders.connectors.iter().map(|x| folder(root, x)).collect(),
        meta,
        limits,
        machine: on_the_list(root),
        known_id: std::sync::OnceLock::new(),
    })
}

/// The workspace `cwd` is in, refused when its config has a problem: every
/// one, in a `BadConfig`.
pub fn find(cwd: &Path) -> Result<Workspace> {
    let ws = find_as_is(cwd)?;
    let root = ws.root.clone();
    let root = root.as_path();
    let problems = ws.manifest_problems()?;
    if !problems.is_empty() {
        let name = match ws.journal().is_file() {
            true => {
                crate::engine::journal::Journal::open(&ws)?
                    .workspace()?
                    .name
            }
            false => root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        return Err(BadConfig {
            root: root.to_path_buf(),
            name,
            problems: ws.placed(problems),
        }
        .into());
    }
    Ok(ws)
}

/// The config's keys that say what the workspace is: kept as written, for
/// `ws info`, `/health` and the log's start.
const ABOUT: [&str; 5] = ["name", "version", "description", "tags", "metadata"];

/// A workspace whose config has problems: every one, each in its file at
/// its place, for every command to tell as `ws check` does.
#[derive(Debug)]
pub struct BadConfig {
    pub root: PathBuf,
    /// The workspace's name, for the problems' addresses.
    pub name: String,
    pub problems: Vec<Problem>,
}

impl std::fmt::Display for BadConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}: {}",
            self.root.join(MANIFEST).display(),
            crate::flow::told(&self.problems)
        )
    }
}

impl std::error::Error for BadConfig {}

impl BadConfig {
    /// The config's problems in `e`, when that is what it is.
    pub fn of(e: &anyhow::Error) -> Option<&BadConfig> {
        e.chain().find_map(|c| c.downcast_ref::<BadConfig>())
    }
}

/// Whether the machine's list holds the workspace at `root`.
fn on_the_list(root: &Path) -> bool {
    crate::machine::list_path()
        .and_then(|p| crate::machine::List::read(&p))
        .is_ok_and(|l| l.workspaces.iter().any(|e| e.folder == root))
}

/// The files in `dir` and its subfolders that `keep` picks, sorted; a
/// missing folder is an error naming it. Subfolders only group: hidden ones
/// (`.venv`, `.git`) and `node_modules` belong to tools and are skipped.
fn listed(dir: &Path, keep: impl Fn(&Path) -> Option<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    walk(dir, &|p| (!p.is_dir()).then(|| keep(p)).flatten(), &mut out)?;
    out.sort();
    Ok(out)
}

/// What `dir` and its subfolders hold that `keep` keeps, folders included;
/// tools' folders (`.git`, `node_modules`) are passed over.
fn walk(dir: &Path, keep: &dyn Fn(&Path) -> Option<PathBuf>, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
    for e in entries {
        let p = e?.path();
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            if name.is_some_and(|n| n.starts_with('.') || n == "node_modules") {
                continue;
            }
            if let Some(kept) = keep(&p) {
                out.push(kept);
            }
            walk(&p, keep, out)?;
        } else if let Some(kept) = keep(&p) {
            out.push(kept);
        }
    }
    Ok(())
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

/// A file's whole name: actions and connectors have no extension, and their
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
    /// The workspace's id, from its journal: what stays when it is renamed.
    /// Read once.
    pub fn id(&self) -> Result<String> {
        if let Some(id) = self.known_id.get() {
            return Ok(id.clone());
        }
        let id = crate::engine::journal::read_id(self)?;
        Ok(self.known_id.get_or_init(|| id).clone())
    }

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
    /// The file another process touches once it has saved a message in
    /// the journal: the running daemon watches it, and wakes.
    /// The pipe another process wakes the daemon through (`engine::wake`).
    pub fn wake_file(&self) -> PathBuf {
        self.root.join(".itero").join("wake")
    }

    /// Written when the workspace stops: the daemon's file watch watches it
    /// to end its wait.
    pub fn stop_file(&self) -> PathBuf {
        self.root.join(".itero").join("stop")
    }

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
    /// What the daemon reads to know its workflows, for it to watch: the
    /// manifest, each workflow folder and its subfolders (a file comes or
    /// goes there), and each workflow file.
    pub fn own_files(&self) -> Result<Vec<PathBuf>> {
        let mut out = vec![self.root.join(MANIFEST)];
        for dir in &self.workflows {
            out.push(dir.clone());
            if dir.exists() {
                walk(
                    dir,
                    &|p| {
                        (p.is_dir() || p.extension().is_some_and(|x| x == "json"))
                            .then(|| p.to_path_buf())
                    },
                    &mut out,
                )?;
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

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
        let (values, problems) = self.read_flow_values()?;
        if !problems.is_empty() {
            bail!("{}", crate::flow::told(&problems));
        }
        Ok(values)
    }

    /// Every workflow file that can be read, as its JSON, by name, and the
    /// built-in ones no file replaces; and the problems of the rest, at
    /// `workflow/<name>/...`. An error only for what is not the workflows'
    /// own (a folder that cannot be listed).
    pub fn read_flow_values(&self) -> Result<(BTreeMap<String, Value>, Vec<Problem>)> {
        let (values, problems, _) = self.read_flow_files()?;
        Ok((values, problems))
    }

    /// As `read_flow_values`, and every file that is JSON as written, by
    /// name, whatever else is wrong with it.
    fn read_flow_files(&self) -> Result<(Values, Vec<Problem>, Values)> {
        let (mut out, mut problems, mut written) = (BTreeMap::new(), vec![], BTreeMap::new());
        for (name, f) in self.flow_files()? {
            let at = format!("workflows/{name}");
            let text = match std::fs::read_to_string(&f) {
                Ok(t) => t,
                Err(e) => {
                    problems.push(Problem::new(
                        at,
                        format!("cannot read {}: {e}", f.display()),
                    ));
                    continue;
                }
            };
            // past a syntax error nothing more of the file can be read
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    problems.push(Problem::new(at, format!("not JSON: {e}")));
                    continue;
                }
            };
            written.insert(name.clone(), v.clone());
            // every step that can be read, whatever else is wrong
            problems.extend(
                self.built_in_problems(&v)
                    .into_iter()
                    .map(|(place, m)| Problem::new(format!("{at}/{place}"), m)),
            );
            if v.get("meta").is_some() {
                problems.push(Problem::new(
                    at,
                    format!(
                        "meta goes in {MANIFEST} now: \"workflow\": {{\"{name}\": {{\"description\": ...}}}}"
                    ),
                ));
                continue;
            }
            // each file an arg names becomes its text, before anything reads it
            let mut v = v;
            let files = crate::file_arg::inline(&self.root, &mut v);
            let usable = files.is_empty();
            problems.extend(files.into_iter().map(|why| Problem::new(at.clone(), why)));
            match crate::flow::read(&v) {
                Ok(_) if usable => {
                    out.insert(name, v);
                }
                Ok(_) => {}
                Err(ps) => problems.extend(ps.into_iter().map(|p| p.under(&at))),
            }
        }
        for (name, text) in BUILT_IN {
            // the machine's daemon scans for models once, for every workspace
            if self.machine && name == "models" {
                continue;
            }
            if !out.contains_key(name) && !written.contains_key(name) {
                let v: Value = serde_json::from_str(text)?;
                written.insert(name.to_string(), v.clone());
                out.insert(name.to_string(), v);
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
            problems.push(Problem::new(
                "limits",
                format!(
                    "{counted} workflows, and limits.workflows allows {}",
                    self.limits.workflows
                ),
            ));
        }
        Ok((out, problems, written))
    }

    /// Every workflow, read, by name (its file name).
    pub fn flows(&self) -> Result<Flows> {
        let read = self.read_flows()?;
        if !read.problems.is_empty() {
            bail!("{}", crate::flow::told(&read.problems));
        }
        Ok(read.flows)
    }

    /// Every workflow that can be read, by name, and the problems of the
    /// rest, each at its address (`workflow/posta/steps/#ai`).
    pub fn read_flows(&self) -> Result<ReadFlows> {
        let entries = self.entries("workflow")?;
        let (values, mut problems, written) = self.read_flow_files()?;
        let mut flows = vec![];
        for (n, v) in &values {
            match self.flow_with_entry(n, v, &entries) {
                Ok(f) => flows.push((n.clone(), f)),
                Err(ps) => problems.extend(ps),
            }
        }
        Ok(ReadFlows {
            flows,
            problems,
            written,
            values,
        })
    }
    /// What the config has wrong, refused by every command and the daemon
    /// alike: what its schema refuses (`manifest::problems`), an entry naming
    /// no workflow, action or connector, and the folders from before it
    /// (`args/`, `config/`). Read from the file as it is now, so an edit
    /// counts from the next pass.
    pub fn manifest_problems(&self) -> Result<Vec<Problem>> {
        let path = self.root.join(MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let (v, mut wrong) = crate::manifest::problems(&text);
        for (dir, now) in [
            (
                "args",
                "an action's args are written in the step that runs it",
            ),
            (
                "config",
                "a workflow's values go in .env, a step's args or the event",
            ),
        ] {
            if self.root.join(dir).is_dir() {
                wrong.push(Problem::new("", format!("{dir}/: is gone: {now}")));
            }
        }
        // entries are checked against what is there once they can be read
        let Some(Value::Object(m)) = v else {
            return Ok(wrong);
        };
        // a built-in workflow is there even where it is not read (the
        // machine's daemon scans for models once, for every workspace)
        let workflows: Vec<String> = self
            .flow_files()?
            .into_keys()
            .chain(BUILT_IN.iter().map(|(n, _)| n.to_string()))
            .collect();
        let actions: Vec<String> = self
            .action_files()?
            .into_keys()
            .map(|f| f.split('@').next().unwrap_or(&f).to_string())
            .chain(
                crate::engine::worker::BUILT_IN
                    .iter()
                    .map(|b| b.to_string()),
            )
            .collect();
        let connectors: Vec<String> = self
            .connector_files()?
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        for (kind, known) in [
            ("workflow", workflows),
            ("action", actions),
            ("connector", connectors),
        ] {
            let Some(Value::Object(entries)) = m.get(kind) else {
                continue;
            };
            for name in entries.keys() {
                if !known.contains(name) {
                    wrong.push(
                        Problem::new(format!("{kind}s/{name}"), format!("no {kind} {name}"))
                            .placed(MANIFEST, format!("{kind}.{name}")),
                    );
                }
            }
        }
        Ok(wrong)
    }

    /// The workflow `name`, its file read as `v`, with its entry in the
    /// manifest (`entries`, the `workflow` ones): what it is.
    pub fn read_flow(
        &self,
        name: &str,
        v: &Value,
        entries: &Map<String, Value>,
    ) -> Result<crate::flow::Flow> {
        self.flow_with_entry(name, v, entries)
            .map_err(|ps| anyhow!("{}", crate::flow::told(&ps)))
    }

    /// What is wrong with the args of the built-in actions a workflow's
    /// steps run, each at its step's address: refused by `ws check` and the
    /// daemon alike, since both read the workflows through here. A step of
    /// an action the workspace has its own file for is that file's.
    fn built_in_problems(&self, v: &Value) -> Vec<(String, String)> {
        let mut out = vec![];
        for (place, step, args, _) in crate::flow::addressed(v) {
            let crate::flow::Name::Action {
                name,
                version: None,
            } = &step
            else {
                continue;
            };
            use crate::engine::worker::{AI, BROWSER, LOAD};
            // looked up for the built-ins checked here alone
            if ![AI, BROWSER, LOAD].contains(&name.as_str()) || self.action_file(name, None).is_ok()
            {
                continue;
            }
            match name.as_str() {
                LOAD => out.extend(
                    crate::engine::load::problems(&args, true)
                        .into_iter()
                        .map(|m| (place.clone(), format!("#load: {m}"))),
                ),
                BROWSER => {
                    if let Err(e) = crate::browser::step::check(&args) {
                        out.push((place.clone(), format!("#browser: {e}")));
                    }
                }
                AI => {
                    if let Err(e) = crate::ai::step::check(&args) {
                        // the address says the step
                        let e = e.strip_prefix("#ai: ").map(String::from).unwrap_or(e);
                        out.push((place.clone(), e));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn flow_with_entry(
        &self,
        name: &str,
        v: &Value,
        entries: &Map<String, Value>,
    ) -> Result<crate::flow::Flow, Vec<Problem>> {
        let at = format!("workflows/{name}");
        let mut f = crate::flow::read(v)
            .map_err(|ps| ps.into_iter().map(|p| p.under(&at)).collect::<Vec<_>>())?;
        // a built-in workflow says what it is in its own JSON; the entry
        // goes on top
        let mut entry = match v.get("meta") {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        if let Some(Value::Object(e)) = entries.get(name) {
            entry.extend(e.clone());
        }
        f.meta = crate::flow::entry(&Value::Object(entry))
            .map_err(|e| vec![Problem::new(at, e).placed(MANIFEST, format!("workflow.{name}"))])?;
        Ok(f)
    }

    /// Each problem with no file of its own put in its file, at its place
    /// there, from its address: a workflow's, an action's or a connector's
    /// file, the config for `limits` and `folders`; the workspace's own
    /// (`""`) keep none.
    pub fn placed(&self, ps: Vec<Problem>) -> Vec<Problem> {
        let flows = self.flow_files().unwrap_or_default();
        let actions = self.action_files().unwrap_or_default();
        let connectors: BTreeMap<String, PathBuf> = self
            .connector_files()
            .unwrap_or_default()
            .into_iter()
            .collect();
        ps.into_iter()
            .map(|p| {
                if p.file.is_some() {
                    return p;
                }
                let mut parts = p.at.splitn(3, '/');
                let (kind, name, rest) = (parts.next(), parts.next(), parts.next());
                let file = match (kind, name) {
                    (Some("workflows"), Some(n)) => flows.get(n),
                    (Some("actions"), Some(n)) => actions.get(n),
                    (Some("connectors"), Some(n)) => connectors.get(n),
                    (Some("limits" | "folders"), None) => {
                        let place = p.at.clone();
                        return p.placed(MANIFEST, place);
                    }
                    _ => None,
                };
                match file {
                    Some(f) => {
                        let (file, place) = (self.shown(f), rest.unwrap_or("").replace('/', "."));
                        p.placed(file, place)
                    }
                    None => p,
                }
            })
            .collect()
    }

    /// The manifest as written now: read again each time, so an edit
    /// counts from the next pass, without a restart.
    pub fn manifest(&self) -> Result<Map<String, Value>> {
        read_manifest(&self.root.join(MANIFEST))
    }

    /// The manifest's entries of one kind (`workflow`, `action` or
    /// `connector`), by name; each an object.
    pub fn entries(&self, kind: &str) -> Result<Map<String, Value>> {
        let path = self.root.join(MANIFEST);
        match self.manifest()?.remove(kind) {
            None => Ok(Map::new()),
            Some(Value::Object(m)) => m
                .into_iter()
                .map(|(name, e)| match e {
                    // true: on, with its defaults; false: off
                    Value::Bool(true) => Ok((name, Value::Object(Map::new()))),
                    Value::Bool(false) => Ok((name, serde_json::json!({ "off": true }))),
                    Value::Object(_) => Ok((name, e)),
                    _ => bail!(
                        "{}: {kind}.{name} is true, false or an object",
                        path.display()
                    ),
                })
                .collect(),
            Some(_) => bail!(
                "{}: {kind} is an object: {{\"<name>\": {{...}}}}",
                path.display()
            ),
        }
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
    /// whoever asks it (`itero ws info`, the watchdog).
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

    /// Every connector in the simple form: an executable whose file name is
    /// the type of the messages it sends.
    pub fn connector_files(&self) -> Result<Vec<(String, PathBuf)>> {
        // only executables are connectors: notes beside them are left alone
        let files = self.files_in(&self.connectors, true)?;
        for (kind, f) in &files {
            if !crate::flow::is_message_type(kind) || crate::engine::events::engine_own(kind) {
                bail!(
                    "{}: a connector's name is the message type it sends, two or more words joined by . (like mail.received)",
                    f.display()
                );
            }
        }
        Ok(named("connector", files)?.into_iter().collect())
    }
}
