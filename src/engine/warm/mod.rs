//! Warm actions: a step with `"warm": true` runs its Python action in a
//! copy kept loaded between steps (runner.rs, pool.rs). Here: which Python
//! runs it, what tells a changed file, how long and how many copies.
pub mod pool;
pub mod runner;

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The Python a warm action runs in, from its first line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Python {
    /// `python3`, `python`, `python3.N`: found on the PATH, or a path.
    Plain(String),
    /// `uv run --script`: the script's own environment.
    UvScript,
    /// `uv run --project <folder> python`: the project's environment.
    UvProject(PathBuf),
}

/// Which Python `first_line` runs, or why the action cannot be warm.
pub fn python(first_line: &str) -> Result<Python, String> {
    let runs = crate::needs::interpreter(first_line).unwrap_or_default();
    let name = Path::new(&runs)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let is_python = name == "python"
        || name == "python3"
        || name
            .strip_prefix("python3.")
            .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()));
    if is_python {
        return Ok(Python::Plain(runs));
    }
    if name == "uv" {
        let words: Vec<&str> = first_line.split_whitespace().collect();
        if words.contains(&"--script") {
            return Ok(Python::UvScript);
        }
        if let Some(i) = words.iter().position(|w| *w == "--project")
            && let Some(folder) = words.get(i + 1)
            && words.get(i + 2) == Some(&"python")
        {
            return Ok(Python::UvProject(PathBuf::from(folder)));
        }
    }
    let runs = if runs.is_empty() {
        "nothing".to_string()
    } else {
        name.to_string()
    };
    Err(format!(
        "warm: only Python actions can be warm: its first line runs {runs}"
    ))
}

/// The interpreter a copy starts: for uv, after `uv sync`, since without it
/// uv answers another Python (its find falls back silently).
pub fn interpreter(p: &Python, file: &Path, root: &Path) -> Result<PathBuf, String> {
    let uv = |args: &[&std::ffi::OsStr]| -> Result<String, String> {
        let out = std::process::Command::new("uv")
            .args(args)
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("warm: cannot run uv: {e}"))?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "warm: uv {}: {}",
                args[0].to_string_lossy(),
                why.trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    match p {
        Python::Plain(name) => Ok(PathBuf::from(name)),
        Python::UvScript => {
            uv(&[
                "sync".as_ref(),
                "--script".as_ref(),
                file.as_os_str(),
                "--quiet".as_ref(),
            ])?;
            Ok(PathBuf::from(uv(&[
                "python".as_ref(),
                "find".as_ref(),
                "--script".as_ref(),
                file.as_os_str(),
            ])?))
        }
        Python::UvProject(folder) => {
            uv(&[
                "sync".as_ref(),
                "--project".as_ref(),
                folder.as_os_str(),
                "--quiet".as_ref(),
            ])?;
            // find --project answers the base Python, not the project's
            Ok(root.join(folder).join(".venv/bin/python"))
        }
    }
}

/// What tells a file changed: its device, inode, size and modified time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    dev: u64,
    ino: u64,
    len: u64,
    modified_ns: i128,
}

pub fn identity(file: &Path) -> std::io::Result<Identity> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(file)?;
    Ok(Identity {
        dev: m.dev(),
        ino: m.ino(),
        len: m.len(),
        modified_ns: i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec()),
    })
}

/// How long a copy waits for a request before it stops.
pub fn idle() -> Duration {
    std::env::var("ITERO_TEST_WARM_IDLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(10 * 60))
}

/// The most copies of one action: the CPUs, never more than the steps.
pub fn most_copies(steps: usize) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    cpus.min(steps).max(1)
}
