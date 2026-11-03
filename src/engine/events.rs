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
    /// There is no workspace to send it to.
    Nowhere(String),
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EmitError::Bad(m) | EmitError::Nowhere(m) | EmitError::Internal(m) => f.write_str(m),
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

/// itero-ai, the local model's server, answers on its socket:
/// `{"socket", "seconds"}`, how long it took from its start.
pub const AI_STARTED: &str = "itero.ai.started";
/// A start that never answered: `{"why", "seconds"}`, `seconds` when one
/// was started (not when it waited for its turn, or could not be run).
pub const AI_START_FAILED: &str = "itero.ai.start.failed";
/// It exited idle: `{"after"}`, its idle seconds.
pub const AI_STOPPED: &str = "itero.ai.stopped";
/// It ended any other way: `{"exit", "why"}`, its last line on stderr.
pub const AI_FAILED: &str = "itero.ai.failed";
/// It loaded a model: `{"model", "seconds"}`.
pub const AI_MODEL_LOADED: &str = "itero.ai.model.loaded";
/// An `#ai` step named a model the machine's index has not:
/// `{"model", "workflow"}`. The `models` workflow looks for models on it.
pub const AI_MODEL_MISSING: &str = "itero.ai.model.missing";

/// A workspace of the machine's daemon has a pass running past its limit:
/// `{"workspace", "since"}`. Told to every workspace the daemon runs.
pub const WORKSPACE_STUCK: &str = "itero.workspace.stuck";
/// A workspace's loop kept failing and was given up on:
/// `{"workspace", "why"}`. Told to every workspace the daemon runs.
pub const WORKSPACE_FAILED: &str = "itero.workspace.failed";

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
/// journal: for the processes that are not the daemon (`itero ws emit`, a
/// click). A daemon that is not running finds it when it starts.
pub fn nudge(ws: &Workspace, now: &str) -> std::io::Result<()> {
    std::fs::write(ws.wake_file(), now)
}

/// What asking a run to stop did.
#[derive(Debug, Clone, PartialEq)]
pub enum StopAsked {
    /// Asked: the daemon running the workspace stops it now, or (`daemon`
    /// false) the next to start does.
    Asked { name: String, daemon: bool },
    /// It has ended already, so.
    Ended { name: String, status: String },
    /// No run has that id or name.
    Missing,
}

/// Asks run `run` (an id or a name; of two with one name, the newest) to
/// stop: a `run.stop` event on it, which the daemon handles in order with
/// the run's others, and a nudge to a running daemon.
pub fn stop_run(ws: &Workspace, run: &str, now: &str) -> anyhow::Result<StopAsked> {
    let j = Journal::open(ws)?;
    let Some(r) = j.run_by(run)?.into_iter().next() else {
        return Ok(StopAsked::Missing);
    };
    if r.runtime.ended.is_some() {
        return Ok(StopAsked::Ended {
            name: r.name,
            status: super::steps::status(&r.runtime).into(),
        });
    }
    j.publish(&Envelope::new(
        "outside",
        "run.stop",
        &journal::subject(&r.id),
        now,
        None,
        Value::Object(Map::new()),
    ))?;
    let daemon = super::lock::held(ws).map_err(anyhow::Error::msg)?;
    if daemon {
        nudge(ws, now)?;
    }
    Ok(StopAsked::Asked {
        name: r.name,
        daemon,
    })
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
    let defined = |n: &str| super::secrets::get_in(ws, n).is_some();
    let flows = ws.flows()?;
    let starts = flows
        .iter()
        .filter(|(_, f)| {
            f.on.events.iter().any(|e| e.kind == kind)
                && crate::flow::active(&f.meta, now)
                && crate::flow::holds(
                    &f.on,
                    &super::steps::first_state(
                        Value::Object(data.clone()),
                        ws.manifest().unwrap_or_default(),
                    ),
                    &defined,
                )
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
            .map(|e| (distance(&e.kind, kind), &e.kind))
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

/// Whether an event may be sent from outside: its data, an object.
pub fn check(kind: &str, data: Option<Value>) -> Result<Value, EmitError> {
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
    match data {
        None => Ok(Value::Object(Map::new())),
        Some(v @ Value::Object(_)) => Ok(v),
        Some(_) => Err(EmitError::Bad("data must be a JSON object".into())),
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
    let data = check(kind, data)?;
    let j = Journal::open(ws).map_err(|e| EmitError::Internal(format!("{e:#}")))?;
    emit_to(&j, kind, data, now)
}

/// `emit`, into a journal already open, of an event `check` passed.
pub fn emit_to(j: &Journal, kind: &str, data: Value, now: &str) -> Result<String, EmitError> {
    let e = Envelope::new("outside", kind, &journal::subject("app"), now, None, data);
    j.publish(&e)
        .map_err(|e| EmitError::Internal(format!("{e:#}")))?;
    Ok(e.id)
}
