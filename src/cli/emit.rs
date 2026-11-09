//! `itero ws emit` and `itero emit`: an event for one workspace, or for
//! every workspace on the machine's list.
use super::EmitArgs;
use super::out::show;
use crate::engine::daemon::stamp;
use crate::engine::events::{self, Waited};
use crate::engine::machine;
use crate::machine::{List, list_path};
use crate::workspace::Workspace;
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

/// The data in `--file`, or on stdin when that is `-`.
fn data_in(path: &Path) -> Result<Value> {
    let (name, text) = if path == Path::new("-") {
        (
            "stdin".to_string(),
            std::io::read_to_string(std::io::stdin()),
        )
    } else {
        (path.display().to_string(), std::fs::read_to_string(path))
    };
    let text = text.map_err(|e| anyhow!("{name}: {e}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| anyhow!("{name}: not JSON: {e}"))?;
    if !value.is_object() {
        bail!("{name}: the data is a JSON object");
    }
    Ok(value)
}

/// The event's data, as given.
fn data(args: &EmitArgs) -> Result<Option<Value>> {
    if let Some(path) = &args.file {
        return data_in(path).map(Some);
    }
    args.data
        .as_deref()
        .map(|d| serde_json::from_str(d).map_err(|_| anyhow!("the data is a JSON object")))
        .transpose()
}

/// How long `--wait` waits, when it is given.
fn wait(args: &EmitArgs) -> Result<Option<Duration>> {
    if !args.wait {
        return Ok(None);
    }
    humantime::parse_duration(&args.timeout)
        .map(Some)
        .map_err(|_| {
            anyhow!(
                "--timeout: a duration is like 30s or 5m, not {}",
                args.timeout
            )
        })
}

/// `itero ws emit`: saved for the workspace, to wait for its daemon if none
/// runs.
pub fn one(ws: &Workspace, args: EmitArgs) -> Result<()> {
    use crate::answer::events::{Emit, Emitted};
    let kind = &args.kind;
    let e = Emit {
        kind: kind.clone(),
        data: data(&args)?,
        wait: wait(&args)?,
    };
    let mut sent = match crate::answer::Client::Local(ws.clone()).emit(&e)? {
        Emitted::Waited(w) => {
            println!("{}", serde_json::to_string_pretty(&w)?);
            match w.how() {
                Waited::Done => return Ok(()),
                Waited::Failed => std::process::exit(1),
                Waited::TimedOut => {
                    eprintln!("timed out: not every run it started has ended");
                    std::process::exit(2);
                }
            }
        }
        Emitted::Sent(sent) => sent,
    };
    // none holds it: what this machine's daemon, if one answers, says of it
    let machine = if sent.daemon.running {
        None
    } else {
        super::workspace::machine_says(&ws.root)
    };
    if let Some(m) = &machine {
        sent.daemon.machine = Some(json!({
            "state": m.state.as_ref().map(|(s, _)| s),
            "why": m.state.as_ref().and_then(|(_, w)| w.as_ref()),
            "itero": m.itero,
        }));
    }
    let id = sent.id.clone();
    show(args.json, &serde_json::to_value(&sent)?, |_| {
        println!("{id}");
        // what it will do, on stderr: stdout stays the id for scripts
        if !sent.starts.is_empty() {
            eprintln!("starts: {}", sent.starts.join(", "));
        }
        if sent.waiting > 0 {
            eprintln!("may reach {} waiting run(s)", sent.waiting);
        }
        if sent.starts.is_empty() {
            match &sent.nearest {
                Some(n) => eprintln!("nothing starts on {kind}: did you mean {n}?"),
                None => eprintln!("nothing starts on {kind}"),
            }
        }
        match (&machine, sent.daemon.running) {
            (_, true) => {}
            (Some(m), false) => {
                for l in m.lines(m.name.as_deref().unwrap_or("this workspace")) {
                    eprintln!("{l}");
                }
            }
            (None, false) => eprintln!("no daemon is running: it waits for itero daemon"),
        }
    })
}

/// `itero emit`: saved for every workspace on the machine's list.
pub fn all(args: EmitArgs) -> Result<()> {
    let data = data(&args)?;
    let timeout = wait(&args)?;
    let list = List::read(&list_path()?)?;
    if list.workspaces.is_empty() {
        bail!("{}", machine::EMPTY_LIST);
    }
    if timeout.is_some() {
        // with no daemon nothing would answer: say so, sending nothing. One
        // that cannot be asked is left to say why when it is sent to.
        let idle: Vec<&str> = std::thread::scope(|s| {
            let asked: Vec<_> = list
                .workspaces
                .iter()
                .map(|e| {
                    s.spawn(move || match machine::open(e) {
                        Ok((ws, _)) => matches!(super::daemon_running(&ws), Ok(false)),
                        Err(_) => false,
                    })
                })
                .collect();
            list.workspaces
                .iter()
                .zip(asked)
                .filter_map(|(e, a)| a.join().unwrap_or(false).then_some(e.name.as_str()))
                .collect()
        });
        if !idle.is_empty() {
            bail!(
                "no daemon runs {}: start one (itero service start), then send it again",
                idle.join(", ")
            );
        }
    }
    let sent = machine::emit_all(&args.kind, data, &stamp(chrono::Utc::now()))?;
    match timeout {
        Some(timeout) => wait_for_all(sent, timeout),
        None => said(sent, args.json),
    }
}

/// Exit 1 when the event reached no workspace, or one could not save it;
/// a workspace missing from its folder is said, and fails nothing.
fn failed(sent: &[machine::Sent]) -> bool {
    use machine::To;
    sent.iter().any(|s| matches!(s.to, To::Failed(_)))
        || !sent.iter().any(|s| matches!(s.to, To::Sent(..)))
}

/// Prints where the event went, one workspace a line, and why it did not
/// reach any that could not take it.
fn said(sent: Vec<machine::Sent>, json_out: bool) -> Result<()> {
    use machine::To;
    let failed = failed(&sent);
    let rows: Vec<Value> = sent
        .into_iter()
        .map(|s| match s.to {
            To::Sent(_, id) => json!({ "workspace": s.name, "id": id }),
            To::Missing(why) => json!({ "workspace": s.name, "missing": why }),
            To::Failed(why) => json!({ "workspace": s.name, "error": why }),
        })
        .collect();
    show(json_out, &Value::Array(rows), |v| {
        for r in v.as_array().into_iter().flatten() {
            let name = r["workspace"].as_str().unwrap_or_default();
            match (r["id"].as_str(), r["missing"].as_str()) {
                (Some(id), _) => println!("{name} {id}"),
                (None, Some(why)) => eprintln!("{name}: {why}"),
                (None, None) => eprintln!("{name}: {}", super::out::text(&r["error"])),
            }
        }
    })?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

/// Waits for the runs the event started in each workspace, in turn, with
/// what time is left, and prints them by workspace; exit 1 when one failed,
/// could not take the event or none could, 2 when time ran out.
fn wait_for_all(sent: Vec<machine::Sent>, timeout: Duration) -> Result<()> {
    use machine::To;
    let end = Instant::now() + timeout;
    let mut answers = Map::new();
    let mut code = if failed(&sent) { 1 } else { 0 };
    for s in sent {
        let answer = match s.to {
            To::Sent(ws, id) => {
                let left = end.saturating_duration_since(Instant::now());
                let (answer, how) = events::wait_for(&ws, &id, left)?;
                code = code.max(match how {
                    Waited::Done => 0,
                    Waited::Failed => 1,
                    Waited::TimedOut => 2,
                });
                answer
            }
            To::Missing(why) => {
                eprintln!("{}: {why}", s.name);
                continue;
            }
            To::Failed(why) => json!({ "error": why }),
        };
        answers.insert(s.name, answer);
    }
    println!("{}", serde_json::to_string_pretty(&answers)?);
    if code == 2 {
        eprintln!("timed out: not every run it started has ended");
    }
    if code > 0 {
        std::process::exit(code);
    }
    Ok(())
}
