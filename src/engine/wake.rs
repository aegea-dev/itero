//! The wake pipe: how another process (`itero ws emit`, the webhook, a
//! click) tells a workspace's daemon, at once, that it saved a message. A
//! FIFO at `Workspace::wake_file`: the daemon holds it open for reading and
//! writing, so it never sees an end and opening it never blocks, and a
//! thread of its own wakes the pass at each byte. No file watch and no
//! quiet time: the message is taken as soon as the byte is read.
use super::log::Log;
use super::stop::Stop;
use crate::workspace::Workspace;
use serde_json::json;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::sync::mpsc::Sender;

/// Makes the pipe, in place of whatever is at its path (an older itero's
/// wake file), and opens it for the daemon. Opened before the daemon's
/// first pass: a message saved after that pass looked finds the pipe open,
/// so none is missed between the two.
pub fn open(ws: &Workspace) -> io::Result<File> {
    let path = ws.wake_file();
    std::fs::create_dir_all(path.parent().expect("a folder"))?;
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_fifo() => {}
        Ok(_) => {
            std::fs::remove_file(&path)?;
            make(&path)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => make(&path)?,
        Err(e) => return Err(e),
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
}

fn make(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: a valid, NUL-terminated path
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // made meanwhile by another start: one daemon holds the workspace's
    // lock, so it is the same pipe
    if e.raw_os_error() == Some(libc::EEXIST) {
        return Ok(());
    }
    Err(e)
}

/// Reads the pipe and wakes the pass at each read, until `stop` is set (a
/// stop writes to the pipe too, so the read ends). Several bytes written
/// meanwhile are one read, and one wake.
pub fn listen(mut pipe: File, wake: &Sender<()>, stop: &Stop, log: &Log) {
    let mut buf = [0u8; 256];
    loop {
        match pipe.read(&mut buf) {
            Ok(n) if n > 0 => {
                if stop.is_set() || wake.send(()).is_err() {
                    return;
                }
            }
            // held open for writing too, it has no end: something else
            // happened to it
            Ok(_) => {
                log.write(
                    "error",
                    json!({ "why": "the wake pipe ended: messages wait for the next pass" }),
                );
                return;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                log.write(
                    "error",
                    json!({ "why": format!("reading the wake pipe: {e}; messages wait for the next pass") }),
                );
                return;
            }
        }
    }
}

/// Wakes the daemon of `ws` with one byte into its pipe. No daemon there
/// (no pipe yet, or nobody reading it) is no error: it takes the message
/// when it starts. A full pipe already holds a wake it has not read.
pub fn send(ws: &Workspace) -> io::Result<()> {
    let opened = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(ws.wake_file());
    let mut pipe = match opened {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ENXIO) => {
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    match pipe.write(&[1]) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(e) => Err(e),
    }
}
