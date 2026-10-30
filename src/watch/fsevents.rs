//! macOS, a watched folder's files: FSEvents, one stream for the kqueue
//! backend's folders, on its own dispatch queue. kqueue tells a folder's
//! own events (made, removed, an entry coming or going) but not a file in
//! it changing; FSEvents tells that, for a file of any name. It is not used
//! for a single file: it cannot watch one whose name has a composed letter
//! (`ü` stored as one character), and watching it through its folder
//! would bring in the folder's whole tree.
//!
//! The stream's callback sends each event over a channel and wakes the
//! kqueue wait with a user event (`WAKE`); the backend then `take`s them.
//! An event on a direct child of a watched folder is `changed` for the
//! folder; the folder's own events and deeper ones are left to kqueue, or
//! out, as inotify on Linux.
use super::{Change, Kind};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_core_foundation::{
    CFArray, CFMutableString, CFRetained, CFString, CFStringNormalizationForm,
};
use objc2_core_services::{
    ConstFSEventStreamRef, FSEventStreamContext, FSEventStreamCreate, FSEventStreamEventFlags,
    FSEventStreamEventId, FSEventStreamInvalidate, FSEventStreamRef, FSEventStreamRelease,
    FSEventStreamSetDispatchQueue, FSEventStreamStart, FSEventStreamStop,
    kFSEventStreamCreateFlagFileEvents, kFSEventStreamCreateFlagNoDefer,
    kFSEventStreamEventFlagKernelDropped, kFSEventStreamEventFlagMustScanSubDirs,
    kFSEventStreamEventFlagUserDropped, kFSEventStreamEventIdSinceNow,
};
use std::ffi::{CStr, c_char, c_void};
use std::io;
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::mpsc::{Receiver, Sender, channel};

/// The kqueue user event the stream's callback triggers.
pub(super) const WAKE: libc::uintptr_t = 1;

/// One event as FSEvents gives it.
struct Raw {
    path: PathBuf,
    flags: u32,
    id: u64,
}

/// Events were missed: what is under the path must be read again.
const DROPPED: u32 = kFSEventStreamEventFlagMustScanSubDirs
    | kFSEventStreamEventFlagUserDropped
    | kFSEventStreamEventFlagKernelDropped;

/// A file's own events, told at once.
const FLAGS: u32 = kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer;

/// `p` with its names decomposed (NFD), so a name written composed (`ü`)
/// or not (`u` and a mark) is the same. APFS finds a file by either.
fn decomposed(p: &Path) -> PathBuf {
    let Some(s) = p.to_str().filter(|s| !s.is_ascii()) else {
        return p.to_path_buf();
    };
    let Some(m) = CFMutableString::new_copy(None, 0, Some(&CFString::from_str(s))) else {
        return p.to_path_buf();
    };
    // SAFETY: a valid mutable string, normalized in place
    unsafe { CFMutableString::normalize(Some(&m), CFStringNormalizationForm::D) };
    PathBuf::from(m.to_string())
}

/// A watched folder: as named, as FSEvents is given it (real), and that
/// decomposed, which events are compared with.
struct Folder {
    path: PathBuf,
    real: PathBuf,
    key: PathBuf,
}

/// What one event says of the watched folders.
fn folder_changes(folders: &[Folder], raw: &Raw) -> Vec<Change> {
    let path = decomposed(&raw.path);
    folders
        .iter()
        .filter_map(|f| {
            let kind = if raw.flags & DROPPED != 0 {
                (path.starts_with(&f.key) || f.key.starts_with(&path)).then_some(Kind::Rescan)
            } else {
                (path.parent() == Some(f.key.as_path())).then_some(Kind::Changed)
            };
            kind.map(|kind| Change {
                path: f.path.clone(),
                kind,
            })
        })
        .collect()
}

/// What the stream's callback is given: where to send, and what to wake.
struct Wake {
    tx: Sender<Raw>,
    kq: RawFd,
}

/// The stream's callback, on its queue: each event to the backend, then
/// the backend's kqueue woken.
unsafe extern "C-unwind" fn heard(
    _: ConstFSEventStreamRef,
    info: *mut c_void,
    n: usize,
    paths: NonNull<c_void>,
    flags: NonNull<FSEventStreamEventFlags>,
    ids: NonNull<FSEventStreamEventId>,
) {
    // SAFETY: info is the Wake the stream was made with, freed only once
    // the stream is invalidated and its queue drained
    let wake = unsafe { &*(info as *const Wake) };
    let paths = paths.as_ptr() as *const *const c_char;
    for i in 0..n {
        // SAFETY: FSEvents gives n C strings, n flags and n ids
        let (p, f, id) = unsafe {
            (
                CStr::from_ptr(*paths.add(i)),
                *flags.as_ptr().add(i),
                *ids.as_ptr().add(i),
            )
        };
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(p.to_bytes()));
        let _ = wake.tx.send(Raw { path, flags: f, id });
    }
    let ev = libc::kevent {
        ident: WAKE,
        filter: libc::EVFILT_USER,
        flags: 0,
        fflags: libc::NOTE_TRIGGER,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one valid kevent in, none out, on the backend's kqueue, which
    // outlives the stream
    unsafe { libc::kevent(wake.kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
}

/// A stream on its queue; stopped, invalidated and released when dropped.
struct Stream {
    s: FSEventStreamRef,
    queue: DispatchRetained<DispatchQueue>,
    info: *mut Wake,
    started: bool,
}

// SAFETY: a stream may be used from any thread; this one is touched only by
// the backend that owns it, and its callback runs on its own queue
unsafe impl Send for Stream {}

impl Stream {
    /// Watches `reals` from event `since` on.
    fn start(reals: &[PathBuf], since: u64, wake: Wake) -> io::Result<Stream> {
        let names: Vec<CFRetained<CFString>> = reals
            .iter()
            .map(|p| CFString::from_str(&p.to_string_lossy()))
            .collect();
        let paths = CFArray::from_retained_objects(&names);
        let info = Box::into_raw(Box::new(wake));
        let mut ctx = FSEventStreamContext {
            version: 0,
            info: info.cast(),
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: a valid callback and context, and an array of CFStrings
        let s = unsafe {
            FSEventStreamCreate(
                None,
                Some(heard),
                &mut ctx,
                paths.as_opaque(),
                since,
                0.05,
                FLAGS,
            )
        };
        if s.is_null() {
            // SAFETY: handed to no stream
            drop(unsafe { Box::from_raw(info) });
            return Err(io::Error::other("FSEvents made no stream"));
        }
        let mut stream = Stream {
            s,
            queue: DispatchQueue::new("itero.watch", None),
            info,
            started: false,
        };
        // SAFETY: a valid stream and queue; dropping `stream` cleans up if
        // it does not start
        unsafe { FSEventStreamSetDispatchQueue(stream.s, Some(&stream.queue)) };
        stream.started = unsafe { FSEventStreamStart(stream.s) };
        if !stream.started {
            return Err(io::Error::other("FSEvents did not start the stream"));
        }
        Ok(stream)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: a valid stream, stopped only when started
        unsafe {
            if self.started {
                FSEventStreamStop(self.s);
            }
            FSEventStreamInvalidate(self.s);
        }
        // a callback already queued runs before this returns
        self.queue.exec_sync(|| {});
        // SAFETY: no callback runs any more: the stream and its Wake are
        // this one's to free
        unsafe {
            FSEventStreamRelease(self.s);
            drop(Box::from_raw(self.info));
        }
    }
}

/// The FSEvents side of the kqueue backend: its folders' files.
pub(super) struct Folders {
    folders: Vec<Folder>,
    stream: Option<Stream>,
    tx: Sender<Raw>,
    rx: Receiver<Raw>,
    kq: RawFd,
    /// The last event seen: a new stream starts after it.
    last: u64,
}

impl Folders {
    /// For the backend whose kqueue is `kq`, which must outlive this.
    pub(super) fn new(kq: RawFd) -> Folders {
        let (tx, rx) = channel();
        Folders {
            folders: vec![],
            stream: None,
            tx,
            rx,
            kq,
            last: kFSEventStreamEventIdSinceNow,
        }
    }

    /// Watches these folders (as named) from now on; a new stream only when
    /// they differ. One that is gone is left out: kqueue tells that.
    pub(super) fn set(&mut self, folders: &[PathBuf]) -> io::Result<()> {
        let wanted: Vec<Folder> = folders
            .iter()
            .filter_map(|p| {
                let real = std::fs::canonicalize(p).ok()?;
                Some(Folder {
                    path: p.clone(),
                    key: decomposed(&real),
                    real,
                })
            })
            .collect();
        let same = wanted.len() == self.folders.len()
            && wanted
                .iter()
                .zip(&self.folders)
                .all(|(a, b)| a.path == b.path && a.real == b.real);
        if same {
            return Ok(());
        }
        self.stream = None;
        self.folders = wanted;
        if self.folders.is_empty() {
            return Ok(());
        }
        let reals: Vec<PathBuf> = self.folders.iter().map(|f| f.real.clone()).collect();
        let wake = Wake {
            tx: self.tx.clone(),
            kq: self.kq,
        };
        self.stream = Some(Stream::start(&reals, self.last, wake).map_err(|e| {
            let named: Vec<String> = reals.iter().map(|p| p.display().to_string()).collect();
            io::Error::new(e.kind(), format!("{}: {e}", named.join(", ")))
        })?);
        Ok(())
    }

    /// The changes the events heard since say.
    pub(super) fn take(&mut self) -> Vec<Change> {
        let mut out = vec![];
        for raw in self.rx.try_iter() {
            if self.last == kFSEventStreamEventIdSinceNow || raw.id > self.last {
                self.last = raw.id;
            }
            out.extend(folder_changes(&self.folders, &raw));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(path: &str, flags: u32) -> Raw {
        Raw {
            path: PathBuf::from(path),
            flags,
            id: 1,
        }
    }

    fn folder(named: &str, real: &str) -> Folder {
        Folder {
            path: PathBuf::from(named),
            real: PathBuf::from(real),
            key: decomposed(Path::new(real)),
        }
    }

    fn kinds(fs: &[Folder], r: &Raw) -> Vec<Kind> {
        folder_changes(fs, r).into_iter().map(|c| c.kind).collect()
    }

    #[test]
    fn a_name_composed_or_not_is_the_same_path() {
        let composed = Path::new("/gelen/henüz yok");
        let apart = Path::new("/gelen/henu\u{308}z yok");
        assert_ne!(composed, apart);
        assert_eq!(decomposed(composed), apart);
        assert_eq!(decomposed(apart), apart);
        assert_eq!(decomposed(Path::new("/ascii")), Path::new("/ascii"));
    }

    #[test]
    fn a_file_in_a_folder_changes_the_folder_and_nothing_else_does() {
        let fs = [folder("~/Gelen Kutusü", "/Users/ben/Gelen Kutusü")];
        // a direct child, named composed or not, under the folder as named
        let c = folder_changes(&fs, &raw("/Users/ben/Gelen Kutusü/fatura.pdf", 0x1000));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].path, PathBuf::from("~/Gelen Kutusü"));
        assert_eq!(c[0].kind, Kind::Changed);
        assert_eq!(
            kinds(
                &fs,
                &raw("/Users/ben/Gelen Kutusu\u{308}/makbuz.pdf", 0x1000)
            ),
            [Kind::Changed]
        );
        // the folder itself (kqueue tells it), deeper, and beside: nothing
        assert!(kinds(&fs, &raw("/Users/ben/Gelen Kutusü", 0x100)).is_empty());
        assert!(kinds(&fs, &raw("/Users/ben/Gelen Kutusü/eski/a.pdf", 0x1000)).is_empty());
        assert!(kinds(&fs, &raw("/Users/ben/komşu.txt", 0x1000)).is_empty());
    }

    #[test]
    fn dropped_events_are_a_rescan_of_the_folders_they_concern() {
        let fs = [folder("/a/klasör", "/a/klasör"), folder("/b", "/b")];
        assert_eq!(
            kinds(&fs, &raw("/a", kFSEventStreamEventFlagMustScanSubDirs)),
            [Kind::Rescan]
        );
        assert_eq!(
            kinds(&fs, &raw("/", kFSEventStreamEventFlagKernelDropped)),
            [Kind::Rescan, Kind::Rescan]
        );
        assert!(kinds(&fs, &raw("/c", kFSEventStreamEventFlagUserDropped)).is_empty());
    }
}
