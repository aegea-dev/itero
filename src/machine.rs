//! The machine's list of workspaces, which one `itero daemon --all`
//! runs: state the commands write, in itero's state folder.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// itero's state folder, what itero writes: `$XDG_STATE_HOME/itero`,
/// else `~/.local/state/itero`, on every OS.
pub fn state_dir() -> Result<PathBuf> {
    state_dir_from(|k| std::env::var(k).ok())
}

/// itero's config folder, what its user writes: `$XDG_CONFIG_HOME/itero`,
/// else `~/.config/itero`, on every OS (`/etc/itero` with
/// `XDG_CONFIG_HOME=/etc`).
pub fn config_dir() -> Result<PathBuf> {
    config_dir_from(|k| std::env::var(k).ok())
}

fn state_dir_from(get: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    xdg_dir("XDG_STATE_HOME", ".local/state", get)
}

fn config_dir_from(get: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config", get)
}

/// `$<var>/itero`, else `~/<under_home>/itero`: an empty variable is not
/// set, as the XDG Base Directory spec says.
fn xdg_dir(var: &str, under_home: &str, get: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    if let Some(x) = get(var).filter(|x| !x.is_empty()) {
        return Ok(PathBuf::from(x).join("itero"));
    }
    let home = get("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(under_home).join("itero"))
}

/// Writes `text` to `p` whole, readable by its owner only (it holds
/// tokens or their hashes): to a file beside it, made 0600, then renamed
/// over it, so no reader sees half of it.
pub fn write_private(p: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let dir = p.parent().context("a file in a folder")?;
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let tmp = p.with_extension("json.tmp");
    let mut f = private_file(&tmp).with_context(|| tmp.display().to_string())?;
    f.write_all(text.as_bytes())
        .with_context(|| tmp.display().to_string())?;
    drop(f);
    std::fs::rename(&tmp, p).with_context(|| p.display().to_string())
}

/// A new file, truncated, that only its owner may read or write.
#[cfg(unix)]
fn private_file(p: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(p)?;
    // a file already there keeps its mode: make it 0600 too
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

#[cfg(not(unix))]
fn private_file(p: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(p)
}

/// The list's file.
pub fn list_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("workspaces.json"))
}

/// One workspace on the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The workspace's journal id: its identity.
    pub id: String,
    /// Its journal's two words, unique on the machine.
    pub name: String,
    pub folder: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct List {
    pub workspaces: Vec<Entry>,
}

impl List {
    /// The list at `p`; none there is an empty one.
    pub fn read(p: &Path) -> Result<List> {
        match std::fs::read_to_string(p) {
            Ok(t) => serde_json::from_str(&t).with_context(|| p.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(List::default()),
            Err(e) => Err(e).with_context(|| p.display().to_string()),
        }
    }

    /// Writes it whole, to a file beside it renamed over it: a daemon
    /// watching it never reads half a list.
    pub fn write(&self, p: &Path) -> Result<()> {
        let dir = p.parent().context("the list is in a folder")?;
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| tmp.display().to_string())?;
        std::fs::rename(&tmp, p).with_context(|| p.display().to_string())
    }

    pub fn by_id(&self, id: &str) -> Option<&Entry> {
        self.workspaces.iter().find(|e| e.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Entry> {
        self.workspaces.iter().find(|e| e.name == name)
    }

    /// Adds `e`, or replaces the entry with its id (a folder moved).
    pub fn upsert(&mut self, e: Entry) {
        match self.workspaces.iter_mut().find(|w| w.id == e.id) {
            Some(w) => *w = e,
            None => self.workspaces.push(e),
        }
    }

    /// Takes the entry with this id out; whether there was one.
    pub fn remove_id(&mut self, id: &str) -> bool {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.id != id);
        self.workspaces.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, name: &str, folder: &str) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            folder: folder.into(),
        }
    }

    #[test]
    fn a_missing_list_is_empty_and_a_written_one_reads_back() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("durum/workspaces.json");
        assert_eq!(List::read(&p).unwrap().workspaces, vec![]);
        let mut l = List::default();
        l.upsert(e("1", "quiet-river", "/çalışma/ev"));
        l.write(&p).unwrap();
        assert_eq!(List::read(&p).unwrap(), l);
    }

    #[test]
    fn upsert_by_id_moves_a_folder_and_remove_takes_it_out() {
        let mut l = List::default();
        l.upsert(e("1", "quiet-river", "/eski"));
        l.upsert(e("1", "quiet-river", "/yeni"));
        assert_eq!(l.workspaces.len(), 1);
        assert_eq!(
            l.by_name("quiet-river").unwrap().folder,
            PathBuf::from("/yeni")
        );
        assert_eq!(l.by_id("1").unwrap().name, "quiet-river");
        assert!(l.remove_id("1"));
        assert!(!l.remove_id("1"));
    }

    #[test]
    fn the_state_folder_follows_xdg_state_home() {
        let got = state_dir_from(|k| match k {
            "XDG_STATE_HOME" => Some("/durum".into()),
            "HOME" => Some("/ev".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(got, PathBuf::from("/durum/itero"));
        let got = state_dir_from(|k| (k == "HOME").then(|| "/ev".into())).unwrap();
        assert_eq!(got, PathBuf::from("/ev/.local/state/itero"));
    }

    #[test]
    fn the_config_folder_follows_xdg_config_home() {
        let got = config_dir_from(|k| match k {
            "XDG_CONFIG_HOME" => Some("/ayar".into()),
            "HOME" => Some("/ev".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(got, PathBuf::from("/ayar/itero"));
        // empty is not set, as the XDG spec says
        let got = config_dir_from(|k| match k {
            "XDG_CONFIG_HOME" => Some(String::new()),
            "HOME" => Some("/ev".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(got, PathBuf::from("/ev/.config/itero"));
        // a container's /etc/itero, by XDG_CONFIG_HOME=/etc
        let got = config_dir_from(|k| (k == "XDG_CONFIG_HOME").then(|| "/etc".into())).unwrap();
        assert_eq!(got, PathBuf::from("/etc/itero"));
        assert!(config_dir_from(|_| None).is_err());
    }
}
