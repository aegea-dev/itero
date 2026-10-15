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

#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    pub port: u16,
    pub workflows: Vec<PathBuf>,
    pub actions: Vec<PathBuf>,
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
    pub fn workflows(&self) -> Result<Vec<(PathBuf, workflow::File)>> {
        let defs: Vec<actions::Definition> = self.actions()?.into_iter().map(|(_, d)| d).collect();
        let mut out: Vec<(PathBuf, workflow::File)> = vec![];
        for dir in &self.workflows {
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
        Ok(out)
    }
}
