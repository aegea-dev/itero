//! Runs one source for the daemon: starts it with its settings, turns its
//! lines into events for the workspace, keeps its cursor, and starts it
//! again when it ends, waiting longer each time; one that keeps ending is
//! given up on, and the log and `/health` say so.
use super::{Definition, Settings};
use crate::engine::daemon::{Health, SourceHealth, stamp};
use crate::engine::journal::{self, Envelope, Journal};
use crate::engine::worker;
use crate::workspace::Workspace;
use serde_json::{Map, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// When a source is started again, and when it is given up on.
pub struct Policy {
    /// The first wait before starting it again; it doubles each time.
    pub first_pause: Duration,
    pub max_pause: Duration,
    /// A run this long sets the wait back to `first_pause`.
    pub calm_after: Duration,
    /// Started again at most this many times within `window`; the next end
    /// gives it up.
    pub give_up_after: usize,
    pub window: Duration,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            first_pause: Duration::from_secs(1),
            max_pause: Duration::from_secs(60),
            calm_after: Duration::from_secs(300),
            give_up_after: 10,
            window: Duration::from_secs(600),
        }
    }
}

/// A source of the workspace: its name, its folder, its definition.
pub struct Source<'a> {
    pub name: &'a str,
    pub folder: &'a Path,
    pub def: &'a Definition,
}

/// One line a source printed.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Event { kind: String, data: Value },
    Cursor(Value),
}

/// Reads one line of a source's output; `emit` is the type of an event
/// that names none.
pub fn parse_line(line: &str, emit: Option<&str>) -> Result<Line, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not JSON ({e}): {line}"))?;
    let Value::Object(mut o) = v else {
        return Err(format!("not a JSON object: {line}"));
    };
    if !o.contains_key("type")
        && let Some(c) = o.remove("cursor")
    {
        return Ok(Line::Cursor(c));
    }
    let kind = match o.remove("type") {
        Some(Value::String(t)) => t,
        Some(other) => return Err(format!("type is text, not {other}")),
        None => emit
            .map(str::to_string)
            .ok_or("no type, and its file sets no emit")?,
    };
    if !kind.split('.').all(crate::workflow::validate::is_word) {
        return Err(format!(
            "\"{kind}\" is not an event type, like mail.changed"
        ));
    }
    if crate::engine::events::engine_own(&kind) {
        return Err(format!("\"{kind}\" is the engine's own"));
    }
    let data = match o.remove("data") {
        None => Value::Object(Map::new()),
        Some(d @ Value::Object(_)) => d,
        Some(_) => return Err("data is an object".into()),
    };
    Ok(Line::Event { kind, data })
}

fn set(health: &Mutex<Health>, name: &str, f: impl FnOnce(&mut SourceHealth)) {
    let mut h = health.lock().unwrap_or_else(PoisonError::into_inner);
    let s = h.sources.entry(name.to_string()).or_insert(SourceHealth {
        state: "running".into(),
        restarts: 0,
        last_exit: None,
    });
    f(s);
}

/// A source's container: one per workspace and source.
fn container_name(ws: &Workspace, name: &str) -> String {
    worker::container_name(&format!("source:{}:{name}", ws.root.display()))
}

/// The command that runs a source: `itero source <built-in>`; its program,
/// a path with `/` being in its folder; or its program in a container.
fn command(ws: &Workspace, exe: &Path, s: &Source) -> Command {
    let mut c = match (&s.def.source, &s.def.image) {
        (Some(built_in), _) => {
            let mut c = Command::new(exe);
            c.args(["source", built_in]);
            c
        }
        (None, Some(image)) => {
            let name = container_name(ws, s.name);
            // one left by a daemon that stopped goes first
            worker::remove_named_container(&name);
            let mut c = Command::new(worker::CONTAINER_RUNTIME);
            c.args(worker::container_args(&worker::Contained {
                name: &name,
                label: "dev.aegea.itero.source",
                folder: s.folder,
                mount: "/source",
                image,
                env: vec![],
                program: &s.def.program,
            }));
            c
        }
        (None, None) => {
            let p = &s.def.program[0];
            let mut c = Command::new(if p.contains('/') {
                s.folder.join(p)
            } else {
                p.into()
            });
            c.args(&s.def.program[1..]);
            c
        }
    };
    c.current_dir(s.folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    c
}

/// One start of the source, until it ends: how it ended.
fn once(ws: &Workspace, exe: &Path, s: &Source, wake: &Sender<()>) -> anyhow::Result<String> {
    let j = Journal::open(ws)?;
    let with = crate::engine::daemon::fill_secrets(&Value::Object(s.def.with.clone()))
        .map_err(|e| anyhow::anyhow!(e))?;
    let settings = Settings {
        name: s.name.to_string(),
        with: with.as_object().cloned().unwrap_or_default(),
        emit: s.def.emit.clone(),
        cursor: j.cursor(s.name)?,
    };
    let mut child = command(ws, exe, s).spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    // a source that reads nothing may be gone already: its end says why
    let _ = writeln!(stdin, "{}", serde_json::to_string(&settings)?);
    // stdin stays open while it runs: its closing tells the source that
    // the daemon has gone
    let stdout = child.stdout.take().expect("piped");
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(&line, s.def.emit.as_deref()) {
            Ok(Line::Event { kind, data }) => {
                let e = Envelope::new(
                    &format!("source/{}", s.name),
                    &kind,
                    &journal::subject("app"),
                    &stamp(chrono::Utc::now()),
                    None,
                    data,
                );
                j.publish(&e)?;
                let _ = wake.send(());
            }
            Ok(Line::Cursor(c)) => j.set_cursor(s.name, &c)?,
            Err(why) => eprintln!("source {}: {why}", s.name),
        }
    }
    drop(stdin);
    Ok(child.wait()?.to_string())
}

/// Runs a source until it is given up on.
pub fn run(
    ws: &Workspace,
    exe: &Path,
    s: &Source,
    policy: &Policy,
    wake: &Sender<()>,
    health: &Mutex<Health>,
) {
    let mut ends: Vec<Instant> = vec![];
    let mut pause = Duration::ZERO;
    loop {
        let began = Instant::now();
        set(health, s.name, |h| {
            h.state = "running".into();
            h.restarts = ends.len();
        });
        let ended = match once(ws, exe, s, wake) {
            Ok(status) => status,
            Err(e) => format!("error: {e:#}"),
        };
        let ran = began.elapsed();
        eprintln!(
            "source {}: ended ({ended}) after {}s",
            s.name,
            ran.as_secs()
        );
        ends.retain(|t| t.elapsed() < policy.window);
        ends.push(Instant::now());
        let restarts = ends.len() - 1;
        if restarts >= policy.give_up_after {
            eprintln!(
                "source {}: gave up: started again {restarts} times in {}s",
                s.name,
                policy.window.as_secs()
            );
            set(health, s.name, |h| {
                h.state = "gave up".into();
                h.restarts = restarts;
                h.last_exit = Some(ended);
            });
            return;
        }
        pause = if ran >= policy.calm_after {
            policy.first_pause
        } else {
            (pause * 2).clamp(policy.first_pause, policy.max_pause)
        };
        set(health, s.name, |h| {
            h.state = "waiting".into();
            h.restarts = restarts;
            h.last_exit = Some(ended);
        });
        std::thread::sleep(pause);
    }
}
