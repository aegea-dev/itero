//! Connectors: executables in `connectors/` that keep running and tell itero
//! when something happens outside:
//!
//! - each line a connector prints is the data of one message, a JSON object;
//!   the message's type is the connector's file name (`mail.received`);
//! - its stdin stays open while the daemon runs, and closes when it goes;
//! - its stderr goes to the daemon's log;
//! - `itero` on its PATH is the daemon's own.
//!
//! `itero watch <path>` is there for connectors that watch a file or folder;
//! `itero webhook` for those that take requests from outside.
use crate::flow::Listen;
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub mod supervise;
pub mod webhook;

/// A copy of a connector: its file, run with one set of args. The daemon
/// runs one for each different set the workflows' `on` entries give it.
#[derive(Debug, Clone, PartialEq)]
pub struct Copy {
    /// Its event's type (the file's name) and its args, as written.
    pub listen: Listen,
    pub file: PathBuf,
}

impl Copy {
    /// A copy with no args, as a connector no `on` entry gives args runs.
    pub fn plain(kind: &str, file: PathBuf) -> Copy {
        Copy {
            listen: Listen::of(kind),
            file,
        }
    }

    /// How the log and `/health` name it: its type, then its args as
    /// written (`mail.received {"MAILBOX":"sales"}`). No secret: a
    /// `$$NAME` is not looked up there.
    pub fn name(&self) -> String {
        match self.listen.key().as_str() {
            "" => self.listen.kind.clone(),
            key => format!("{} {key}", self.listen.kind),
        }
    }

    /// The source of the events it sends: `connector/<type>`, then
    /// `?<args as written>` when it has args. Which workflows an event
    /// reaches is read from it.
    pub fn source(&self) -> String {
        match self.listen.key().as_str() {
            "" => format!("connector/{}", self.listen.kind),
            key => format!("connector/{}?{key}", self.listen.kind),
        }
    }
}

/// The copies to run: each connector once for every different set of
/// args a workflow's `on` entry gives its event, or once with none when no
/// entry gives it args. A workflow that cannot be read is left out (the
/// daemon's pass tells why).
pub fn copies(ws: &Workspace) -> Result<Vec<Copy>> {
    let flows = ws.read_flows()?.flows;
    let mut out = vec![];
    for (kind, file) in ws.connector_files()? {
        let mut sets: BTreeMap<String, Listen> = BTreeMap::new();
        for (_, f) in &flows {
            for e in f.on.events.iter().filter(|e| e.kind == kind) {
                sets.insert(e.key(), e.clone());
            }
        }
        if sets.is_empty() {
            out.push(Copy::plain(&kind, file));
            continue;
        }
        for (_, listen) in sets {
            out.push(Copy {
                listen,
                file: file.clone(),
            });
        }
    }
    Ok(out)
}

/// `itero watch <path>`: a line each time a file or folder changes, from
/// the OS's own file events, debounced (quiet for 500 ms, at most 5 s).
pub struct Watch {
    backend: Box<dyn crate::watch::Backend>,
    debouncer: crate::watch::Debouncer,
}

impl Watch {
    /// Sets the watch: what can go wrong goes wrong here.
    pub fn new(path: &std::path::Path) -> Result<Watch> {
        let backend =
            crate::watch::open(path).with_context(|| format!("cannot watch {}", path.display()))?;
        Ok(Watch {
            backend,
            debouncer: crate::watch::Debouncer::new(
                Duration::from_millis(500),
                Duration::from_secs(5),
            ),
        })
    }

    /// Prints the changes, one JSON line each, until the watch fails or
    /// `out` does.
    pub fn run(mut self, out: &mut dyn std::io::Write) -> Result<()> {
        crate::watch::run(self.backend.as_mut(), &mut self.debouncer, |c| {
            writeln!(out, "{}", json!(c))?;
            out.flush()
        })?;
        Ok(())
    }

    /// Runs `command` (a program and its args, no shell) now, the watch
    /// being set, then again after each change, one run at a time: a
    /// change during a run gives one more. Its output is this process's.
    /// A run that cannot start, or ends with an error, ends the watch.
    pub fn run_command(mut self, command: &[String]) -> Result<()> {
        let (program, args) = command.split_first().context("no command after --")?;
        loop {
            run_once(program, args).with_context(|| format!("`{}`", command.join(" ")))?;
            crate::watch::next(self.backend.as_mut(), &mut self.debouncer)?;
        }
    }
}

/// The pid of the command's run under way, while it is not yet reaped: so
/// `end_run` never signals a pid another process may have by then.
static RUN: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

fn running() -> std::sync::MutexGuard<'static, Option<u32>> {
    RUN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ends the command's run under way, if there is one: `itero watch` is
/// ending, and leaves nothing behind printing to its connector's output.
pub fn end_run() {
    if let Some(pid) = *running() {
        // SAFETY: a signal to our own child, not reaped while RUN holds it
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
}

fn run_once(program: &str, args: &[String]) -> Result<()> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .context("cannot start it")?;
    *running() = Some(child.id());
    let ended = ended_unreaped(child.id());
    *running() = None;
    let status = child.wait()?;
    ended?;
    if !status.success() {
        anyhow::bail!("ended: {status}");
    }
    Ok(())
}

/// Waits for the process `pid` to end, leaving it to be reaped.
fn ended_unreaped(pid: u32) -> std::io::Result<()> {
    loop {
        // SAFETY: siginfo_t is plain data; all zeros is a valid value
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits on our own child, into `info`; WNOWAIT leaves it
        // waitable
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if r == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}
