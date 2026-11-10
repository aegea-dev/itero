//! `itero ws apply`: this folder's workspace sent to a context's server,
//! every file but this machine's own, the server's copy made to match.
use super::out::show;
use crate::answer::apply::{File, Files, own};
use crate::answer::remote::Remote;
use anyhow::{Result, bail};
use base64::Engine as _;
use std::path::Path;

/// The most a workspace may send: more is a mistake to say, not to send.
const LIMIT: u64 = 64 * 1024 * 1024;

/// Every file under `root` but this machine's own (`.itero/`, `.env`,
/// `.git/`): its path from `root`, whether it is executable, its bytes.
/// A symlink (it points at this machine) or more than `LIMIT` in all is
/// refused, every problem at once.
pub fn gather(root: &Path) -> Result<Files> {
    let mut files = vec![];
    let mut sizes: Vec<(u64, String)> = vec![];
    let mut problems = vec![];
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?
            .collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if own(&rel) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                problems.push(format!(
                    "{rel}: a symlink, which points at this machine: put the file itself there"
                ));
            } else if kind.is_dir() {
                todo.push(path);
            } else {
                let bytes =
                    std::fs::read(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
                sizes.push((bytes.len() as u64, rel.clone()));
                files.push(File {
                    path: rel,
                    executable: executable(&path),
                    content: base64::engine::general_purpose::STANDARD.encode(&bytes),
                });
            }
        }
    }
    let total: u64 = sizes.iter().map(|(n, _)| n).sum();
    if total > LIMIT {
        sizes.sort_by_key(|s| std::cmp::Reverse(s.0));
        let largest: Vec<String> = sizes
            .iter()
            .take(3)
            .map(|(n, p)| format!("{p} ({} MiB)", n / (1024 * 1024)))
            .collect();
        problems.push(format!(
            "{} MiB in all, more than the {} MiB a workspace may send; the largest: {}",
            total / (1024 * 1024),
            LIMIT / (1024 * 1024),
            largest.join(", ")
        ));
    }
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    Ok(Files { files })
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

/// `itero ws apply`: the workspace at `root` sent to `r`'s server, and
/// what it did there (with `dry_run`, what it would do).
pub fn run(r: &Remote, root: &Path, dry_run: bool, json: bool) -> Result<()> {
    let files = gather(root)?;
    let done = crate::answer::remote::apply(r, &files, dry_run)?;
    let (ws, on) = (&r.ws, &r.name);
    show(json, &serde_json::to_value(&done)?, |_| {
        if done.created {
            println!("created {ws} on {on}");
        }
        for (mark, paths) in [
            ("+", &done.added),
            ("~", &done.changed),
            ("-", &done.removed),
        ] {
            for p in paths {
                println!("{mark} {p}");
            }
        }
        let nothing = done.added.is_empty() && done.changed.is_empty() && done.removed.is_empty();
        if nothing && !done.created {
            println!("{ws} on {on} is up to date");
        }
        if dry_run {
            println!("dry run: nothing written");
        }
    })
}
