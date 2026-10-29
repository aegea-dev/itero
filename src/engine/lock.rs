//! One daemon per workspace's journal: whichever daemon runs a workspace
//! holds an exclusive lock on its `.itero/daemon.lock` for as long as it
//! does, and writes in it who it is. The lock ends with the process,
//! however it ends.
use crate::workspace::Workspace;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;

/// A workspace's lock, held while this lives.
pub struct DaemonLock {
    _file: File,
}

/// Locks `ws` for the daemon `who` (its name and pid), or says which
/// daemon runs it already.
pub fn take(ws: &Workspace, who: &str) -> Result<DaemonLock, String> {
    let dir = ws.root.join(".itero");
    let path = dir.join("daemon.lock");
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(&dir).map_err(at)?;
    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(at)?;
    // SAFETY: flock on a descriptor this function owns
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(at(e));
        }
        let mut holder = String::new();
        let _ = file.read_to_string(&mut holder);
        return Err(format!(
            "{}: this workspace is run by {} already",
            ws.root.display(),
            holder.trim()
        ));
    }
    file.set_len(0).map_err(at)?;
    file.rewind().map_err(at)?;
    file.write_all(who.as_bytes()).map_err(at)?;
    Ok(DaemonLock { _file: file })
}
