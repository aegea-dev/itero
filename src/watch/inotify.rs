//! Linux: inotify on the target and on its parent folder. The target's own
//! events say it changed, was removed or moved; the parent's say its name
//! came or went, which is how a file that is not there yet, or that an
//! atomic save replaced, is found again.
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

pub struct Inotify {
    fd: OwnedFd,
    path: PathBuf,
    /// The target's name in its parent.
    name: Vec<u8>,
    /// The target's watch; none while it is not there.
    target: Option<i32>,
    parent: i32,
}

fn add(fd: i32, p: &Path, mask: u32) -> io::Result<i32> {
    let c = CString::new(p.as_os_str().as_bytes())?;
    // SAFETY: a valid descriptor and C string
    let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), mask) };
    if wd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(wd)
    }
}

impl Inotify {
    pub fn new(path: &Path) -> io::Result<Inotify> {
        // SAFETY: no pointers
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor this function owns
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let folder = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = add(fd.as_raw_fd(), folder, PARENT)?;
        let name = path
            .file_name()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default();
        let mut i = Inotify {
            fd,
            path: path.to_path_buf(),
            name,
            target: None,
            parent,
        };
        i.watch_target();
        Ok(i)
    }

    /// Watches the target if it is there; true when the watch is new (a
    /// path watched already gives its watch back unchanged).
    fn watch_target(&mut self) -> bool {
        let mask = if self.path.is_dir() { FOLDER } else { FILE };
        match add(self.fd.as_raw_fd(), &self.path, mask) {
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

impl Backend for Inotify {
    fn wait(&mut self, timeout: Duration) -> io::Result<Vec<Change>> {
        let mut pfd = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
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
        let mut out = vec![];
        let mut at = 0usize;
        let mut look_again = false;
        let mut lost = false;
        while at + head <= len as usize {
            // SAFETY: the kernel wrote a whole inotify_event here
            let ev: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(buf[at..].as_ptr().cast()) };
            let name = &buf[at + head..at + head + ev.len as usize];
            let name = name.split(|b| *b == 0).next().unwrap_or(&[]);
            at += head + ev.len as usize;
            if ev.mask & libc::IN_Q_OVERFLOW != 0 {
                out.push(Kind::Rescan);
                look_again = true;
            } else if Some(ev.wd) == self.target {
                if ev.mask & libc::IN_DELETE_SELF != 0 {
                    out.push(Kind::Removed);
                    self.target = None;
                    lost = true;
                } else if ev.mask & libc::IN_MOVE_SELF != 0 {
                    out.push(Kind::Renamed);
                    self.target = None;
                    lost = true;
                    look_again = true;
                } else if ev.mask & libc::IN_IGNORED != 0 {
                    self.target = None;
                    lost = true;
                    look_again = true;
                } else {
                    out.push(Kind::Changed);
                }
            } else if ev.wd == self.parent && name == self.name.as_slice() {
                if ev.mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0 {
                    look_again = true;
                } else if ev.mask & (libc::IN_DELETE | libc::IN_MOVED_FROM) != 0
                    && self.target.is_some()
                {
                    // the name went: the target's own event follows, or
                    // (a move away) never comes
                    out.push(Kind::Removed);
                    look_again = true;
                }
            }
        }
        if look_again && self.path.exists() {
            let had = lost || out.iter().any(|k| *k != Kind::Changed);
            if self.watch_target() {
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
