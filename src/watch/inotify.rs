//! Linux: inotify on each target and on its parent folder, all on one
//! inotify descriptor. A target's own events say it changed, was removed or
//! moved; its parent's say its name came or went, which is how a file that
//! is not there yet, or that an atomic save replaced, is found again.
use super::{Backend, Change, Kind};
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const FILE: u32 = libc::IN_MODIFY
    | libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;
const FOLDER: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MODIFY
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;
const PARENT: u32 = libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO;

/// One watched path. Two paths in one folder share its watch: inotify
/// gives one watch per inode.
struct Entry {
    path: PathBuf,
    /// The target's name in its parent.
    name: Vec<u8>,
    /// The target's watch; none while it is not there.
    target: Option<i32>,
    parent: i32,
}

pub struct Inotify {
    fd: OwnedFd,
    entries: Vec<Entry>,
}

/// Adds to what a watch reports (`IN_MASK_ADD`): a folder may be one
/// path's parent and another's target.
fn add(fd: i32, p: &Path, mask: u32) -> io::Result<i32> {
    let c = CString::new(p.as_os_str().as_bytes())?;
    // SAFETY: a valid descriptor and C string
    let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask | libc::IN_MASK_ADD) };
    if wd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(wd)
    }
}

impl Entry {
    fn new(fd: i32, path: &Path) -> io::Result<Entry> {
        let folder = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = add(fd, folder, PARENT)?;
        let name = path
            .file_name()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default();
        let mut e = Entry {
            path: path.to_path_buf(),
            name,
            target: None,
            parent,
        };
        e.watch_target(fd);
        Ok(e)
    }

    /// Watches the target if it is there; true when the watch is new (a
    /// path watched already gives its watch back unchanged).
    fn watch_target(&mut self, fd: i32) -> bool {
        let mask = if self.path.is_dir() { FOLDER } else { FILE };
        match add(fd, &self.path, mask) {
            Ok(wd) => {
                let new = self.target != Some(wd);
                self.target = Some(wd);
                new
            }
            Err(_) => {
                self.target = None;
                false
            }
        }
    }
}

impl Inotify {
    pub fn new(paths: &[PathBuf]) -> io::Result<Inotify> {
        // SAFETY: no pointers
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut i = Inotify {
            // SAFETY: a new descriptor this function owns
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            entries: vec![],
        };
        i.set(paths)?;
        Ok(i)
    }
}

impl Backend for Inotify {
    fn set(&mut self, paths: &[PathBuf]) -> io::Result<()> {
        let (keep, gone): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| paths.contains(&e.path));
        self.entries = keep;
        // a watch goes when no path kept uses it
        let used =
            |wd: i32, es: &[Entry]| es.iter().any(|e| e.parent == wd || e.target == Some(wd));
        let mut dropped = vec![];
        for e in gone {
            for wd in std::iter::once(e.parent).chain(e.target) {
                if !used(wd, &self.entries) && !dropped.contains(&wd) {
                    dropped.push(wd);
                    // SAFETY: a valid descriptor; a watch gone already is EINVAL
                    unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
                }
            }
        }
        let mut failed = None;
        for p in paths {
            if !self.entries.iter().any(|e| e.path == *p) {
                match Entry::new(self.fd.as_raw_fd(), p) {
                    Ok(e) => self.entries.push(e),
                    Err(e) => {
                        failed.get_or_insert(super::naming(p, e));
                    }
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }

    fn wait(&mut self, timeout: Option<Duration>) -> io::Result<Vec<Change>> {
        let mut pfd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // -1: for good
        let ms = timeout.map_or(-1, |t| t.as_millis().min(i32::MAX as u128) as i32);
        // SAFETY: one valid pollfd
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted {
                Ok(vec![])
            } else {
                Err(e)
            };
        }
        if n == 0 {
            return Ok(vec![]);
        }
        let mut buf = vec![0u8; 64 * 1024];
        // SAFETY: room for buf.len() bytes
        let len = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock {
                Ok(vec![])
            } else {
                Err(e)
            };
        }
        let head = std::mem::size_of::<libc::inotify_event>();
        // per entry: its kinds, whether to look again, whether its watch went
        let mut seen: Vec<(Vec<Kind>, bool, bool)> =
            vec![(vec![], false, false); self.entries.len()];
        let mut at = 0usize;
        while at + head <= len as usize {
            // SAFETY: the kernel wrote a whole inotify_event here
            let ev: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(buf[at..].as_ptr().cast()) };
            let name = &buf[at + head..at + head + ev.len as usize];
            let name = name.split(|b| *b == 0).next().unwrap_or(&[]);
            at += head + ev.len as usize;
            for (e, (out, look_again, lost)) in self.entries.iter_mut().zip(&mut seen) {
                if ev.mask & libc::IN_Q_OVERFLOW != 0 {
                    out.push(Kind::Rescan);
                    *look_again = true;
                } else if Some(ev.wd) == e.target {
                    if ev.mask & libc::IN_DELETE_SELF != 0 {
                        out.push(Kind::Removed);
                        e.target = None;
                        *lost = true;
                    } else if ev.mask & libc::IN_MOVE_SELF != 0 {
                        out.push(Kind::Renamed);
                        e.target = None;
                        *lost = true;
                        *look_again = true;
                    } else if ev.mask & libc::IN_IGNORED != 0 {
                        e.target = None;
                        *lost = true;
                        *look_again = true;
                    } else {
                        out.push(Kind::Changed);
                    }
                } else if ev.wd == e.parent && name == e.name.as_slice() {
                    if ev.mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0 {
                        *look_again = true;
                    } else if ev.mask & (libc::IN_DELETE | libc::IN_MOVED_FROM) != 0
                        && e.target.is_some()
                    {
                        // the name went: the target's own event follows, or
                        // (a move away) never comes
                        out.push(Kind::Removed);
                        *look_again = true;
                    }
                }
            }
        }
        let fd = self.fd.as_raw_fd();
        let mut changes = vec![];
        for (e, (mut out, look_again, lost)) in self.entries.iter_mut().zip(seen) {
            if look_again && e.path.exists() {
                let had = lost || out.iter().any(|k| *k != Kind::Changed);
                if e.watch_target(fd) {
                    // a new watch: what was written before it was set is unseen
                    out.push(if had { Kind::Rescan } else { Kind::Created });
                }
            }
            changes.extend(out.into_iter().map(|kind| Change {
                path: e.path.clone(),
                kind,
            }));
        }
        Ok(changes)
    }
}
