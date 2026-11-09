//! Following a workspace live, for a program outside itero: the events
//! that come in (`GET /events/stream`) and the log's lines (`GET
//! /log/stream`), one JSON line each, as they come.
//!
//! Each listener has a thread that looks every `every` (100 ms unless
//! asked) at what is already saved, the journal or the log file, and
//! sends what is new into a small channel, which is the response's body.
//! Nothing changes where events are saved or the log is written, so
//! nobody listening costs nothing, and a slow listener is read at its own
//! pace. When the listener goes, the server drops the body, the channel
//! closes, and the thread ends at its next look.
use super::journal::Journal;
use crate::workspace::Workspace;
use axum::body::Bytes;
use serde_json::json;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

/// How often a stream looks, unless asked.
const EVERY: Duration = Duration::from_millis(100);
/// The quickest a stream may be asked to look: quicker is a thread waking
/// for nothing.
const QUICKEST: Duration = Duration::from_millis(10);
/// The slowest: slower, and a listener would think it dead.
const SLOWEST: Duration = Duration::from_secs(60);
/// Events read at once, so a listener far behind is not read into memory
/// all at once.
const PAGE: usize = 500;
/// Lines waiting for a listener before its thread waits for it.
const ROOM: usize = 64;

/// The two streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Events,
    Log,
}

impl Kind {
    fn route(self) -> &'static str {
        match self {
            Kind::Events => "/events/stream",
            Kind::Log => "/log/stream",
        }
    }
}

/// How many follow now, by stream: `GET /health`'s `streams`.
#[derive(Debug, Default)]
pub struct Streams {
    events: AtomicUsize,
    log: AtomicUsize,
}

impl Streams {
    fn of(&self, kind: Kind) -> &AtomicUsize {
        match kind {
            Kind::Events => &self.events,
            Kind::Log => &self.log,
        }
    }

    pub fn health(&self) -> serde_json::Value {
        json!({
            "events": self.events.load(Ordering::SeqCst),
            "log": self.log.load(Ordering::SeqCst),
        })
    }
}

/// One listener counted, for as long as its thread lives.
struct Counted(Arc<Streams>, Kind);

impl Counted {
    fn new(streams: Arc<Streams>, kind: Kind) -> Counted {
        streams.of(kind).fetch_add(1, Ordering::SeqCst);
        Counted(streams, kind)
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.of(self.1).fetch_sub(1, Ordering::SeqCst);
    }
}

/// Where an events stream starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    /// With the events saved from now on.
    Now,
    /// After this event: every one since, then on.
    Event(String),
}

/// What a listener asked for, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub every: Duration,
    pub after: After,
    /// Only events of these types; every type when empty.
    pub types: Vec<String>,
    /// Ends after this many lines.
    pub count: Option<u64>,
}

/// The query's parameters for the stream `kind`, checked: every problem
/// in one text, joined by `; `.
pub fn asked(query: &[(String, String)], kind: Kind) -> Result<Asked, String> {
    let mut a = Asked {
        every: EVERY,
        after: After::Now,
        types: Vec::new(),
        count: None,
    };
    let mut problems = Vec::new();
    let mut given = std::collections::BTreeSet::new();
    let events_only = |name: &str| format!("{name} is for /events/stream only");
    for (name, value) in query {
        let once = matches!(name.as_str(), "every" | "after" | "count");
        if once && !given.insert(name.clone()) {
            problems.push(format!("{name} is given once"));
            continue;
        }
        match name.as_str() {
            "every" => match humantime::parse_duration(value) {
                Ok(d) if (QUICKEST..=SLOWEST).contains(&d) => a.every = d,
                Ok(_) => problems.push(format!("every is from 10ms to 1m, not {value}")),
                Err(_) => problems.push(format!(
                    "every: a duration is like 100ms or 2s, not \"{value}\""
                )),
            },
            "count" => match value.parse::<u64>() {
                Ok(n) if n >= 1 => a.count = Some(n),
                _ => problems.push(format!(
                    "count is a whole number from 1, not \"{value}\""
                )),
            },
            "after" if kind == Kind::Log => problems.push(events_only("after")),
            "after" => a.after = After::Event(value.clone()),
            "type" if kind == Kind::Log => {
                if given.insert("type".into()) {
                    problems.push(events_only("type"));
                }
            }
            "type" if crate::flow::is_message_type(value) => a.types.push(value.clone()),
            "type" => problems.push(format!(
                "type: an event type is two or more words joined by dots, like order.placed, not {value}"
            )),
            _ => problems.push(format!(
                "{name} is not a parameter of {}: {}",
                kind.route(),
                match kind {
                    Kind::Events => "every, after, type, count",
                    Kind::Log => "every, count",
                }
            )),
        }
    }
    if problems.is_empty() {
        Ok(a)
    } else {
        Err(problems.join("; "))
    }
}

/// Why a stream was not started.
#[derive(Debug)]
pub enum Refused {
    /// `after` names an event the journal does not have.
    Gone(String),
    Internal(anyhow::Error),
}

/// A stream's body: its lines, as its thread sends them, until it ends.
pub struct Follow {
    rx: mpsc::Receiver<Bytes>,
}

impl futures_core::Stream for Follow {
    type Item = Result<Bytes, std::convert::Infallible>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.get_mut().rx.poll_recv(cx).map(|line| line.map(Ok))
    }
}

/// A stream whose thread runs `look`, counted in `streams` as `kind`.
fn start(
    streams: Arc<Streams>,
    kind: Kind,
    look: impl FnOnce(mpsc::Sender<Bytes>) + Send + 'static,
) -> Result<Follow, Refused> {
    let (tx, rx) = mpsc::channel(ROOM);
    let counted = Counted::new(streams, kind);
    std::thread::Builder::new()
        .name(format!("itero-follow{}", kind.route().replace('/', "-")))
        .spawn(move || {
            let _counted = counted;
            look(tx);
        })
        .map_err(|e| Refused::Internal(e.into()))?;
    Ok(Follow { rx })
}

/// Lines sent, against `count`.
struct Sent {
    n: u64,
    count: Option<u64>,
}

impl Sent {
    /// Sends `line`; false when the stream is to end: the listener has gone,
    /// or it has its `count`.
    fn send(&mut self, tx: &mpsc::Sender<Bytes>, line: Bytes) -> bool {
        if tx.blocking_send(line).is_err() {
            return false;
        }
        self.n += 1;
        self.count != Some(self.n)
    }
}

/// The events that come into the workspace `ws`, as `asked`. Where it
/// starts is found before it answers, so an `after` the journal does not
/// have is refused at once.
pub fn events(ws: &Workspace, asked: Asked, streams: Arc<Streams>) -> Result<Follow, Refused> {
    let j = Journal::open(ws).map_err(Refused::Internal)?;
    let from = match &asked.after {
        After::Now => j.last_seq().map_err(Refused::Internal)?,
        After::Event(id) => j
            .seq_came_in(id)
            .map_err(Refused::Internal)?
            .ok_or_else(|| {
                Refused::Gone(format!("no event {id} here: forgotten, or never sent"))
            })?,
    };
    start(streams, Kind::Events, move |tx| {
        if let Err(e) = look_at_events(&j, from, &asked, &tx) {
            eprintln!("itero: following events stopped: {e:#}");
        }
    })
}

fn look_at_events(
    j: &Journal,
    mut from: i64,
    asked: &Asked,
    tx: &mpsc::Sender<Bytes>,
) -> anyhow::Result<()> {
    let mut sent = Sent {
        n: 0,
        count: asked.count,
    };
    // none yet: the first look reads, for what came since the request
    let mut version = None;
    while !tx.is_closed() {
        let v = j.changed()?;
        if version != Some(v) {
            version = Some(v);
            loop {
                let page = j.came_in_after(from, PAGE)?;
                for r in &page {
                    from = r.seq;
                    if !asked.types.is_empty() && !asked.types.contains(&r.event.kind) {
                        continue;
                    }
                    let line = json!({
                        "id": r.event.id,
                        "type": r.event.kind,
                        "time": r.event.time,
                        "source": r.event.source,
                        "data": r.event.data,
                    })
                    .to_string()
                        + "\n";
                    if !sent.send(tx, line.into()) {
                        return Ok(());
                    }
                }
                if page.len() < PAGE {
                    break;
                }
            }
        }
        std::thread::sleep(asked.every);
    }
    Ok(())
}

/// The new lines of the workspace's log, as `asked`: from the log's end
/// now, or, when there is no log yet, all of the one that comes.
pub fn log(ws: &Workspace, asked: Asked, streams: Arc<Streams>) -> Result<Follow, Refused> {
    let path = ws.root.join(".itero").join("itero.log");
    let open = match std::fs::File::open(&path) {
        Ok(mut f) => {
            f.seek(std::io::SeekFrom::End(0))
                .map_err(|e| Refused::Internal(e.into()))?;
            Some(f)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Refused::Internal(e.into())),
    };
    start(streams, Kind::Log, move |tx| {
        let mut l = LogLook {
            path,
            open,
            rolled: None,
            half: Vec::new(),
            sent: Sent {
                n: 0,
                count: asked.count,
            },
        };
        if let Err(e) = l.run(&asked, &tx) {
            eprintln!("itero: following the log stopped: {e:#}");
        }
    })
}

/// A log followed: the file read, and the one rolled away from under it.
struct LogLook {
    path: PathBuf,
    open: Option<std::fs::File>,
    /// The file before a roll, read once more at the next look: a line
    /// its writer opened it for just before the roll ends there.
    rolled: Option<std::fs::File>,
    /// The start of a line not yet ended.
    half: Vec<u8>,
    sent: Sent,
}

impl LogLook {
    fn run(&mut self, asked: &Asked, tx: &mpsc::Sender<Bytes>) -> anyhow::Result<()> {
        while !tx.is_closed() {
            if !self.look(tx)? {
                return Ok(());
            }
            std::thread::sleep(asked.every);
        }
        Ok(())
    }

    /// One look; false when the stream is to end.
    fn look(&mut self, tx: &mpsc::Sender<Bytes>) -> anyhow::Result<bool> {
        if let Some(mut old) = self.rolled.take()
            && !self.read(&mut old, tx)?
        {
            return Ok(false);
        }
        if self.open.is_none() {
            match std::fs::File::open(&self.path) {
                Ok(f) => self.open = Some(f),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
                Err(e) => return Err(e.into()),
            }
        }
        let mut f = self.open.take().expect("opened");
        if !self.read(&mut f, tx)? {
            return Ok(false);
        }
        // rolled: the path names another file now, or none
        use std::os::unix::fs::MetadataExt;
        let now = match std::fs::metadata(&self.path) {
            Ok(m) => Some(m.ino()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if now == Some(f.metadata()?.ino()) {
            self.open = Some(f);
        } else {
            self.rolled = Some(f);
        }
        Ok(true)
    }

    /// Sends the whole lines `f` has from where it was read to; false when
    /// the stream is to end.
    fn read(&mut self, f: &mut std::fs::File, tx: &mpsc::Sender<Bytes>) -> anyhow::Result<bool> {
        let mut more = Vec::new();
        f.read_to_end(&mut more)?;
        self.half.extend_from_slice(&more);
        while let Some(end) = self.half.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.half.drain(..=end).collect();
            if !self.sent.send(tx, line.into()) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn every_problem_is_said_at_once() {
        let why = asked(
            &q(&[
                ("every", "1h"),
                ("count", "0"),
                ("type", "tek"),
                ("bilinmeyen", "1"),
                ("after", "a"),
                ("after", "b"),
                ("every", "yavaş"),
            ]),
            Kind::Events,
        )
        .unwrap_err();
        let problems: Vec<&str> = why.split("; ").collect();
        assert_eq!(
            problems,
            [
                "every is from 10ms to 1m, not 1h",
                "count is a whole number from 1, not \"0\"",
                "type: an event type is two or more words joined by dots, like order.placed, not tek",
                "bilinmeyen is not a parameter of /events/stream: every, after, type, count",
                "after is given once",
                "every is given once",
            ]
        );
    }

    #[test]
    fn the_log_takes_every_and_count_only() {
        let why = asked(
            &q(&[("after", "x"), ("type", "a.b"), ("type", "c.d")]),
            Kind::Log,
        )
        .unwrap_err();
        assert_eq!(
            why,
            "after is for /events/stream only; type is for /events/stream only"
        );
        let a = asked(&q(&[("every", "250ms"), ("count", "3")]), Kind::Log).unwrap();
        assert_eq!(a.every, Duration::from_millis(250));
        assert_eq!(a.count, Some(3));
    }

    #[test]
    fn nothing_asked_is_every_event_from_now_each_100ms() {
        let a = asked(&[], Kind::Events).unwrap();
        assert_eq!(
            a,
            Asked {
                every: Duration::from_millis(100),
                after: After::Now,
                types: vec![],
                count: None
            }
        );
        let a = asked(
            &q(&[
                ("type", "sipariş.verildi"),
                ("type", "kapı.açıldı"),
                ("after", "01a"),
            ]),
            Kind::Events,
        )
        .unwrap();
        assert_eq!(a.types, ["sipariş.verildi", "kapı.açıldı"]);
        assert_eq!(a.after, After::Event("01a".into()));
    }
}
