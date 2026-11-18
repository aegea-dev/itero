//! The daemon's warm copies: for each action file, the copies kept between
//! passes (each one `itero worker --warm`, in its own process group), and a
//! step handed to an idle one with its job's lock. A request ends as a
//! fresh worker's run would; what follows it is the pass's, unchanged.
use super::super::daemon::{CANCEL_CHECK, Pipes, READERS, overdue, stop};
use super::super::files::Share;
use super::super::worker::{Failure, LOST_EXIT, Outcome, StepRun};
use super::{Identity, WarmAnswer};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// One warm copy: its worker, the socket its steps go over, and the open
/// files it holds for its whole life.
pub struct Copy {
    child: Child,
    ours: std::os::unix::net::UnixStream,
    out: Option<BufReader<std::io::PipeReader>>,
    /// The end of what the worker said on stderr: why, when it fails.
    err: Arc<Mutex<Vec<u8>>>,
    pub file: PathBuf,
    pub id: Identity,
    idle_since: Instant,
    /// Its worker was reaped: its group's id may be another's by now.
    reaped: bool,
    _share: Share,
}

impl Copy {
    /// The copy's worker's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Copy {
    /// Its group killed: the worker, its runner and anything they left.
    fn drop(&mut self) {
        if !self.reaped {
            stop(&mut self.child);
            gone(self.child.id());
        }
    }
}

/// Waits, `READERS` at most, until nothing is left in process group `group`:
/// a job lost runs again only once nothing of its last run is left.
fn gone(group: u32) {
    let end = Instant::now() + READERS;
    // SAFETY: kill with signal 0 only asks whether a process of the group
    // is there
    while unsafe { libc::kill(-(group as i32), 0) } == 0 && Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Starts a copy for `file`: `itero worker --warm` in its own process group,
/// with the daemon's own folder first on its PATH, as a worker is started.
pub(crate) fn start(
    exe: &Path,
    file: &Path,
    id: Identity,
    share: Share,
    pipes: Pipes,
) -> std::io::Result<Copy> {
    let Pipes {
        ours,
        theirs,
        out: (stdout, out_end),
        err: (stderr, err_end),
    } = pipes;
    let mut c = Command::new(exe);
    c.args(["worker", "--warm"])
        .env("PATH", crate::connectors::supervise::path_with(exe))
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
        .stdout(Stdio::from(out_end))
        .stderr(Stdio::from(err_end));
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let child = c.spawn()?;
    drop(c);
    let err = Arc::new(Mutex::new(Vec::new()));
    {
        // the worker's own stderr, for its whole life: its last lines kept
        let err = err.clone();
        let mut stderr = stderr;
        std::thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut e = err.lock().unwrap_or_else(PoisonError::into_inner);
                e.extend_from_slice(&buf[..n]);
                let over = e.len().saturating_sub(4096);
                e.drain(..over);
            }
        });
    }
    Ok(Copy {
        child,
        ours,
        out: Some(BufReader::new(stdout)),
        err,
        file: file.to_path_buf(),
        id,
        idle_since: Instant::now(),
        reaped: false,
        _share: share,
    })
}

/// How a request on a copy ended: its outcome, whether the copy may take
/// another, and why not.
pub struct Ended {
    pub outcome: Result<Outcome, Failure>,
    pub keep: bool,
    pub why: Option<String>,
}

/// What the copy's worker printed for a request: its `done` line, or
/// nothing (it ended before one).
type Heard = (
    Option<serde_json::Value>,
    Option<BufReader<std::io::PipeReader>>,
);

/// Hands `step` and its job's lock to `copy`, and waits for its answer:
/// past the step's `quiet` or `most`, or once `cancel` is set, the copy is
/// killed and the job is lost, as a fresh worker's would be.
pub fn request(
    copy: &mut Copy,
    step: &StepRun,
    said: Arc<dyn Fn(&str) + Send + Sync>,
    cancel: &AtomicBool,
    lock: std::fs::File,
) -> Ended {
    let fail = |why: String| Ended {
        outcome: Err(Failure::Error(why.clone())),
        keep: false,
        why: Some(why),
    };
    if cancel.load(Ordering::SeqCst) {
        return Ended {
            outcome: Err(Failure::Lost(super::super::journal::JOB_STOPPED.into())),
            keep: true,
            why: None,
        };
    }
    let text = match serde_json::to_string(step) {
        Ok(t) => t,
        Err(e) => return fail(format!("the step: {e}")),
    };
    let sent = super::super::handover::send_framed(
        &copy.ours,
        text.as_bytes(),
        std::os::fd::AsFd::as_fd(&lock),
    );
    // the copy's worker holds the job's lock now, or has gone
    drop(lock);
    if let Err(e) = sent {
        return fail(format!("cannot give the warm copy its step: {e}"));
    }
    let started = Instant::now();
    let heard = Arc::new(Mutex::new(started));
    let (tx, rx) = std::sync::mpsc::channel::<Heard>();
    {
        let (heard, out) = (heard.clone(), copy.out.take());
        std::thread::spawn(move || {
            let Some(mut out) = out else {
                let _ = tx.send((None, None));
                return;
            };
            let mut line = String::new();
            loop {
                line.clear();
                match out.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send((None, None));
                        return;
                    }
                    Ok(_) => {}
                }
                *heard.lock().unwrap_or_else(PoisonError::into_inner) = Instant::now();
                let l = line.trim_end_matches('\n');
                if let Some(done) = l.strip_prefix("done ") {
                    let _ = tx.send((serde_json::from_str(done).ok(), Some(out)));
                    return;
                }
                if let Some(text) = l.strip_prefix("said ")
                    && let Ok(text) = serde_json::from_str::<String>(text)
                {
                    said(&text);
                }
            }
        });
    }
    let quiet = std::time::Duration::from_secs(step.quiet);
    let most = std::time::Duration::from_secs(step.most);
    let (done, out) = loop {
        match overdue(started, &heard, quiet, most, cancel) {
            Err(why) => {
                // its group killed, and nothing of it left, before the job
                // may run again
                stop(&mut copy.child);
                gone(copy.child.id());
                copy.reaped = true;
                return Ended {
                    outcome: Err(Failure::Lost(why)),
                    keep: false,
                    why: Some("killed".into()),
                };
            }
            Ok(wait) => match rx.recv_timeout(wait.min(CANCEL_CHECK)) {
                Ok(h) => break h,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break (None, None),
            },
        }
    };
    copy.out = out;
    let Some(done) = done else {
        // the worker ended without an answer: read as a worker's end is
        let status = copy.child.wait();
        copy.reaped = true;
        let why = String::from_utf8_lossy(&copy.err.lock().unwrap_or_else(PoisonError::into_inner))
            .trim()
            .to_string();
        let why = why.strip_prefix("error: ").unwrap_or(&why).to_string();
        let outcome = match status.ok().and_then(|s| s.code()) {
            None => Err(Failure::Lost("the warm copy was killed".into())),
            Some(LOST_EXIT) => Err(Failure::Lost(why)),
            Some(_) if why.is_empty() => Err(Failure::Error("the warm copy failed".into())),
            Some(_) => Err(Failure::Error(why)),
        };
        return Ended {
            outcome,
            keep: false,
            why: Some("it ended".into()),
        };
    };
    let outcome = match serde_json::from_value::<WarmAnswer>(done["answer"].clone()) {
        Ok(WarmAnswer::Outcome(o)) => Ok(o),
        Ok(WarmAnswer::Error(w)) => Err(Failure::Error(w)),
        Ok(WarmAnswer::Lost(w)) => Err(Failure::Lost(w)),
        Err(e) => Err(Failure::Error(format!("the warm copy's answer: {e}"))),
    };
    Ended {
        outcome,
        keep: done["keep"].as_bool() == Some(true),
        why: done["why"].as_str().map(str::to_string),
    }
}

/// What the pass may do for a warm step.
pub enum Take {
    /// An idle copy of the file as it is now.
    Idle(Copy),
    /// None idle, room for one more: start it.
    New,
    /// As many copies as allowed are busy: the job waits, taking no slot.
    Full,
}

/// Each action file's copies: idle ones, and how many are busy.
pub struct Pool {
    idle: HashMap<PathBuf, Vec<Copy>>,
    busy: HashMap<PathBuf, usize>,
    most: usize,
}

impl Pool {
    pub fn new(most: usize) -> Pool {
        Pool {
            idle: HashMap::new(),
            busy: HashMap::new(),
            most,
        }
    }

    /// A copy for a step of `file` as it is now (`id`), counted busy until
    /// it is given back. Idle copies of the file as it was are taken out
    /// and returned, for the pass to end with `changed`.
    pub fn take(&mut self, file: &Path, id: Identity) -> (Take, Vec<Copy>) {
        let idle = self.idle.entry(file.to_path_buf()).or_default();
        let (now, old): (Vec<Copy>, Vec<Copy>) = idle.drain(..).partition(|c| c.id == id);
        *idle = now;
        let busy = self.busy.entry(file.to_path_buf()).or_default();
        let take = if let Some(c) = idle.pop() {
            *busy += 1;
            Take::Idle(c)
        } else if *busy + idle.len() < self.most {
            *busy += 1;
            Take::New
        } else {
            Take::Full
        };
        (take, old)
    }

    /// A busy copy back: idle when `keep` (and of the file as it is now),
    /// else returned for the pass to end.
    pub fn give_back(&mut self, mut copy: Copy, keep: bool) -> Option<Copy> {
        self.done_with(&copy.file);
        if !keep {
            return Some(copy);
        }
        copy.idle_since = Instant::now();
        self.idle.entry(copy.file.clone()).or_default().push(copy);
        None
    }

    /// A busy count down, for a copy that never was (it could not start).
    pub fn done_with(&mut self, file: &Path) {
        if let Some(n) = self.busy.get_mut(file) {
            *n = n.saturating_sub(1);
        }
    }

    /// Idle copies past `idle` since their last request, taken out.
    pub fn stop_idle(&mut self, now: Instant, idle: std::time::Duration) -> Vec<Copy> {
        let mut out = vec![];
        for copies in self.idle.values_mut() {
            let (keep, old): (Vec<Copy>, Vec<Copy>) = copies
                .drain(..)
                .partition(|c| now.duration_since(c.idle_since) < idle);
            *copies = keep;
            out.extend(old);
        }
        out
    }

    /// When the next idle copy is due to stop.
    pub fn next_idle(&self, idle: std::time::Duration) -> Option<Instant> {
        self.idle
            .values()
            .flatten()
            .map(|c| c.idle_since + idle)
            .min()
    }
}
