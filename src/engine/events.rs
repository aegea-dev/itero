//! Messages from outside (an app, a script, a person): checked, then
//! written to the journal and nothing more. The daemon's next pass takes
//! them from there, so a message sent while it is stopped waits for it.
use super::journal::{self, Envelope, Journal};
use crate::workspace::Workspace;
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum EmitError {
    /// The message itself is wrong.
    Bad(String),
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EmitError::Bad(m) | EmitError::Internal(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EmitError {}

/// The start of the types of the daemon's own messages, `itero.run.error`:
/// workflows may run on them, and only the daemon sends them.
pub const OWN: &str = "itero.";

/// Types the engine sends itself: from outside they would forge a worker's
/// result, a timer, a run's start, a connector's state or the daemon's own
/// message.
pub fn engine_own(kind: &str) -> bool {
    ["action.", "timer.", "run.", "workflow.", "connector.", OWN]
        .iter()
        .any(|p| kind.starts_with(p))
}

/// A watched path changed: `{"path", "kind"}`. It starts the workflows
/// whose `on` names the path.
pub const PATH_CHANGED: &str = "itero.path.changed";

/// A click on a notification `#notify` showed: `{"notification", "answer",
/// "workflow", "data"}`, saved by Itero.app through `itero
/// notification-clicked`. Itero's own, so nothing from outside fakes one.
pub const NOTIFICATION_CLICKED: &str = "itero.notification.clicked";

/// Saves a click on a notification for the workspace; its data must name
/// the notification. Returns the message's id.
pub fn notification_clicked(ws: &Workspace, data: Value, now: &str) -> anyhow::Result<String> {
    if !data.get("notification").is_some_and(Value::is_string) {
        anyhow::bail!("a click names its notification: {{\"notification\": \"<id>\", ...}}");
    }
    let e = own(NOTIFICATION_CLICKED, data, now);
    Journal::open(ws)?.publish(&e)?;
    Ok(e.id)
}

/// Tells the running daemon, if there is one, that a message waits in the
/// journal: for the processes that are not the daemon (`itero emit`, a
/// click). A daemon that is not running finds it when it starts.
pub fn nudge(ws: &Workspace, now: &str) -> std::io::Result<()> {
    std::fs::write(ws.wake_file(), now)
}

/// One of the daemon's own messages, for the workspace.
pub fn own(kind: &str, data: Value, now: &str) -> Envelope {
    debug_assert!(kind.starts_with(OWN));
    Envelope::new("daemon", kind, &journal::subject("app"), now, None, data)
}

/// What a message will do, as the workflows on disk say: the workflows it
/// starts, how many runs wait (it may reach them), and, when nothing
/// starts on its type, the nearest type something does start on.
#[derive(Debug, Default, PartialEq)]
pub struct Fate {
    pub starts: Vec<String>,
    pub waiting: usize,
    pub nearest: Option<String>,
}

pub fn fate(ws: &Workspace, kind: &str, data: &Map<String, Value>) -> anyhow::Result<Fate> {
    let now = chrono::Utc::now();
    let defined = |n: &str| super::secrets::get(n).is_some();
    let flows = ws.flows()?;
    let starts = flows
        .iter()
        .filter(|(_, f)| {
            f.on.events.iter().any(|e| e == kind)
                && crate::flow::active(&f.meta, now)
                && crate::flow::holds(&f.on, data, &defined)
        })
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>();
    let waiting = Journal::open(ws)?
        .active_runs()?
        .iter()
        .filter(|r| super::steps::is_waiting(&r.runtime))
        .count();
    // a type a letter or three away is a typo
    let nearest = if starts.is_empty() {
        flows
            .iter()
            .flat_map(|(_, f)| f.on.events.iter())
            .map(|e| (distance(e, kind), e))
            .filter(|(d, _)| (1..=3).contains(d))
            .min()
            .map(|(_, e)| e.clone())
    } else {
        None
    };
    Ok(Fate {
        starts,
        waiting,
        nearest,
    })
}

/// The edits (a letter added, removed or changed) from `a` to `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// How waiting for a message's answer ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Waited {
    /// Every run it started ended, none failed or in error.
    Done,
    /// They all ended; one failed or ended in error.
    Failed,
    /// Time ran out: the answer has the runs so far.
    TimedOut,
}

/// Waits up to `timeout` for the runs `message` started to end, and
/// answers `{"message", "runs": [...]}`, each run as `journal::answer`
/// says. A message that starts nothing is answered once a daemon has
/// taken it.
pub fn wait_for(
    ws: &Workspace,
    message: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<(Value, Waited)> {
    let j = Journal::open(ws)?;
    let end = std::time::Instant::now() + timeout;
    loop {
        let runs = j.runs_of(message)?;
        let ended = runs
            .as_ref()
            .is_some_and(|rs| rs.iter().all(|r| r.ended.is_some()));
        if ended || std::time::Instant::now() >= end {
            let runs = runs.unwrap_or_default();
            let failed = runs
                .iter()
                .any(|r| matches!(super::steps::status(&r.runtime), "failed" | "error"));
            let answer = serde_json::json!({
                "message": message,
                "runs": runs.iter().map(|r| journal::answer(r, &r.runtime)).collect::<Vec<_>>(),
            });
            let how = match (ended, failed) {
                (false, _) => Waited::TimedOut,
                (true, true) => Waited::Failed,
                (true, false) => Waited::Done,
            };
            return Ok((answer, how));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Saves an event for the workspace, where it may start runs and reach
/// waiting ones; returns its id.
pub fn emit(
    ws: &Workspace,
    kind: &str,
    data: Option<Value>,
    now: &str,
) -> Result<String, EmitError> {
    if !crate::flow::is_message_type(kind) {
        return Err(EmitError::Bad(format!(
            "an event type is two or more words joined by dots, like order.placed, not {kind}"
        )));
    }
    if engine_own(kind) {
        return Err(EmitError::Bad(format!(
            "{kind} is the engine's own: it cannot be sent from outside"
        )));
    }
    let data = match data {
        None => Value::Object(Map::new()),
        Some(v @ Value::Object(_)) => v,
        Some(_) => return Err(EmitError::Bad("data must be a JSON object".into())),
    };
    let internal = |e: anyhow::Error| EmitError::Internal(format!("{e:#}"));
    let j = Journal::open(ws).map_err(internal)?;
    let e = Envelope::new("outside", kind, &journal::subject("app"), now, None, data);
    j.publish(&e).map_err(internal)?;
    Ok(e.id)
}
