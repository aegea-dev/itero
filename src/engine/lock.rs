//! One daemon per workspace's journal: whichever daemon runs a workspace
//! holds an exclusive lock on its `.itero/daemon.lock` for as long as it
//! does, and writes in it who it is. The lock ends with the process,
//! however it ends.
use crate::workspace::Workspace;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

/// A workspace's lock, held while this lives.
pub struct DaemonLock {
    _file: File,
}

/// Why a lock was not taken.
#[derive(Debug)]
pub enum Refused {
    /// Another holds it: what it wrote in the file.
    Held(String),
    Failed(String),
}

/// Locks the file `path` (made, with its folder, when missing) for `who`,
/// written in it; the lock ends when the result drops.
pub fn take_file(path: &Path, who: &str) -> Result<DaemonLock, Refused> {
    let at = |e: std::io::Error| Refused::Failed(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(at)?;
    }
    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(at)?;
    // SAFETY: flock on a descriptor this function owns
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(at(e));
        }
        let mut holder = String::new();
        let _ = file.read_to_string(&mut holder);
        return Err(Refused::Held(holder.trim().to_string()));
    }
    file.set_len(0).map_err(at)?;
    file.rewind().map_err(at)?;
    file.write_all(who.as_bytes()).map_err(at)?;
    Ok(DaemonLock { _file: file })
}

fn path(ws: &Workspace) -> std::path::PathBuf {
    ws.root.join(".itero").join("daemon.lock")
}

/// Locks `ws` for the daemon `who` (its name and pid), or says which
/// daemon runs it already. A command asking whether a daemon runs it
/// holds the lock for an instant ([`held`]): a lock still held after a
/// short while is a daemon's.
pub fn take(ws: &Workspace, who: &str) -> Result<DaemonLock, String> {
    let mut tries = 0;
    let taken = loop {
        match take_file(&path(ws), who) {
            Err(Refused::Held(_)) if tries < 10 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => break other,
        }
    };
    match taken {
        Ok(l) => Ok(l),
        Err(Refused::Held(by)) => Err(format!(
            "{}: this workspace is run by {by} already",
            ws.root.display()
        )),
        Err(Refused::Failed(why)) => Err(why),
    }
}

/// Whether a daemon runs `ws`: it holds the workspace's lock, with or
/// without its HTTP API. Asked by trying the lock and letting it go at
/// once; the file is not made or written.
pub fn held(ws: &Workspace) -> Result<bool, String> {
    let p = path(ws);
    let file = match File::open(&p) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", p.display())),
    };
    // SAFETY: flock on a descriptor this function owns; closing it lets
    // the lock go
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
        return Ok(false);
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(true),
        _ => Err(format!("{}: {e}", p.display())),
    }
}
