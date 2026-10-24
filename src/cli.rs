//! The command line.
use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "itero",
    version,
    about = "Runs workflows again and again: schedules, events, steps, workers"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the workspace's workflows: their schedules, events and jobs, pass after pass
    Daemon {
        /// One pass, then stop: for tests and scripts, so hidden
        #[arg(long, hide = true)]
        once: bool,
        /// The time to run at, with --once (RFC 3339; for tests)
        #[arg(long, requires = "once", hide = true)]
        now: Option<String>,
        /// Seconds between passes
        #[arg(long, default_value_t = 1)]
        interval: u64,
        /// The passes only: no HTTP API
        #[arg(long)]
        no_serve: bool,
    },
    /// Check every workflow, action and connector; print each
    Check,
    /// Send a message: it starts runs and reaches the runs waiting for it
    Emit {
        /// Its type, two or more words joined by dots, like order.placed
        #[arg(value_name = "TYPE")]
        kind: String,
        /// Its data, a JSON object
        #[arg(value_name = "DATA")]
        data: Option<String>,
        /// Wait for the runs it starts to end, and print them as JSON (exit 1: one failed or broke; 2: time ran out)
        #[arg(long)]
        wait: bool,
        /// With --wait: how long, like 30s or 5m
        #[arg(long, default_value = "60s", requires = "wait")]
        timeout: String,
        /// Post each run's end to this URL, when it ends
        #[arg(long, value_name = "URL")]
        reply: Option<String>,
    },
    /// Set an action's args (a JSON object, or a list of objects: its step runs once per item), or a connector's (an object)
    Args {
        /// The action, without # or a version; or the connector
        name: String,
        /// The args, as JSON
        #[arg(
            value_name = "DATA",
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        data: Option<String>,
        /// Read the args from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Set a workflow's settings, or show them: a JSON object its runs start with
    Config {
        /// The workflow
        workflow: String,
        /// The settings, as JSON; without them, they are shown
        #[arg(value_name = "DATA", conflicts_with = "file")]
        data: Option<String>,
        /// Read the settings from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Keep a secret in the OS secret store, for $$NAME (asks for it, without echo)
    Secret {
        /// Its name: a letter or _, then letters, digits or _
        name: String,
        /// Read it from this file (its last newline left out)
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// The runs, newest first: id, name, workflow, where it is or how it ended, started, ended
    Runs {
        /// One run, by id or name: what happened, step by step
        run: Option<String>,
        /// The internal workflows' runs too
        #[arg(long)]
        all: bool,
    },
    /// The workspace: its identity, meta and limits, and the daemon running it
    Info,
    /// Each workflow: what starts it, its runs and the last; and whether a daemon takes the messages
    Workflows,
    /// Where a workflow, action or connector of this name is (#name: the action only)
    Which { name: String },
    /// Runs that ended in error, newest first: when, the workflow, the run, the step and why
    Errors {
        /// How many
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Start this workspace's daemon at login, kept up by a watchdog
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// The workflow manager: one decision, JSON in on stdin, JSON out on stdout
    #[command(hide = true)]
    Manager,
    /// A worker: one job, JSON in on stdin, its output as JSON on stdout
    #[command(hide = true)]
    Worker,
    /// A click on a notification itero showed, as Itero.app tells it
    #[command(hide = true)]
    NotificationClicked {
        /// The click, as JSON: {"notification", "answer", "workflow", "data"}
        data: String,
    },
    /// For connectors: print a line each time a file or folder changes
    Watch { path: std::path::PathBuf },
    /// Keeps `itero daemon` running: restarts it when it exits, stops
    /// answering or stops making passes (what the service runs)
    #[command(hide = true)]
    Watchdog {
        /// Seconds between the daemon's passes
        #[arg(long, default_value_t = 1)]
        interval: u64,
        /// Seconds between checks of the daemon's /health
        #[arg(long, default_value_t = 10)]
        check_every: u64,
        /// Checks in a row without an answer before a restart
        #[arg(long, default_value_t = 3)]
        misses: u32,
        /// Seconds without a pass before the daemon counts as hung
        #[arg(long, default_value_t = 600)]
        stale_after: u64,
        /// Where its lines and the daemon's stderr go
        #[arg(long)]
        log: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ServiceCmd {
    /// Run the daemon as a service (per user, at login), kept up by a watchdog
    Install {
        /// Seconds between passes
        #[arg(long, default_value_t = 1)]
        interval: u64,
    },
    /// Stop and remove the service
    Uninstall,
    /// The service's state and the daemon's health
    Status,
}

/// Lists a command's subcommands, and theirs, by name in help: one display
/// order for all, which clap breaks by name.
fn by_name(c: clap::Command) -> clap::Command {
    c.mut_subcommands(|s| by_name(s.display_order(0)))
}

/// Runs the command line; returns the exit code.
pub fn run(args: Vec<String>) -> i32 {
    use clap::{CommandFactory, FromArgMatches};
    let parsed = by_name(Cli::command())
        .try_get_matches_from(args)
        .and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    match dispatch(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

/// What a pass did, on stderr when it is a terminal: only when it did
/// something, or failed. A pass that failed is in the log too.
fn report(pass: &Result<crate::engine::daemon::Pass>, log: Option<&crate::engine::log::Log>) {
    use std::io::IsTerminal;
    if let (Err(e), Some(log)) = (pass, log) {
        log.write("error", serde_json::json!({ "why": format!("{e:#}") }));
    }
    if !std::io::stderr().is_terminal() {
        return;
    }
    match pass {
        Ok(p) if p.delivered + p.worked + p.lost + p.deferred > 0 => eprintln!(
            "{} events, {} jobs, {} lost (run again), {} waiting",
            p.delivered, p.worked, p.lost, p.deferred
        ),
        Ok(_) => {}
        Err(e) => eprintln!("error: {e:#}"),
    }
}

/// Set by SIGINT or SIGTERM: the daemon is asked to stop.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// When the daemon is asked to stop (SIGINT, SIGTERM), logs
/// `daemon.stopped` and ends the process.
fn stop_on_signal(log: crate::engine::log::Log) {
    #[cfg(unix)]
    // SAFETY: the handler only stores to an atomic
    unsafe {
        let handler = on_stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    std::thread::spawn(move || {
        while !STOP.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        log.write("daemon.stopped", serde_json::json!({}));
        std::process::exit(0);
    });
}

/// Ignores SIGPIPE again, for the commands that write to other processes
/// (the daemon to its manager and workers, a worker to its action, the
/// watchdog to the daemon): one that has gone is a write error, already
/// handled, not a reason to die. `main` restores the default for the
/// rest, so `itero runs | head` ends quietly.
fn ignore_sigpipe() {
    #[cfg(unix)]
    // SAFETY: before this command starts any thread or child
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

/// The workspace's daemon's `/health`, when one answers on `port`.
fn daemon_health(port: u16) -> Option<serde_json::Value> {
    crate::net::client(std::time::Duration::from_secs(3))
        .ok()?
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .and_then(|r| r.json())
        .ok()
}

/// Whether this workspace's own daemon answers on its port.
fn daemon_running(
    ws: &crate::workspace::Workspace,
    j: &crate::engine::journal::Journal,
) -> Result<bool> {
    let me = j.workspace()?;
    Ok(ws
        .daemon_port()
        .and_then(daemon_health)
        .is_some_and(|h| h["workspace"]["id"] == me.id.as_str()))
}

/// How long before `now` an RFC 3339 time was: `2m ago`, `3h ago`.
fn ago(at: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = at.parse::<chrono::DateTime<chrono::Utc>>() else {
        return at.to_string();
    };
    let s = (now - t).num_seconds().max(0);
    match s {
        0..60 => format!("{s}s ago"),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

/// A run, from the journal: how it started, each step and how it went,
/// the messages it took, and its state.
fn story(j: &crate::engine::journal::Journal, r: &crate::engine::journal::Run) -> Result<()> {
    use crate::engine::journal::subject;
    println!(
        "{}  {}  {}  {}",
        r.name,
        r.id,
        r.workflow,
        crate::engine::steps::status(&r.runtime)
    );
    // each result's cause is its job, which names the action
    let jobs: std::collections::BTreeMap<String, String> = j
        .events(Some(&subject("worker")))?
        .into_iter()
        .filter(|e| e.event.data["run"] == r.id.as_str())
        .map(|e| {
            let action = e.event.data["job"]["action"].as_str().unwrap_or("?");
            (e.event.id, format!("#{action}"))
        })
        .collect();
    for e in j.events(Some(&subject(&r.id)))? {
        let d = &e.event.data;
        let step = || {
            let name = e
                .event
                .cause
                .as_ref()
                .and_then(|c| jobs.get(c))
                .map_or("a step", String::as_str);
            let undo = crate::engine::steps::Phase::Undo.index() as u64;
            match d["path"][0].as_u64() {
                Some(p) if p == undo => format!("{name} (undo)"),
                _ => name.to_string(),
            }
        };
        let line = match e.event.kind.as_str() {
            "run.started" => format!("started  {d}"),
            "action.finished" => format!("{}  done  {}", step(), d["output"]),
            "action.waiting" => format!("{}  waiting  {}", step(), d["output"]),
            "action.failed" => format!("{}  error  {}", step(), d["error"]),
            "timer.fired" => "waited too long".into(),
            // a message is copied to every waiting run: show those it took
            "message" if r.runtime.taken.iter().any(|t| d["id"] == t.as_str()) => {
                format!(
                    "message {}  {}",
                    d["type"].as_str().unwrap_or("?"),
                    d["data"]
                )
            }
            _ => continue,
        };
        println!("  {}  {line}", e.event.time);
    }
    if let Some(ended) = &r.ended {
        println!(
            "  {ended}  ended {}",
            crate::engine::steps::status(&r.runtime)
        );
    }
    println!(
        "  state {}",
        serde_json::Value::Object(r.runtime.state.clone())
    );
    Ok(())
}

/// What `itero check` refuses in the manifest: an entry naming no workflow,
/// action or connector, and the folders from before it (`args/`, `config/`).
fn check_manifest(
    ws: &crate::workspace::Workspace,
    flows: &[(String, crate::flow::Flow)],
) -> Result<()> {
    let manifest = crate::workspace::MANIFEST;
    let mut wrong = vec![];
    for (dir, now) in [
        ("args", "\"action\": {\"<name>\": {\"args\": ...}}"),
        ("config", "\"workflow\": {\"<name>\": {\"settings\": ...}}"),
    ] {
        if ws.root.join(dir).is_dir() {
            wrong.push(format!("{dir}/: this goes in {manifest} now, as {now}"));
        }
    }
    let actions: Vec<String> = ws
        .action_files()?
        .into_keys()
        .map(|f| f.split('@').next().unwrap_or(&f).to_string())
        .chain(
            crate::engine::worker::BUILT_IN
                .iter()
                .map(|b| b.to_string()),
        )
        .collect();
    let connectors: Vec<String> = ws.connector_files()?.into_iter().map(|(n, _)| n).collect();
    for name in ws.entries("connector")?.keys() {
        if let Err(why) = crate::engine::daemon::connector_args(ws, name) {
            wrong.push(why);
        }
    }
    for (kind, known) in [
        (
            "workflow",
            flows.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        ),
        ("action", actions),
        ("connector", connectors),
    ] {
        for name in ws.entries(kind)?.keys() {
            if !known.contains(name) {
                wrong.push(format!("{kind}.{name}: no {kind} {name}"));
            }
        }
    }
    if !wrong.is_empty() {
        bail!("{}: {}", ws.root.join(manifest).display(), wrong.join("; "));
    }
    Ok(())
}

fn dispatch(cli: Cli) -> Result<()> {
    let workspace = || crate::workspace::find(&std::env::current_dir()?);
    if matches!(
        cli.cmd,
        Cmd::Daemon { .. } | Cmd::Manager | Cmd::Worker | Cmd::Watchdog { .. }
    ) {
        ignore_sigpipe();
    }
    match cli.cmd {
        Cmd::Daemon {
            once,
            now,
            interval,
            no_serve,
        } => {
            use crate::engine::daemon;
            let ws = workspace()?;
            let exe = std::env::current_exe()?;
            if once {
                let at = match &now {
                    Some(t) => t
                        .parse::<chrono::DateTime<chrono::Utc>>()
                        .map_err(|_| anyhow!("--now takes an RFC 3339 time, not {t}"))?,
                    None => chrono::Utc::now(),
                };
                let pass = daemon::run_once(&ws, at, &exe);
                report(&pass, None);
                pass?;
                return Ok(());
            }
            use crate::engine::http::{self, Engine};
            // its port first: a daemon that cannot listen starts nothing
            let listening = if no_serve {
                None
            } else {
                Some(http::listen(&ws)?)
            };
            let workspace = crate::engine::journal::Journal::open(&ws)?.workspace()?;
            let log = crate::engine::log::Log::new(&ws, &workspace.id);
            if let Some((_, Some(note))) = &listening {
                log.write("port.changed", serde_json::json!({ "why": note }));
            }
            let me = daemon::Daemon::current();
            log.write(
                "daemon.started",
                serde_json::json!({
                    "workspace_name": workspace.name,
                    "workspace_created": workspace.created,
                    "workspace_itero": workspace.itero,
                    "path": ws.root.display().to_string(),
                    "meta": ws.meta,
                    "daemon_name": me.name,
                    "started": me.started,
                    "pid": me.pid,
                    "host": me.host,
                    "itero": me.itero,
                }),
            );
            stop_on_signal(log.clone());
            crate::engine::journal::Journal::open(&ws)?.publish(&crate::engine::events::own(
                "itero.daemon.started",
                serde_json::json!({ "daemon": me.id, "name": me.name, "itero": me.itero }),
                &me.started,
            ))?;
            eprintln!(
                "itero: running the workflows in {}, a pass every {interval}s (Ctrl-C stops); log: .itero/itero.log",
                ws.root.display()
            );
            let (wake, woken) = std::sync::mpsc::channel();
            let engine = std::sync::Arc::new(Engine {
                health: std::sync::Mutex::new(daemon::Health::starting(&daemon::stamp(
                    chrono::Utc::now(),
                ))),
                wake,
            });
            // a bad connector file stops the daemon here, naming the file
            for (kind, file) in ws.connector_files()? {
                let (ws, exe, engine, log) = (ws.clone(), exe.clone(), engine.clone(), log.clone());
                let wake = engine.wake.clone();
                std::thread::spawn(move || {
                    let connector = crate::connectors::supervise::Connector {
                        kind: &kind,
                        file: &file,
                    };
                    let policy = crate::connectors::supervise::Policy::default();
                    crate::connectors::supervise::run(
                        &ws,
                        &exe,
                        &connector,
                        &policy,
                        &wake,
                        &engine.health,
                        &log,
                    );
                });
            }
            {
                // one thread watches every path the workflows name
                let (ws, wake, log) = (ws.clone(), engine.wake.clone(), log.clone());
                std::thread::spawn(move || daemon::watch_paths(&ws, &wake, &log));
            }
            let looping = {
                let (ws, engine) = (ws.clone(), engine.clone());
                let interval = std::time::Duration::from_secs(interval);
                std::thread::spawn(move || {
                    daemon::run_loop(&ws, interval, &exe, &woken, &engine.health, |p| {
                        report(p, Some(&log))
                    })
                })
            };
            match listening {
                None => {
                    // the loop's sender lives in `engine`: the loop runs for good
                    looping
                        .join()
                        .map_err(|_| anyhow!("the daemon's loop stopped unexpectedly"))?;
                }
                Some((listener, _)) => {
                    let port = listener.local_addr()?.port();
                    eprintln!("itero: serving on http://127.0.0.1:{port}/");
                    http::serve(http::router(ws.clone(), engine, port), listener)?;
                }
            }
        }
        Cmd::Emit {
            kind,
            data,
            wait,
            timeout,
            reply,
        } => {
            use crate::engine::events::{self, Waited};
            let ws = workspace()?;
            let data = data
                .map(|d| serde_json::from_str(&d).map_err(|_| anyhow!("the data is a JSON object")))
                .transpose()?;
            let now = crate::engine::daemon::stamp(chrono::Utc::now());
            let fields = data
                .as_ref()
                .and_then(|d: &serde_json::Value| d.as_object().cloned())
                .unwrap_or_default();
            if wait {
                let timeout = humantime::parse_duration(&timeout).map_err(|_| {
                    anyhow!("--timeout: a duration is like 30s or 5m, not {timeout}")
                })?;
                // with no daemon nothing would answer: say so, sending nothing
                if !daemon_running(&ws, &crate::engine::journal::Journal::open(&ws)?)? {
                    bail!("no daemon is running: start itero daemon, then send it again");
                }
                let id = events::emit_with(&ws, &kind, data, reply.as_deref(), &now)?;
                let (answer, how) = events::wait_for(&ws, &id, timeout)?;
                println!("{}", serde_json::to_string_pretty(&answer)?);
                match how {
                    Waited::Done => return Ok(()),
                    Waited::Failed => std::process::exit(1),
                    Waited::TimedOut => {
                        eprintln!("timed out: not every run it started has ended");
                        std::process::exit(2);
                    }
                }
            }
            let id = events::emit_with(&ws, &kind, data, reply.as_deref(), &now)?;
            println!("{id}");
            // what it will do, on stderr: stdout stays the id for scripts
            let fate = crate::engine::events::fate(&ws, &kind, &fields)?;
            if !fate.starts.is_empty() {
                eprintln!("starts: {}", fate.starts.join(", "));
            }
            if fate.waiting > 0 {
                eprintln!("may reach {} waiting run(s)", fate.waiting);
            }
            if fate.starts.is_empty() {
                match &fate.nearest {
                    Some(n) => eprintln!("nothing starts on {kind}: did you mean {n}?"),
                    None => eprintln!("nothing starts on {kind}"),
                }
            }
            if !daemon_running(&ws, &crate::engine::journal::Journal::open(&ws)?)? {
                eprintln!("no daemon is running: it waits for itero daemon");
            }
        }
        Cmd::Args { name, data, file } => {
            let ws = workspace()?;
            let text = match (data, file) {
                (Some(d), _) => d,
                (None, Some(f)) => std::fs::read_to_string(&f)
                    .map_err(|e| anyhow!("cannot read {}: {e}", f.display()))?,
                (None, None) => bail!("give the args as JSON, or --file"),
            };
            let bad_name = name.is_empty()
                || name.contains(['/', '\\', '#', '@', '$'])
                || name.starts_with('.');
            if bad_name {
                bail!("{name} is not an action's or a connector's name");
            }
            // a connector's when the workspace has a connector of that name
            let connector = ws.connector_files()?.iter().any(|(n, _)| *n == name);
            if connector && ws.action_file(&name, None).is_ok() {
                bail!(
                    "{name} is an action and a connector: set its args in {}",
                    crate::workspace::MANIFEST
                );
            }
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| anyhow!("args are an object, or a list of objects, as JSON"))?;
            let ok = match &v {
                serde_json::Value::Object(_) => true,
                serde_json::Value::Array(items) => {
                    !connector && items.iter().all(|i| i.is_object())
                }
                _ => false,
            };
            if !ok && connector {
                bail!("a connector's args are an object, as JSON: a connector runs once");
            }
            if !ok {
                bail!("args are an object, or a list of objects, as JSON");
            }
            let kind = if connector { "connector" } else { "action" };
            let f = ws.write_entry(kind, &name, "args", v)?;
            println!("{}", f.display());
        }
        Cmd::Config {
            workflow,
            data,
            file,
        } => {
            let ws = workspace()?;
            let bad_name = workflow.is_empty()
                || workflow.contains(['/', '\\', '#', '@', '$'])
                || workflow.starts_with('.');
            if bad_name {
                bail!("{workflow} is not a workflow's name");
            }
            let text = match (data, file) {
                (Some(d), _) => d,
                (None, Some(path)) => std::fs::read_to_string(&path)
                    .map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?,
                (None, None) => {
                    let settings = ws
                        .entries("workflow")?
                        .get(&workflow)
                        .and_then(|e| e.get("settings"))
                        .cloned();
                    match settings {
                        Some(s) => println!("{}", serde_json::to_string_pretty(&s)?),
                        None => eprintln!(
                            "{workflow} has no settings in {}",
                            crate::workspace::MANIFEST
                        ),
                    }
                    return Ok(());
                }
            };
            let Ok(settings @ serde_json::Value::Object(_)) = serde_json::from_str(&text) else {
                bail!("settings are an object, as JSON: {{\"name\": value}}");
            };
            let f = ws.write_entry("workflow", &workflow, "settings", settings)?;
            println!("{}", f.display());
        }
        Cmd::Secret { name, file } => {
            crate::engine::secrets::ready(&name)?;
            let value = match file {
                Some(f) => {
                    let t = std::fs::read_to_string(&f)
                        .map_err(|e| anyhow!("cannot read {}: {e}", f.display()))?;
                    let t = t.strip_suffix('\n').unwrap_or(&t);
                    t.strip_suffix('\r').unwrap_or(t).to_string()
                }
                None => rpassword::prompt_password(format!("{name}: "))?,
            };
            if value.is_empty() {
                bail!("{name}: an empty secret is not kept");
            }
            crate::engine::secrets::set(&name, &value)?;
            println!("{name}: kept in the OS secret store");
        }
        Cmd::Check => {
            let ws = workspace()?;
            for dir in ws.workflows.iter().chain(&ws.actions) {
                if !dir.exists() {
                    eprintln!("note: no folder {}", dir.display());
                }
            }
            let mut flows = ws.flows()?;
            check_manifest(&ws, &flows)?;
            // the internal ones last, apart
            flows.sort_by_key(|(_, f)| f.meta.internal);
            let j = crate::engine::journal::Journal::open(&ws)?;
            let now = crate::engine::daemon::stamp(chrono::Utc::now());
            let files = ws.flow_files()?;
            let mut apart = false;
            for (name, f) in &flows {
                if f.meta.internal && !apart {
                    apart = true;
                    println!("\ninternal");
                }
                println!("{name}");
                let id = j.workflow_id(name, &now)?;
                println!("  id {}  since {}", id.id, id.created);
                match files.get(name) {
                    Some(file) => println!("  file {}", ws.shown(file)),
                    None => println!("  file built in"),
                }
                if !f.settings.is_empty() {
                    println!(
                        "  settings {}",
                        serde_json::Value::Object(f.settings.clone())
                    );
                }
                let m = &f.meta;
                if let Some(d) = &m.description {
                    println!("  description {d}");
                }
                for (k, t) in [("from", m.from), ("until", m.until)] {
                    if let Some(t) = t {
                        println!("  {k} {}", crate::engine::daemon::stamp(t));
                    }
                }
                for (k, on) in [("off", m.off), ("internal", m.internal)] {
                    if on {
                        println!("  {k}");
                    }
                }
                for (k, v) in &m.other {
                    println!("  {k} {v}");
                }
                let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
                println!("  runs so far {}", runs.len());
                if let Some(last) = runs.first() {
                    println!(
                        "  last run {} {}  {}",
                        last.name,
                        crate::engine::steps::status(&last.runtime),
                        last.ended.as_deref().unwrap_or(&last.started)
                    );
                }
                for u in f.uses() {
                    match u {
                        crate::flow::Name::Action { name: a, version } => {
                            match ws.action_file(&a, version.as_deref()) {
                                Ok(file) => {
                                    println!("  #{a}  {}", file.display());
                                    // the PATH an action runs with
                                    let path = crate::connectors::supervise::path_with(
                                        &std::env::current_exe()?,
                                    );
                                    let first = crate::engine::worker::first_line(&file)
                                        .unwrap_or_default();
                                    for note in crate::needs::notes(&file, &first, Some(&path)) {
                                        println!("    {note}");
                                    }
                                }
                                Err(_)
                                    if crate::engine::worker::BUILT_IN.contains(&a.as_str())
                                        && version.is_none() =>
                                {
                                    println!("  #{a}  built in")
                                }
                                Err(e) => bail!("{name}: {e:#}"),
                            }
                        }
                        crate::flow::Name::Workflow(w) => {
                            if !flows.iter().any(|(n, _)| *n == w) {
                                bail!("{name} uses workflow {w}, which is not there");
                            }
                            println!("  {w}  workflow");
                        }
                    }
                }
            }
            for (kind, file) in ws.connector_files()? {
                println!("connector {kind}  {}", file.display());
            }
        }
        Cmd::Runs { run: Some(run), .. } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let found = j.run_by(&run)?;
            let Some(r) = found.first() else {
                bail!("no run {run}: itero runs lists them");
            };
            if found.len() > 1 {
                println!(
                    "{} runs are named {run}: this is the newest; give an id for another",
                    found.len()
                );
            }
            story(&j, r)?;
        }
        Cmd::Which { name } => {
            let ws = workspace()?;
            let (action_only, name) = match name.strip_prefix('#') {
                Some(n) => (true, n.to_string()),
                None => (false, name),
            };
            let mut found: Vec<(&str, String)> = vec![];
            if !action_only {
                match ws.flow_files()?.get(&name) {
                    Some(f) => found.push(("workflow", ws.shown(f))),
                    None if crate::workspace::BUILT_IN.iter().any(|(n, _)| *n == name) => {
                        found.push(("workflow", "built in".into()))
                    }
                    None => {}
                }
            }
            // the action, and each of its versions
            let actions: Vec<String> = ws
                .action_files()?
                .into_iter()
                .filter(|(file, _)| *file == name || file.starts_with(&format!("{name}@")))
                .map(|(_, f)| ws.shown(&f))
                .collect();
            if actions.is_empty() && crate::engine::worker::BUILT_IN.contains(&name.as_str()) {
                found.push(("action", "built in".into()));
            }
            found.extend(actions.into_iter().map(|a| ("action", a)));
            if !action_only {
                for (kind, f) in ws.connector_files()? {
                    if kind == name {
                        found.push(("connector", ws.shown(&f)));
                    }
                }
            }
            if found.is_empty() {
                bail!("nothing named {name}: itero workflows and itero check list them");
            }
            for (kind, path) in found {
                println!("{kind:<10}{path}");
            }
        }
        Cmd::Workflows => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let now = chrono::Utc::now();
            let mut flows = ws.flows()?;
            flows.sort_by_key(|(_, f)| f.meta.internal);
            let files = ws.flow_files()?;
            // the columns: name, its file, what starts it, runs, the last, notes
            let mut rows: Vec<[String; 6]> = vec![];
            for (name, f) in &flows {
                let on = &f.on;
                let mut starts: Vec<String> = on.events.iter().map(|e| format!("on {e}")).collect();
                starts.extend(on.schedules.iter().map(|s| format!("at {s}")));
                starts.extend(on.paths.iter().map(|p| format!("path {p}")));
                if starts.is_empty() {
                    starts.push("a step only".into());
                }
                let mut notes = vec![];
                if f.meta.off {
                    notes.push("off");
                } else if !crate::flow::active(&f.meta, now) {
                    notes.push("not now (from/until)");
                }
                if f.meta.internal {
                    notes.push("internal");
                }
                let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
                let last = match runs.first() {
                    Some(r) => {
                        let at = r.ended.as_deref().unwrap_or(&r.started);
                        let status = crate::engine::steps::status(&r.runtime);
                        format!("last: {status} {}", ago(at, now))
                    }
                    None => "last: none".into(),
                };
                let file = files
                    .get(name)
                    .map_or_else(|| "built in".to_string(), |f| ws.shown(f));
                rows.push([
                    name.clone(),
                    file,
                    starts.join(", "),
                    format!("runs {}", runs.len()),
                    last,
                    notes.join(", "),
                ]);
            }
            let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0);
            let widths: Vec<usize> = (0..5).map(width).collect();
            for r in &rows {
                let mut line = String::new();
                for (c, w) in widths.iter().enumerate() {
                    line += &format!("{:<w$}   ", r[c]);
                }
                line += &r[5];
                println!("{}", line.trim_end());
            }
            let waiting = match j.waiting_messages()? {
                0 => "no message waiting".to_string(),
                1 => "1 message waiting".to_string(),
                n => format!("{n} messages waiting"),
            };
            let daemon = if daemon_running(&ws, &j)? {
                "running"
            } else {
                "not running"
            };
            println!("daemon: {daemon}, {waiting}");
        }
        Cmd::Info => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let w = j.workspace()?;
            println!("workspace");
            println!("  id {}", w.id);
            // the manifest's name, once given; until then, itero's
            let named = ws.meta.get("name").and_then(|n| n.as_str());
            println!("  name {}", named.unwrap_or(&w.name));
            println!("  created {}", w.created);
            println!("  path {}", ws.root.display());
            println!("  itero {}", w.itero);
            let about: Vec<_> = ws.meta.iter().filter(|(k, _)| *k != "name").collect();
            if !about.is_empty() {
                println!("  meta");
                for (k, v) in about {
                    println!("    {k} {v}");
                }
            }
            let l = &ws.limits;
            println!("  limits");
            println!("    workflows {}", l.workflows);
            println!("    runs {}", l.runs);
            println!("    steps {}", l.steps);
            println!("    space {}", l.space);
            println!("    keep {}", humantime::format_duration(l.keep));
            println!("    log {}", l.log);
            println!("    logs {}", l.logs);
            let port = ws.daemon_port();
            match (port, port.and_then(daemon_health)) {
                (Some(p), Some(h)) if h["workspace"]["id"] != w.id.as_str() => println!(
                    "daemon: port {p} answers, but for another workspace ({})",
                    h["workspace"]["path"]
                        .as_str()
                        .or(h["workspace"].as_str())
                        .unwrap_or("unknown")
                ),
                (Some(p), Some(h)) => {
                    let d = &h["daemon"];
                    println!("daemon");
                    for k in ["id", "name", "started", "pid", "host", "itero"] {
                        println!(
                            "  {k} {}",
                            d[k].as_str().map_or(d[k].to_string(), String::from)
                        );
                    }
                    println!("  port {p}");
                }
                (Some(p), None) => println!("daemon: not running (no answer on port {p})"),
                (None, _) => {
                    println!("daemon: not running (no port yet: the OS gives one at its start)")
                }
            }
        }
        Cmd::Runs { run: None, all } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let apart = if all { vec![] } else { ws.internal()? };
            for r in j.runs_page(None, None, &apart, usize::MAX)? {
                println!(
                    "{}  {:<20} {:<20} {:<12} {}  {}",
                    r.id,
                    r.name,
                    r.workflow,
                    crate::engine::steps::status(&r.runtime),
                    r.started,
                    r.ended.as_deref().unwrap_or("-")
                );
            }
        }
        Cmd::Watch { path } => {
            let watch = crate::connectors::Watch::new(&path)?;
            // the daemon going away closes stdin: then the connector using
            // this stops it
            std::thread::spawn(|| {
                let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
                std::process::exit(0);
            });
            watch.run(&mut std::io::stdout().lock())?;
        }
        Cmd::Errors { limit } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            for r in j.runs_page(None, Some("error"), &[], limit)? {
                let st = &r.runtime.state;
                let text = |k: &str| st.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                println!(
                    "{}  {:<20} {}  {}: {}",
                    r.ended.as_deref().unwrap_or(&r.started),
                    r.workflow,
                    r.id,
                    text("failed_step"),
                    text("why")
                );
            }
        }
        Cmd::Service { cmd } => {
            use crate::engine::service;
            let ws = workspace()?;
            match cmd {
                ServiceCmd::Install { interval } => {
                    let exe = std::env::current_exe()?;
                    let log = service::log_file(&ws.root);
                    if let Some(dir) = log.parent() {
                        std::fs::create_dir_all(dir)?;
                    }
                    let path = std::env::var("PATH").ok();
                    service::install(service::install_ctx(&ws.root, &exe, interval, path))?;
                    println!(
                        "installed: {} runs itero in {}; it starts at login",
                        service::label(&ws.root).to_qualified_name(),
                        ws.root.display()
                    );
                    match ws.port {
                        Some(p) => println!("API: http://127.0.0.1:{p}/"),
                        None => println!("API: on the port the OS gives it, as itero info says"),
                    }
                    println!("log: {}", log.display());
                    println!("it keeps this shell's PATH; install again after changing it");
                }
                ServiceCmd::Uninstall => {
                    if service::uninstall(&ws.root)? {
                        println!("stopped and removed");
                    } else {
                        println!("no service for {}", ws.root.display());
                    }
                }
                ServiceCmd::Status => {
                    println!("service: {}", service::status(&ws.root)?);
                    let port = ws.daemon_port();
                    match port.and_then(daemon_health) {
                        Some(h) => println!(
                            "daemon: pid {}, {} passes, last {}{}",
                            h["pid"],
                            h["passes"],
                            h["last_pass"].as_str().unwrap_or("none yet"),
                            h["last_error"]
                                .as_str()
                                .map(|e| format!(", last error: {e}"))
                                .unwrap_or_default()
                        ),
                        None => match port {
                            Some(p) => println!("daemon: no answer on port {p}"),
                            None => println!("daemon: no port yet"),
                        },
                    }
                }
            }
        }
        Cmd::Watchdog {
            interval,
            check_every,
            misses,
            stale_after,
            log,
        } => {
            use crate::engine::watchdog::{Options, Rules, run};
            let ws = workspace()?;
            run(&Options {
                exe: std::env::current_exe()?,
                daemon_args: vec!["--interval".into(), interval.to_string()],
                ws: ws.clone(),
                rules: Rules {
                    check_every: std::time::Duration::from_secs(check_every),
                    misses,
                    stale_after: std::time::Duration::from_secs(stale_after),
                    ..Rules::default()
                },
                log,
            })?;
        }
        Cmd::Manager => {
            crate::engine::die_at("manager");
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
            let input: crate::engine::steps::StepInput =
                serde_json::from_str(&text).map_err(|e| anyhow!("the manager's input: {e}"))?;
            let answer = crate::engine::steps::decide(&input).map_err(|e| anyhow!(e))?;
            println!("{}", serde_json::to_string(&answer)?);
        }
        Cmd::NotificationClicked { data } => {
            let ws = workspace()?;
            let v: serde_json::Value =
                serde_json::from_str(&data).map_err(|e| anyhow!("the click, as JSON: {e}"))?;
            let now = crate::engine::daemon::stamp(chrono::Utc::now());
            crate::engine::events::notification_clicked(&ws, v, &now)?;
        }
        Cmd::Worker => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
            let step: crate::engine::worker::StepRun =
                serde_json::from_str(&text).map_err(|e| anyhow!("the worker's step: {e}"))?;
            use crate::engine::worker::{Failure, LOST_EXIT, run_step_telling};
            use std::io::Write;
            use std::time::{Duration, Instant};
            #[cfg(unix)]
            crate::engine::worker::stop_with_parent();
            crate::engine::die_at("worker");
            // while the action prints, a blank line on stdout now and then
            // tells the daemon it is working; the answer after them is read
            // all the same. A daemon gone stops this worker anyway, so a
            // line it cannot take is no error.
            let last = std::sync::Mutex::new(None::<Instant>);
            let printed = || {
                let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) {
                    *last = Some(Instant::now());
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(b"\n").and_then(|()| out.flush());
                }
            };
            match run_step_telling(&step, &printed) {
                Ok(outcome) => {
                    // the step's effect is done; its answer is not yet given
                    crate::engine::die_at("worker-done");
                    println!("{}", serde_json::to_string(&outcome)?)
                }
                Err(Failure::Error(why)) => bail!(why),
                Err(Failure::Lost(why)) => {
                    eprintln!("error: {why}");
                    std::process::exit(LOST_EXIT);
                }
            }
        }
    }
    Ok(())
}
