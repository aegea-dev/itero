//! macOS: kqueue's vnode events on each target and on its parent folder,
//! all on one kqueue. A target's own events say it changed, was removed or
//! renamed; its parent's say an entry came or went, which is how a file
//! that is not there yet, or that an atomic save replaced, is found again.
//! A target that is a folder is in folder mode too: FSEvents tells a file
//! in it changing (`fsevents`), which kqueue cannot, and wakes this wait.
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

/// One watched path. Closing its descriptors drops their events.
struct Entry {
    path: PathBuf,
    /// The target's own watch and its inode; none while it is not there.
    target: Option<(OwnedFd, u64)>,
    /// The parent folder's watch.
    parent: OwnedFd,
}

pub struct Kqueue {
    /// Before `kq`: its stream wakes `kq`, so it goes first.
    folders: super::fsevents::Folders,
    kq: OwnedFd,
    entries: Vec<Entry>,
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

impl Entry {
    fn new(kq: RawFd, path: &Path) -> io::Result<Entry> {
        let folder = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = open_evtonly(folder)?;
        register(kq, parent.as_raw_fd(), libc::NOTE_WRITE)?;
        let mut e = Entry {
            path: path.to_path_buf(),
            target: None,
            parent,
        };
        e.watch_target(kq)?;
        Ok(e)
    }

    /// Watches the target if it is there and not watched already; true
    /// when a new watch was set.
    fn watch_target(&mut self, kq: RawFd) -> io::Result<bool> {
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
        register(kq, fd.as_raw_fd(), TARGET)?;
        self.target = Some((fd, ino));
        Ok(true)
    }

    /// What its events this time say: `target`, the flags of its target's
    /// events, and `parent`, whether its parent folder had any.
    fn changes(&mut self, kq: RawFd, target: &[u32], parent: bool) -> io::Result<Vec<Kind>> {
        let mut out = vec![];
        let mut look_again = parent;
        for f in target {
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
        }
        if look_again {
            let had = self.target.is_some() || out.iter().any(|k| *k != Kind::Changed);
            if inode(&self.path).is_none() {
                if self.target.take().is_some() {
                    out.push(Kind::Removed);
                }
            } else if self.watch_target(kq)? {
                // a new watch: what was written before it was set is unseen
                out.push(if had { Kind::Rescan } else { Kind::Created });
            }
        }
        Ok(out)
    }
}

impl Kqueue {
    pub fn new(paths: &[PathBuf]) -> io::Result<Kqueue> {
        // SAFETY: kqueue takes no arguments
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor this function owns
        let kq = unsafe { OwnedFd::from_raw_fd(kq) };
        // the folders' stream wakes the wait through this user event
        let ev = libc::kevent {
            ident: super::fsevents::WAKE,
            filter: libc::EVFILT_USER,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one valid kevent in, none out, no timeout
        if unsafe {
            libc::kevent(
                kq.as_raw_fd(),
                &ev,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut k = Kqueue {
            folders: super::fsevents::Folders::new(kq.as_raw_fd()),
            kq,
            entries: vec![],
        };
        k.set(paths)?;
        Ok(k)
    }

    /// The targets that are folders now, in folder mode: their files'
    /// changes come from FSEvents.
    fn sync_folders(&mut self) -> io::Result<()> {
        let folders: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.target.is_some() && e.path.is_dir())
            .map(|e| e.path.clone())
            .collect();
        self.folders.set(&folders)
    }
}

impl Backend for Kqueue {
    fn set(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        self.entries.retain(|e| paths.contains(&e.path));
        let mut failed = None;
        for p in paths {
            if !self.entries.iter().any(|e| e.path == *p) {
                match Entry::new(self.kq.as_raw_fd(), p) {
                    Ok(e) => self.entries.push(e),
                    Err(e) => {
                        failed.get_or_insert(super::naming(p, e));
                    }
                }
            }
        }
        self.sync_folders()?;
        failed.map_or(Ok(()), Err)
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<Vec<Change>> {
        let ts = timeout.map(|t| libc::timespec {
            tv_sec: t.as_secs().min(i64::MAX as u64) as libc::time_t,
            tv_nsec: t.subsec_nanos() as libc::c_long,
        });
        // SAFETY: kevent is plain data; all zeros is a valid value
        let mut evs: [libc::kevent; 64] = unsafe { std::mem::zeroed() };
        // SAFETY: room for 64 events, and a valid timeout or none
        let n = unsafe {
            libc::kevent(
                self.kq.as_raw_fd(),
                std::ptr::null(),
                0,
                evs.as_mut_ptr(),
                evs.len() as i32,
                ts.as_ref()
                    .map_or(std::ptr::null(), |t| t as *const libc::timespec),
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
        let evs = &evs[..n as usize];
        let kq = self.kq.as_raw_fd();
        let mut out = vec![];
        if evs.iter().any(|ev| ev.filter == libc::EVFILT_USER) {
            out.extend(self.folders.take());
        }
        let evs: Vec<&libc::kevent> = evs
            .iter()
            .filter(|ev| ev.filter == libc::EVFILT_VNODE)
            .collect();
        for e in &mut self.entries {
            let target = e.target.as_ref().map(|(fd, _)| fd.as_raw_fd());
            let parent = e.parent.as_raw_fd();
            let mine: Vec<u32> = evs
                .iter()
                .filter(|ev| Some(ev.ident as RawFd) == target)
                .map(|ev| ev.fflags)
                .collect();
            let parent_moved = evs.iter().any(|ev| ev.ident as RawFd == parent);
            if mine.is_empty() && !parent_moved {
                continue;
            }
            for kind in e.changes(kq, &mine, parent_moved)? {
                out.push(Change {
                    path: e.path.clone(),
                    kind,
                });
            }
        }
        // a target made or removed may have come into folder mode or left it
        self.sync_folders()?;
        Ok(out)
    }
}
