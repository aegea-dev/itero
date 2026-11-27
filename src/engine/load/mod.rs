//! The built-in `#load`: the machine and itero, measured at once. Each
//! part is a read of the OS, the journal or the files, with no waiting,
//! and only the parts asked for (`measure`) are read.
pub mod record;

use super::worker::{Failure, Outcome, StepRun};
use serde_json::{Map, Value, json};
use std::path::Path;

/// The parts `#load` measures, in the order its output has them.
pub const PARTS: [&str; 8] = [
    "os", "cpu", "memory", "disk", "daemon", "queue", "runs", "journal",
];

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
            "daemon" => daemon(ws.machine),
            "queue" => queue(&ws),
            "runs" => runs(&ws),
            "journal" => journal(&ws),
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

/// Memory: what is used, and the rest free, so the two make the total on
/// every system. sysinfo's own "available" cannot be the rest: on macOS it
/// counts the pages apps are using too (active + inactive + free), so it
/// and "used" (apps, wired and compressed, as Activity Monitor has it) add
/// up to more than there is. On Linux, used is total minus available.
fn memory() -> Result<Value, String> {
    let mut s = sysinfo::System::new().map_err(|e| format!("memory: {e}"))?;
    s.refresh_memory_specifics(sysinfo::MemoryRefreshKind::everything());
    let (total, used) = (s.total_memory(), s.used_memory());
    Ok(json!({
        "total": total,
        "used": used,
        "free": total.saturating_sub(used),
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

/// The daemon: this worker's parent. Its own CPU time, memory and open
/// files; the steps running (its `itero worker` children, this one apart),
/// the warm copies (`itero worker --warm`), and the memory of both and of
/// everything they started.
fn daemon(machine: bool) -> Result<Value, String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    // SAFETY: getppid only reads this process's parent's id
    let pid = Pid::from_u32(unsafe { libc::getppid() } as u32);
    let me = Pid::from_u32(std::process::id());
    let mut s = sysinfo::System::new().map_err(|e| format!("daemon: {e}"))?;
    let children: std::collections::HashMap<Pid, Vec<Pid>> = tree(pid.as_u32())
        .map_err(|e| format!("daemon.workers: {e}"))?
        .into_iter()
        .map(|(p, c)| (Pid::from_u32(p), c.into_iter().map(Pid::from_u32).collect()))
        .collect();
    // a process and everything under it
    let below = |from: Pid| {
        let mut all = vec![from];
        let mut i = 0;
        while i < all.len() {
            all.extend(children.get(&all[i]).into_iter().flatten().copied());
            i += 1;
        }
        all
    };
    let mut asked = below(pid);
    asked.retain(|p| *p != me);
    s.refresh_processes_specifics(
        ProcessesToUpdate::Some(&asked),
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );
    let d = s.process(pid).ok_or("daemon: its process is gone")?;
    let files = d.open_files().ok_or("daemon.files: cannot be read")?;
    let is_worker = |p: &sysinfo::Process| p.cmd().get(1).is_some_and(|a| a == "worker");
    let is_warm = |p: &sysinfo::Process| p.cmd().iter().any(|a| a == "--warm");
    let (mut workers, mut warm, mut memory) = (0u64, 0u64, 0u64);
    for c in children
        .get(&pid)
        .into_iter()
        .flatten()
        .filter(|c| **c != me)
    {
        let Some(p) = s.process(*c).filter(|p| is_worker(p)) else {
            continue;
        };
        if is_warm(p) {
            warm += 1
        } else {
            workers += 1
        }
        memory += below(*c)
            .iter()
            .filter_map(|x| s.process(*x))
            .map(|x| x.memory())
            .sum::<u64>();
    }
    Ok(json!({
        "pid": pid.as_u32(),
        "cpu_ms": d.accumulated_cpu_time(),
        "memory": d.memory(),
        "files": files,
        "workers": workers,
        "workers_memory": memory,
        "warm": warm,
        "machine": machine,
    }))
}

/// The processes under `pid`, each with its children, down to the last:
/// asked of the kernel one process at a time, from `pid` down, never by
/// listing every process on the machine (5 ms with 700 of them). A process
/// started or ended while it is read may be missed, as by any listing.
pub fn tree(pid: u32) -> Result<std::collections::HashMap<u32, Vec<u32>>, String> {
    // without the lists, every process would seem to have no children
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        let own = format!("/proc/{me}/task/{me}/children");
        if let Err(e) = std::fs::metadata(&own) {
            return Err(format!(
                "{own}: {e}: this kernel keeps no list of a process's children (CONFIG_PROC_CHILDREN)"
            ));
        }
    }
    let mut tree = std::collections::HashMap::new();
    let mut todo = vec![pid];
    while let Some(p) = todo.pop() {
        let c = children(p)?;
        todo.extend(&c);
        tree.insert(p, c);
    }
    Ok(tree)
}

/// A process's children, from the kernel's list of them.
#[cfg(target_os = "macos")]
fn children(pid: u32) -> Result<Vec<u32>, String> {
    let mut room = 64;
    loop {
        let mut ids: Vec<libc::pid_t> = vec![0; room];
        let bytes = (room * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: the kernel writes at most `bytes` into `ids`, and says how
        // many ids it wrote
        let n =
            unsafe { libc::proc_listchildpids(pid as libc::pid_t, ids.as_mut_ptr().cast(), bytes) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            return Err(format!("the children of {pid}: {e}"));
        }
        let n = n as usize;
        // a full list may have had more: ask again with more room
        if n < room {
            return Ok(ids[..n].iter().map(|p| *p as u32).collect());
        }
        room *= 2;
    }
}

/// A process's children, from the kernel's list of them: one for each of
/// its threads (`/proc/<pid>/task/<thread>/children`), since a child is
/// listed under the thread that started it.
#[cfg(target_os = "linux")]
fn children(pid: u32) -> Result<Vec<u32>, String> {
    use std::io::ErrorKind::NotFound;
    let tasks = format!("/proc/{pid}/task");
    let threads = match std::fs::read_dir(&tasks) {
        Ok(t) => t,
        // it ended: nothing under it
        Err(e) if e.kind() == NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("{tasks}: {e}")),
    };
    let mut all = vec![];
    for t in threads {
        let file = t
            .map_err(|e| format!("{tasks}: {e}"))?
            .path()
            .join("children");
        let list = match std::fs::read_to_string(&file) {
            Ok(l) => l,
            // the thread or its process ended as it was read: a thread's
            // children go to another one
            Err(e) if e.kind() == NotFound || e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(e) => return Err(format!("{}: {e}", file.display())),
        };
        for id in list.split_whitespace() {
            all.push(
                id.parse()
                    .map_err(|e| format!("{}: {id:?}: {e}", file.display()))?,
            );
        }
    }
    Ok(all)
}

fn queue(ws: &crate::workspace::Workspace) -> Result<Value, String> {
    let j = super::journal::Journal::open(ws).map_err(|e| format!("queue: {e:#}"))?;
    let (events, running, waiting) = j.queue().map_err(|e| format!("queue: {e:#}"))?;
    Ok(json!({ "events": events, "running": running, "waiting": waiting }))
}

/// The runs started in the last minute, and how long each waited from its
/// event's saving to its start, as `make bench` measures it: the time in
/// each one's UUIDv7.
fn runs(ws: &crate::workspace::Workspace) -> Result<Value, String> {
    let j = super::journal::Journal::open(ws).map_err(|e| format!("runs: {e:#}"))?;
    let since = (chrono::Utc::now().timestamp_millis() as u64).saturating_sub(60_000);
    // the smallest UUIDv7 of that millisecond
    let first = format!(
        "{:08x}-{:04x}-7000-8000-000000000000",
        since >> 16,
        since & 0xffff
    );
    let started = j
        .started_since(&first)
        .map_err(|e| format!("runs: {e:#}"))?;
    let ms = |id: &str| {
        uuid::Uuid::parse_str(id)
            .ok()
            .and_then(|u| u.get_timestamp())
            .map(|t| {
                let (s, n) = t.to_unix();
                s * 1000 + u64::from(n) / 1_000_000
            })
    };
    let mut waits: Vec<u64> = started
        .iter()
        .filter_map(|(run, event)| Some(ms(run)?.saturating_sub(ms(event.as_deref()?)?)))
        .collect();
    waits.sort_unstable();
    // nearest rank; none in the minute: none to tell
    let at = |p: f64| -> Value {
        match waits.len() {
            0 => Value::Null,
            n => waits[((p * n as f64).ceil() as usize).clamp(1, n) - 1].into(),
        }
    };
    Ok(json!({
        "last_minute": started.len(),
        "pick_up_ms": { "p50": at(0.5), "p95": at(0.95) },
    }))
}

/// The journal's file and its write-ahead log, and all of `.itero/`.
fn journal(ws: &crate::workspace::Workspace) -> Result<Value, String> {
    let file = ws.journal();
    let size = |p: &Path| match std::fs::metadata(p) {
        Ok(m) => Ok(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(format!("journal: {}: {e}", p.display())),
    };
    let mut wal = file.clone().into_os_string();
    wal.push("-wal");
    Ok(json!({
        "size": size(&file)?,
        "wal": size(Path::new(&wal))?,
        "itero": ws.used(),
    }))
}
