//! The built-in `#record-load`: each `load` result a line of the day's
//! file, `.itero/load/<UTC date>.jsonl`. A past day's file is gzipped,
//! and a file whose day ended more than `limits.history` ago deleted.
use crate::engine::worker::{Failure, Outcome, StepRun};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

/// Runs `#record-load`: its arg `load` appended to the day's file, then
/// the folder rolled. Its output names the file.
pub fn run(r: &StepRun) -> Result<Outcome, Failure> {
    let failed = |e: String| Failure::Error(format!("#record-load: {e}"));
    let load = match r.input.get("args").and_then(|a| a.get("load")) {
        Some(v @ Value::Object(_)) => v,
        Some(other) => {
            return Err(failed(format!(
                "load is a #load result, an object, not {other}"
            )));
        }
        None => return Err(failed("load is needed: the #load result to keep".into())),
    };
    let ws = crate::workspace::find(&r.root).map_err(|e| failed(format!("{e:#}")))?;
    let now: DateTime<Utc> = r
        .now
        .parse()
        .map_err(|_| failed(format!("now is not a time: {}", r.now)))?;
    let dir = ws.root.join(".itero").join("load");
    std::fs::create_dir_all(&dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
    let name = format!("{}.jsonl", now.format("%Y-%m-%d"));
    let file = dir.join(&name);
    // one write of the whole line: appends never interleave
    let line = format!("{load}\n");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .map_err(|e| failed(format!("{}: {e}", file.display())))?;
    roll(&dir, now, ws.limits.history).map_err(failed)?;
    Ok(Outcome::Done(
        json!({ "file": format!(".itero/load/{name}") }),
    ))
}

/// Each file of a day before `now`'s gzipped, and each whose day ended
/// more than `history` ago deleted. Files of other names are left alone.
fn roll(dir: &Path, now: DateTime<Utc>, history: std::time::Duration) -> Result<(), String> {
    let history = chrono::Duration::from_std(history).unwrap_or(chrono::Duration::MAX);
    let today = now.date_naive();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries {
        let path = e.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let (day, gzipped) = match (name.strip_suffix(".jsonl.gz"), name.strip_suffix(".jsonl")) {
            (Some(d), _) => (d, true),
            (None, Some(d)) => (d, false),
            (None, None) => continue,
        };
        let Ok(day) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
            continue;
        };
        let ended = day
            .succ_opt()
            .unwrap_or(day)
            .and_time(chrono::NaiveTime::MIN)
            .and_utc();
        if now - ended > history {
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        } else if !gzipped && day < today {
            gzip(&path)?;
        }
    }
    Ok(())
}

/// `<day>.jsonl` to `<day>.jsonl.gz`: written aside, renamed, then the
/// plain file removed, so a crash leaves one whole. A `.part` left by a
/// crash is written over the next time.
fn gzip(path: &Path) -> Result<(), String> {
    let said = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut gz = path.as_os_str().to_owned();
    gz.push(".gz");
    let mut part = gz.clone();
    part.push(".part");
    let mut from = std::fs::File::open(path).map_err(said)?;
    let to = std::fs::File::create(&part).map_err(said)?;
    let mut enc = flate2::write::GzEncoder::new(to, flate2::Compression::default());
    std::io::copy(&mut from, &mut enc).map_err(said)?;
    enc.finish().and_then(|f| f.sync_all()).map_err(said)?;
    std::fs::rename(&part, &gz).map_err(said)?;
    std::fs::remove_file(path).map_err(said)
}
