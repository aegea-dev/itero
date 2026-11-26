//! The built-in `#load`: the machine and itero, measured at once. Each
//! part is a read of the OS, the journal or the files, with no waiting,
//! and only the parts asked for (`measure`) are read.
use super::worker::{Failure, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::path::Path;

/// The parts `#load` measures, in the order its output has them.
pub const PARTS: [&str; 4] = ["os", "cpu", "memory", "disk"];

/// What is wrong with `#load`'s args, every problem: `measure` is a list
/// of parts. `written`: the args as the workflow has them, where a
/// `"$..."` value is filled only when the step runs, and checked then.
pub fn problems(args: &Value, written: bool) -> Vec<String> {
    let every = PARTS.join(", ");
    match args.get("measure") {
        None => vec![],
        Some(Value::String(s)) if written && s.starts_with('$') => vec![],
        Some(Value::Array(items)) if items.is_empty() => {
            vec![format!("measure: at least one part ({every})")]
        }
        Some(Value::Array(items)) => items.iter().filter_map(wrong_part).collect(),
        Some(other) => vec![format!("measure: a list of parts ({every}), not {other}")],
    }
}

fn wrong_part(item: &Value) -> Option<String> {
    let Value::String(s) = item else {
        return Some(format!("measure: a part is text, not {item}"));
    };
    if PARTS.contains(&s.as_str()) {
        return None;
    }
    let known: Vec<String> = PARTS.iter().map(|p| p.to_string()).collect();
    Some(match crate::manifest::nearest(s, &known) {
        Some(near) => format!(r#"measure: "{s}" is not a part; did you mean "{near}"?"#),
        None => format!(r#"measure: "{s}" is not a part: {}"#, PARTS.join(", ")),
    })
}

/// Runs `#load`: `at`, each part asked for, then `took`, what it cost.
pub fn run(r: &StepRun) -> Result<Outcome, Failure> {
    let failed = |e: String| Failure::Error(format!("#load: {e}"));
    let began = std::time::Instant::now();
    let cpu_before = own_cpu_ms();
    let args = r
        .input
        .get("args")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let wrong = problems(&args, false);
    if !wrong.is_empty() {
        return Err(failed(wrong.join("; ")));
    }
    let asked: Vec<&str> = match args.get("measure").and_then(Value::as_array) {
        Some(items) => PARTS
            .iter()
            .copied()
            .filter(|p| items.iter().any(|i| i == p))
            .collect(),
        None => PARTS.to_vec(),
    };
    let ws = crate::workspace::find(&r.root).map_err(|e| failed(format!("{e:#}")))?;
    let mut out = Map::new();
    out.insert("at".into(), super::daemon::stamp(chrono::Utc::now()).into());
    for part in asked {
        let v = match part {
            "os" => os(),
            "cpu" => cpu(),
            "memory" => memory(),
            "disk" => disk(&ws.root),
            _ => unreachable!("every part is measured: {part}"),
        }
        .map_err(failed)?;
        out.insert(part.into(), v);
    }
    out.insert(
        "took".into(),
        json!({
            "ms": began.elapsed().as_millis() as u64,
            "cpu_ms": own_cpu_ms().saturating_sub(cpu_before),
        }),
    );
    Ok(Outcome::Done(Value::Object(out)))
}

/// The CPU time this process has had, user and system, in milliseconds.
fn own_cpu_ms() -> u64 {
    // SAFETY: getrusage only writes the struct it is given
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let ms = |t: libc::timeval| t.tv_sec as u64 * 1000 + t.tv_usec as u64 / 1000;
    ms(u.ru_utime) + ms(u.ru_stime)
}

fn os() -> Result<Value, String> {
    Ok(json!({
        "name": std::env::consts::OS,
        "version": sysinfo::System::os_version().map_err(|e| format!("os.version: {e}"))?,
        "arch": std::env::consts::ARCH,
        "host": sysinfo::System::host_name().map_err(|e| format!("os.host: {e}"))?,
    }))
}

fn cpu() -> Result<Value, String> {
    let count = std::thread::available_parallelism()
        .map_err(|e| format!("cpu.count: {e}"))?
        .get();
    let l = sysinfo::System::load_average().map_err(|e| format!("cpu.load: {e}"))?;
    Ok(json!({ "count": count, "load": [l.one, l.five, l.fifteen] }))
}

fn memory() -> Result<Value, String> {
    let mut s = sysinfo::System::new().map_err(|e| format!("memory: {e}"))?;
    s.refresh_memory_specifics(sysinfo::MemoryRefreshKind::everything());
    Ok(json!({
        "total": s.total_memory(),
        "used": s.used_memory(),
        "free": s.available_memory(),
        "swap_used": s.used_swap(),
    }))
}

/// The disk the workspace is on: its size, and what a user may still write.
#[allow(clippy::unnecessary_cast)] // the fields are u32 on macOS, u64 on Linux
fn disk(root: &Path) -> Result<Value, String> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(root.as_os_str().as_bytes())
        .map_err(|e| format!("disk: {}: {e}", root.display()))?;
    // SAFETY: path ends in NUL; statvfs only writes the struct it is given
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 {
        let e = std::io::Error::last_os_error();
        return Err(format!("disk: {}: {e}", root.display()));
    }
    let block = s.f_frsize as u64;
    Ok(json!({ "total": s.f_blocks as u64 * block, "free": s.f_bavail as u64 * block }))
}
