//! The machine's list of workspaces, which one `itero daemon --all`
//! runs: state the commands write, in itero's state folder.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// itero's state folder: `$XDG_STATE_HOME/itero`, else
/// `~/.local/state/itero`, on every OS.
pub fn state_dir() -> Result<PathBuf> {
    state_dir_from(|k| std::env::var(k).ok())
}

fn state_dir_from(get: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    if let Some(x) = get("XDG_STATE_HOME").filter(|x| !x.is_empty()) {
        return Ok(PathBuf::from(x).join("itero"));
    }
    let home = get("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/state/itero"))
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
}
