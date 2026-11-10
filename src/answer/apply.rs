//! A workspace taken by a server: `PUT /workspaces/{name}` with its files
//! (what `itero ws apply` sends). The first makes it, on the machine's
//! list; later ones make its folder match, as an editor's saves would.
use super::Fault;
use crate::machine::{Entry, List, list_path, state_dir};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// What a server keeps of a workspace itself, never sent or touched.
pub const OWN: &[&str] = &[".itero", ".env", ".git"];

/// A workspace's files, as sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Files {
    pub files: Vec<File>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// From the workspace's folder, `/` between its parts.
    pub path: String,
    #[serde(default)]
    pub executable: bool,
    /// Its bytes, base64.
    pub content: String,
}

/// What an apply did, or would do on a dry run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    pub created: bool,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

/// Whether `path` (from a workspace's folder) is one a server keeps of
/// its own: under `.itero/`, `.git/`, or `.env`.
pub fn own(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    OWN.contains(&first)
}

/// Makes the server's workspace `name` hold `files`: on the list, made
/// when it is not there; every file written, changed or removed so its
/// folder holds those files and no others (its own `.itero/`, `.env` and
/// `.git/` aside). `reserved`: the names a workspace cannot have (the
/// `ws` commands'). With `dry_run`, it says what it would do and writes
/// nothing.
pub fn apply(
    name: &str,
    files: &Files,
    dry_run: bool,
    reserved: &[String],
) -> Result<Applied, Fault> {
    let wanted = checked(name, files, reserved)?;
    let list = List::read(&list_path()?)?;
    let listed = list.by_name(name).cloned();
    let folder = match &listed {
        Some(e) => e.folder.clone(),
        None => state_dir()?.join("workspaces").join(name),
    };
    let there = present(&folder)?;
    let mut done = Applied {
        created: listed.is_none(),
        ..Applied::default()
    };
    for (path, (bytes, executable)) in &wanted {
        match there.get(path) {
            None => done.added.push(path.clone()),
            Some((old, x)) if old != bytes || x != executable => done.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    done.removed = there
        .keys()
        .filter(|p| !wanted.contains_key(*p))
        .cloned()
        .collect();
    if dry_run {
        return Ok(done);
    }
    // written in an order that keeps it whole between writes: what steps
    // run first, the workflows that run them next, the manifest last;
    // removed the other way round
    let rank = |p: &str| match p {
        crate::workspace::MANIFEST => 2,
        p if p.starts_with("workflows/") => 1,
        _ => 0,
    };
    let mut writes: Vec<&String> = done.added.iter().chain(&done.changed).collect();
    writes.sort_by_key(|p| (rank(p), p.to_string()));
    for p in writes {
        let (bytes, executable) = &wanted[p];
        write(&folder.join(p), bytes, *executable).map_err(Fault::Internal)?;
    }
    let mut removals: Vec<&String> = done.removed.iter().collect();
    removals.sort_by_key(|p| (std::cmp::Reverse(rank(p)), p.to_string()));
    for p in removals {
        let f = folder.join(p);
        std::fs::remove_file(&f)
            .map_err(|e| Fault::Internal(anyhow::anyhow!("{}: {e}", f.display())))?;
    }
    if listed.is_none() {
        list_it(name, &folder)?;
    }
    Ok(done)
}

/// Every file of `files`, its bytes and whether it is executable, after
/// checking them and `name`: every problem told at once.
fn checked(
    name: &str,
    files: &Files,
    reserved: &[String],
) -> Result<BTreeMap<String, (Vec<u8>, bool)>, Fault> {
    let mut problems = vec![];
    if !crate::names::plain(name) {
        problems.push(format!("{name}: {}", crate::names::PLAIN));
    }
    if reserved.iter().any(|r| r == name) {
        problems.push(format!(
            "{name}: a ws command's name, so `itero ws {name} ...` could not name it"
        ));
    }
    let mut wanted = BTreeMap::new();
    for f in &files.files {
        let p = &f.path;
        let inside = !p.is_empty()
            && Path::new(p)
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !inside {
            problems.push(format!(
                "{p}: a path inside the workspace, below its folder"
            ));
            continue;
        }
        if own(p) {
            problems.push(format!("{p}: the server's own, never sent"));
            continue;
        }
        match base64::engine::general_purpose::STANDARD.decode(&f.content) {
            Ok(bytes) => {
                if wanted.insert(p.clone(), (bytes, f.executable)).is_some() {
                    problems.push(format!("{p}: sent twice"));
                }
            }
            Err(e) => problems.push(format!("{p}: its content is not base64: {e}")),
        }
    }
    if !wanted.contains_key(crate::workspace::MANIFEST) {
        problems.push(format!(
            "{}: not among the files, and a workspace is a folder with one",
            crate::workspace::MANIFEST
        ));
    }
    match problems.is_empty() {
        true => Ok(wanted),
        false => Err(Fault::Bad(problems.join("\n"))),
    }
}

/// The files in the server's workspace folder now, as `checked` gives
/// them: none for a folder not there yet.
fn present(folder: &Path) -> Result<BTreeMap<String, (Vec<u8>, bool)>, Fault> {
    let mut out = BTreeMap::new();
    if !folder.exists() {
        return Ok(out);
    }
    let mut todo = vec![folder.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| Fault::Internal(anyhow::anyhow!("{}: {e}", dir.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| Fault::Internal(e.into()))?;
            let path = entry.path();
            let rel = path
                .strip_prefix(folder)
                .map_err(|e| Fault::Internal(e.into()))?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if own(&rel) {
                continue;
            }
            let kind = entry.file_type().map_err(|e| Fault::Internal(e.into()))?;
            if kind.is_dir() {
                todo.push(path);
            } else if kind.is_symlink() {
                // never followed: the apply replaces or removes it
                out.insert(rel, (vec![], false));
            } else {
                let bytes = std::fs::read(&path)
                    .map_err(|e| Fault::Internal(anyhow::anyhow!("{}: {e}", path.display())))?;
                out.insert(rel, (bytes, executable(&path)));
            }
        }
    }
    Ok(out)
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

/// Writes `bytes` to `p` whole: to a file beside it, then renamed over
/// it, executable or not.
fn write(p: &Path, bytes: &[u8], executable: bool) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let dir = p.parent().context("a file in a folder")?;
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let tmp = PathBuf::from(format!("{}.itero-apply", p.display()));
    std::fs::write(&tmp, bytes).with_context(|| tmp.display().to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))
            .with_context(|| tmp.display().to_string())?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    std::fs::rename(&tmp, p).with_context(|| p.display().to_string())
}

/// Puts the workspace just made in `folder` on the list as `name`, its
/// journal named so: the machine's daemon starts it.
fn list_it(name: &str, folder: &Path) -> Result<(), Fault> {
    let ws = crate::workspace::find(folder)?;
    let j = crate::engine::journal::Journal::open(&ws)?;
    j.rename_workspace(name)?;
    let me = j.workspace()?;
    let path = list_path()?;
    let mut list = List::read(&path)?;
    list.upsert(Entry {
        id: me.id,
        name: name.to_string(),
        folder: folder.to_path_buf(),
    });
    list.write(&path)?;
    Ok(())
}
