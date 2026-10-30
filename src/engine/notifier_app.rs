//! Itero.app's stable copy, which every notification on macOS comes from.
//! macOS keeps one record of an app's notification permission, following
//! one copy of it; when that copy is deleted (an upgrade deletes the old
//! version's), every copy is refused. So itero keeps its own copy in one
//! place no upgrade deletes, and brings it up to date in place from the
//! Itero.app it ships with: the folder stays, and so does the record.
use super::lock::{Refused, take_file};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Where itero keeps its own Itero.app.
pub fn stable(home: &Path) -> PathBuf {
    home.join("Library/Application Support/itero/Itero.app")
}

/// An Itero.app's `CFBundleVersion`, a counter.
pub fn version(app: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    crate::plist::string(&text, "CFBundleVersion")?
        .trim()
        .parse()
        .ok()
}

/// The notifier inside an Itero.app.
pub fn program(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/itero-notifier")
}

/// Brings `stable` up to `shipped`: when it is missing or older,
/// `shipped`'s Contents replace its own (a new Contents made beside it,
/// renamed into place), the .app folder itself kept. Under the lock
/// `lock`, so steps side by side copy once. Gives the notifier to run.
pub fn up_to_date(shipped: &Path, stable: &Path, lock: &Path) -> Result<PathBuf, String> {
    let want = version(shipped).ok_or_else(|| {
        format!(
            "{}: no CFBundleVersion in its Contents/Info.plist",
            shipped.display()
        )
    })?;
    let _held = wait_for(lock)?;
    let at = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let (new, old) = (stable.join("Contents.new"), stable.join("Contents.old"));
    for leftover in [&new, &old] {
        if leftover.exists() {
            std::fs::remove_dir_all(leftover).map_err(|e| at(leftover, e))?;
        }
    }
    if version(stable).is_some_and(|have| have >= want) && program(stable).is_file() {
        return Ok(program(stable));
    }
    std::fs::create_dir_all(stable).map_err(|e| at(stable, e))?;
    copy_tree(&shipped.join("Contents"), &new)?;
    let contents = stable.join("Contents");
    if contents.exists() {
        std::fs::rename(&contents, &old).map_err(|e| at(&contents, e))?;
    }
    std::fs::rename(&new, &contents).map_err(|e| at(&new, e))?;
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| at(&old, e))?;
    }
    Ok(program(stable))
}

/// The lock, waited for while another step copies (a few seconds at most).
fn wait_for(lock: &Path) -> Result<super::lock::DaemonLock, String> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match take_file(lock, &format!("pid {}", std::process::id())) {
            Ok(l) => return Ok(l),
            Err(Refused::Failed(why)) => return Err(why),
            Err(Refused::Held(by)) if Instant::now() > end => {
                return Err(format!("{}: held by {by} for five seconds", lock.display()));
            }
            Err(Refused::Held(_)) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// Copies `from` to `to`, files with their permissions, links as links.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let at = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(to).map_err(|e| at(to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| at(from, e))? {
        let entry = entry.map_err(|e| at(from, e))?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| at(&src, e))?;
        if kind.is_symlink() {
            let target = std::fs::read_link(&src).map_err(|e| at(&src, e))?;
            std::os::unix::fs::symlink(target, &dst).map_err(|e| at(&dst, e))?;
        } else if kind.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| at(&src, e))?;
        }
    }
    Ok(())
}
