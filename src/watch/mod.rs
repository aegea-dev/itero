//! Watching files and folders with the OS's own events. A `Backend` turns
//! one system's raw events into `Change`s; kqueue (macOS) and inotify
//! (Linux) come first, and FSEvents, fanotify or a polling backend plug in
//! behind the same trait. A backend that may have missed something reports
//! `Rescan`, so the reader looks again instead of trusting silence.
//! Debouncing happens once, here, not in each backend.
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod inotify;
#[cfg(target_os = "macos")]
mod kqueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Created,
    Changed,
    Removed,
    Renamed,
    /// Something may have been missed: read the state again.
    Rescan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: PathBuf,
    pub kind: Kind,
}

/// One system's file events, for any number of paths.
pub trait Backend: Send {
    /// Waits up to `timeout`, or for good with none, for changes; none
    /// when it ran out.
    fn wait(&mut self, timeout: Option<Duration>) -> std::io::Result<Vec<Change>>;
    /// Watches these paths from now on: new ones are added, the others
    /// dropped.
    fn set(&mut self, paths: &[PathBuf]) -> std::io::Result<()>;
}

/// This system's backend, watching files and folders, one descriptor for
/// all. A file that is not there yet is watched for: its creation is a
/// change.
pub fn open_many(paths: &[PathBuf]) -> std::io::Result<Box<dyn Backend>> {
    #[cfg(target_os = "macos")]
    return Ok(Box::new(kqueue::Kqueue::new(paths)?));
    #[cfg(target_os = "linux")]
    return Ok(Box::new(inotify::Inotify::new(paths)?));
    #[allow(unreachable_code)]
    Err(std::io::Error::other(format!(
        "watching {paths:?} needs macOS or Linux"
    )))
}

/// An error about watching `p`, naming it.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn naming(p: &Path, e: std::io::Error) -> std::io::Error {
    std::io::Error::new(e.kind(), format!("{}: {e}", p.display()))
}

/// This system's backend, watching one file or folder.
pub fn open(path: &Path) -> std::io::Result<Box<dyn Backend>> {
    open_many(&[path.to_path_buf()])
}

/// Two changes to one path, as one: what a reader needs to know.
fn merge(old: Kind, new: Kind) -> Kind {
    use Kind::*;
    match (old, new) {
        (Rescan, _) | (_, Rescan) => Rescan,
        (Removed | Renamed, Created) => Changed,
        (Created, Changed) => Created,
        (_, k) => k,
    }
}

/// Holds changes until they have been quiet for `quiet`, or the first of
/// them is `max_wait` old, then gives one change per path.
pub struct Debouncer {
    quiet: Duration,
    max_wait: Duration,
    pending: BTreeMap<PathBuf, Kind>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debouncer {
    pub fn new(quiet: Duration, max_wait: Duration) -> Debouncer {
        Debouncer {
            quiet,
            max_wait,
            pending: BTreeMap::new(),
            first: None,
            last: None,
        }
    }

    pub fn add(&mut self, changes: Vec<Change>, now: Instant) {
        if changes.is_empty() {
            return;
        }
        for c in changes {
            let k = match self.pending.get(&c.path) {
                Some(old) => merge(*old, c.kind),
                None => c.kind,
            };
            self.pending.insert(c.path, k);
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// When the held changes are due, if any are held.
    pub fn due(&self) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet).min(first + self.max_wait))
    }

    /// The held changes, when due at `now`; else none.
    pub fn take(&mut self, now: Instant) -> Vec<Change> {
        match self.due() {
            Some(at) if now >= at => {
                self.first = None;
                self.last = None;
                std::mem::take(&mut self.pending)
                    .into_iter()
                    .map(|(path, kind)| Change { path, kind })
                    .collect()
            }
            _ => vec![],
        }
    }
}

/// Watches until the backend fails or `emit` does (its reader went away).
pub fn run(
    backend: &mut dyn Backend,
    d: &mut Debouncer,
    mut emit: impl FnMut(Change) -> std::io::Result<()>,
) -> std::io::Result<()> {
    loop {
        let now = Instant::now();
        // nothing pending: wait for good
        let timeout = d.due().map(|at| at.saturating_duration_since(now));
        let changes = backend.wait(timeout)?;
        let now = Instant::now();
        d.add(changes, now);
        for c in d.take(now) {
            emit(c)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(p: &str, kind: Kind) -> Change {
        Change {
            path: PathBuf::from(p),
            kind,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn changes_wait_for_quiet_then_come_once_per_path() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(ms(500), ms(5000));
        d.add(vec![c("/posta/kutu", Kind::Changed)], t0);
        d.add(vec![c("/posta/kutu", Kind::Changed)], t0 + ms(200));
        assert_eq!(d.due(), Some(t0 + ms(700)));
        assert!(d.take(t0 + ms(600)).is_empty(), "not quiet yet");
        assert_eq!(d.take(t0 + ms(700)), vec![c("/posta/kutu", Kind::Changed)]);
        assert_eq!(d.due(), None);
    }

    #[test]
    fn a_steady_stream_is_cut_at_max_wait() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(ms(500), ms(2000));
        for i in 0..10 {
            d.add(vec![c("/günlük.log", Kind::Changed)], t0 + ms(i * 300));
        }
        assert_eq!(d.due(), Some(t0 + ms(2000)));
        assert_eq!(d.take(t0 + ms(2000)).len(), 1);
    }

    #[test]
    fn kinds_merge_into_what_a_reader_needs_to_know() {
        let t0 = Instant::now();
        let merged = |a: Kind, b: Kind| {
            let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
            d.add(vec![c("/a", a), c("/a", b)], t0);
            d.take(t0)[0].kind
        };
        assert_eq!(
            merged(Kind::Removed, Kind::Created),
            Kind::Changed,
            "replaced"
        );
        assert_eq!(
            merged(Kind::Renamed, Kind::Created),
            Kind::Changed,
            "replaced"
        );
        assert_eq!(merged(Kind::Created, Kind::Changed), Kind::Created);
        assert_eq!(merged(Kind::Created, Kind::Removed), Kind::Removed);
        assert_eq!(merged(Kind::Changed, Kind::Rescan), Kind::Rescan);
        assert_eq!(merged(Kind::Rescan, Kind::Changed), Kind::Rescan);
        let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
        d.add(vec![c("/b", Kind::Changed), c("/a", Kind::Removed)], t0);
        let out = d.take(t0);
        assert_eq!(out.len(), 2, "one per path");
        assert_eq!(out[0].path, PathBuf::from("/a"), "in path order");
    }

    struct Fake(std::collections::VecDeque<Vec<Change>>);

    impl Backend for Fake {
        fn wait(&mut self, _: Option<Duration>) -> std::io::Result<Vec<Change>> {
            self.0
                .pop_front()
                .ok_or_else(|| std::io::Error::other("bitti"))
        }

        fn set(&mut self, _: &[PathBuf]) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn run_emits_debounced_changes_until_the_backend_ends() {
        let mut b = Fake(
            vec![
                vec![c("/ç", Kind::Changed)],
                vec![],
                vec![c("/ç", Kind::Changed)],
            ]
            .into(),
        );
        let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
        let mut seen = vec![];
        let end = run(&mut b, &mut d, |ch| {
            seen.push(ch);
            Ok(())
        });
        assert!(end.is_err(), "the fake backend ends with an error");
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn run_stops_when_its_reader_goes() {
        let mut b = Fake(vec![vec![c("/ş", Kind::Changed)]; 3].into());
        let mut d = Debouncer::new(Duration::ZERO, Duration::ZERO);
        let end = run(&mut b, &mut d, |_| {
            Err(std::io::Error::other("okuyan gitti"))
        });
        assert_eq!(end.unwrap_err().to_string(), "okuyan gitti");
        assert_eq!(b.0.len(), 2, "stopped at the first failed emit");
    }
}
