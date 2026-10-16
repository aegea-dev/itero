//! macOS: kqueue's vnode events on the target and on its parent folder.
//! The target's own events say it changed, was removed or renamed; the
//! parent's say an entry came or went, which is how a file that is not
//! there yet, or that an atomic save replaced, is found again.
use super::{Backend, Change, Kind};
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TARGET: u32 = libc::NOTE_WRITE
    | libc::NOTE_EXTEND
    | libc::NOTE_ATTRIB
    | libc::NOTE_DELETE
    | libc::NOTE_RENAME
    | libc::NOTE_REVOKE;

pub struct Kqueue {
    kq: OwnedFd,
    path: PathBuf,
    /// The target's own watch and its inode; none while it is not there.
    target: Option<(OwnedFd, u64)>,
    /// The parent folder's watch.
    parent: OwnedFd,
}

/// Opens a path for events only: it keeps no volume busy.
fn open_evtonly(p: &Path) -> io::Result<OwnedFd> {
    let c = CString::new(p.as_os_str().as_bytes())?;
    // SAFETY: a valid C string; O_EVTONLY opens for events only
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a new descriptor this function owns
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn inode(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| m.ino())
}

fn register(kq: RawFd, fd: RawFd, fflags: u32) -> io::Result<()> {
    let ev = libc::kevent {
        ident: fd as libc::uintptr_t,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one valid kevent in, none out, no timeout
    let n = unsafe { libc::kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl Kqueue {
    pub fn new(path: &Path) -> io::Result<Kqueue> {
        // SAFETY: kqueue takes no arguments
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor this function owns
        let kq = unsafe { OwnedFd::from_raw_fd(kq) };
        let folder = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = open_evtonly(folder)?;
        register(kq.as_raw_fd(), parent.as_raw_fd(), libc::NOTE_WRITE)?;
        let mut k = Kqueue {
            kq,
            path: path.to_path_buf(),
            target: None,
            parent,
        };
        k.watch_target()?;
        Ok(k)
    }

    /// Watches the target if it is there and not watched already; true
    /// when a new watch was set.
    fn watch_target(&mut self) -> io::Result<bool> {
        let Some(ino) = inode(&self.path) else {
            self.target = None;
            return Ok(false);
        };
        if self.target.as_ref().is_some_and(|(_, i)| *i == ino) {
            return Ok(false);
        }
        let fd = match open_evtonly(&self.path) {
            Ok(fd) => fd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        register(self.kq.as_raw_fd(), fd.as_raw_fd(), TARGET)?;
        self.target = Some((fd, ino));
        Ok(true)
    }
}

impl Backend for Kqueue {
    fn wait(&mut self, timeout: Duration) -> io::Result<Vec<Change>> {
        let ts = libc::timespec {
            tv_sec: timeout.as_secs().min(i64::MAX as u64) as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as libc::c_long,
        };
        // SAFETY: kevent is plain data; all zeros is a valid value
        let mut evs: [libc::kevent; 16] = unsafe { std::mem::zeroed() };
        // SAFETY: room for 16 events and a valid timeout
        let n = unsafe {
            libc::kevent(
                self.kq.as_raw_fd(),
                std::ptr::null(),
                0,
                evs.as_mut_ptr(),
                evs.len() as i32,
                &ts,
            )
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted {
                Ok(vec![])
            } else {
                Err(e)
            };
        }
        let mut out = vec![];
        let target = self.target.as_ref().map(|(fd, _)| fd.as_raw_fd());
        let parent = self.parent.as_raw_fd();
        let mut look_again = false;
        for ev in &evs[..n as usize] {
            let fd = ev.ident as RawFd;
            if Some(fd) == target {
                let f = ev.fflags;
                let kind = if f & libc::NOTE_DELETE != 0 {
                    Kind::Removed
                } else if f & libc::NOTE_RENAME != 0 {
                    Kind::Renamed
                } else if f & libc::NOTE_REVOKE != 0 {
                    Kind::Rescan
                } else {
                    Kind::Changed
                };
                if kind != Kind::Changed {
                    // its watch now follows an inode that is gone or moved
                    self.target = None;
                    look_again = true;
                }
                out.push(kind);
            } else if fd == parent {
                // an entry in the parent came or went: perhaps the target
                look_again = true;
            }
        }
        if look_again {
            let had = self.target.is_some() || out.iter().any(|k| *k != Kind::Changed);
            if inode(&self.path).is_none() {
                if self.target.take().is_some() {
                    out.push(Kind::Removed);
                }
            } else if self.watch_target()? {
                // a new watch: what was written before it was set is unseen
                out.push(if had { Kind::Rescan } else { Kind::Created });
            }
        }
        Ok(out
            .into_iter()
            .map(|kind| Change {
                path: self.path.clone(),
                kind,
            })
            .collect())
    }
}
