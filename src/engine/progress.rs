//! What a running step said last: its latest line on stderr, kept in
//! `.itero/progress/<job>.json` while it runs, for `itero wf runs` and the
//! HTTP API to show. A file, not the journal: the journal is the daemon's
//! between passes, and a step's progress changes while a pass waits for it.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn dir(root: &Path) -> PathBuf {
    root.join(".itero").join("progress")
}

/// Keeps what step `step` of run `run` (job `job`) said last, at `at`.
pub fn write(
    root: &Path,
    job: i64,
    run: &str,
    step: &str,
    said: &str,
    at: &str,
) -> std::io::Result<()> {
    let d = dir(root);
    std::fs::create_dir_all(&d)?;
    let v = json!({ "run": run, "step": step, "said": said, "at": at });
    // written whole, then moved: a reader never sees half of it
    let tmp = d.join(format!(".{job}.json"));
    std::fs::write(&tmp, v.to_string())?;
    std::fs::rename(tmp, d.join(format!("{job}.json")))
}

/// Forgets job `job`'s progress: its step ended, or was lost.
pub fn clear(root: &Path, job: i64) {
    let _ = std::fs::remove_file(dir(root).join(format!("{job}.json")));
}

/// What the running steps of run `run` said last, newest first, each
/// `{"step", "said", "at"}`.
pub fn of_run(root: &Path, run: &str) -> Vec<Value> {
    let Ok(files) = std::fs::read_dir(dir(root)) else {
        return vec![];
    };
    let mut out: Vec<Value> = files
        .flatten()
        .filter(|f| !f.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|f| serde_json::from_slice::<Value>(&std::fs::read(f.path()).ok()?).ok())
        .filter(|v| v["run"] == run)
        .map(|v| json!({ "step": v["step"], "said": v["said"], "at": v["at"] }))
        .collect();
    out.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
    out
}
