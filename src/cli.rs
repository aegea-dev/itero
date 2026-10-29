//! The command line.
mod ai;
mod complete;
mod kinds;
mod out;
mod top;
mod workflow;
mod workspace;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "itero",
    version,
    about = "itero: your domain, your solution, your tool. Build the harness you need, from anything, for anything."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the workspace's workflows: their schedules, events and jobs, as each comes due
    Daemon {
        /// One pass, then stop: for tests and scripts, so hidden
        #[arg(long, hide = true)]
        once: bool,
        /// The time to run at, with --once (RFC 3339; for tests)
        #[arg(long, requires = "once", hide = true)]
        now: Option<String>,
        /// The passes only: no HTTP API
        #[arg(long)]
        no_serve: bool,
    },
    /// Read every workflow, action and connector, before anything runs, and say what is wrong
    Check {
        #[arg(long)]
        json: bool,
    },
    /// Send an event: it starts runs, and reaches the runs waiting for it
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
        /// Print its id, and what it starts, as JSON (with --wait, the runs are JSON already)
        #[arg(long, conflicts_with = "wait")]
        json: bool,
    },
    /// Keep a secret in the OS secret store, for $$NAME (asks for it, without echo)
    Secret {
        /// Its name: a letter or _, then letters, digits or _
        name: String,
        /// Read it from this file (its last newline left out)
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// The workspace: its identity, meta and limits, and the daemon running it
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Where a workflow, action or connector of this name is (#name: the action only)
    Which {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// The workflows, their runs and their settings
    #[command(visible_alias = "wf")]
    Workflow {
        #[command(subcommand)]
        cmd: WorkflowCmd,
    },
    /// The actions, the ones that break, and their args
    Action {
        #[command(subcommand)]
        cmd: ActionCmd,
    },
    /// The connectors, and their args
    Connector {
        #[command(subcommand)]
        cmd: ConnectorCmd,
    },
    /// The machine's workspaces, which one daemon runs
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceCmd,
    },
    /// The local models #ai steps use (through itero-ai)
    Ai {
        #[command(subcommand)]
        cmd: AiCmd,
    },
    /// Print the script that completes itero's commands and names on Tab
    Completion {
        /// bash, zsh or fish
        shell: Shell,
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
    /// answering or a pass hangs (what the service runs)
    #[command(hide = true)]
    Watchdog {
        /// Gone: a service installed before 0.10.0 still passes it
        #[arg(long, hide = true)]
        interval: Option<String>,
        /// Seconds between checks of the daemon's /health
        #[arg(long, default_value_t = 10)]
        check_every: u64,
        /// Checks in a row without an answer before a restart
        #[arg(long, default_value_t = 3)]
        misses: u32,
        /// Seconds one pass may run before the daemon counts as hung
        #[arg(long, default_value_t = 600)]
        stale_after: u64,
        /// Where its lines and the daemon's stderr go
        #[arg(long)]
        log: Option<std::path::PathBuf>,
    },
}

/// The shells `itero completion` writes for.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
pub enum AiCmd {
    /// The models on this machine, and downloading or removing one
    Models {
        #[command(subcommand)]
        cmd: ModelsCmd,
    },
    /// One GGUF file: what it is, and whether it runs here
    Inspect {
        file: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ModelsCmd {
    /// The models found: LM Studio's, Hugging Face's cache, and itero's own
    List {
        #[arg(long)]
        json: bool,
        /// The removed ones too
        #[arg(long)]
        all: bool,
    },
    /// Download one from Hugging Face: <org>/<repo>, with :<QUANT> or without
    Add {
        name: String,
        /// Download without asking
        #[arg(long)]
        yes: bool,
    },
    /// Take one out of use; its file stays
    Remove {
        name: String,
        /// Delete its file too (only one itero downloaded)
        #[arg(long)]
        delete: bool,
    },
}

#[derive(Subcommand)]
pub enum WorkflowCmd {
    /// Each workflow: what starts it, its runs and the last
    List {
        #[arg(long)]
        json: bool,
    },
    /// The runs, newest first, or one run step by step
    Runs {
        /// One run, by id or name: what happened, step by step
        #[arg(add = complete::runs())]
        run: Option<String>,
        /// Only the runs that ended in error, and why
        #[arg(long)]
        errors: bool,
        /// Only this workflow's runs
        #[arg(long, add = complete::workflows())]
        workflow: Option<String>,
        /// Only the runs that broke at this action
        #[arg(long, add = complete::actions())]
        action: Option<String>,
        /// At most this many
        #[arg(long)]
        limit: Option<usize>,
        /// The internal workflows' runs too
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// A workflow's settings: the JSON object its runs start with
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Show a workflow's settings
    Get {
        #[arg(add = complete::workflows())]
        workflow: String,
        #[arg(long)]
        json: bool,
    },
    /// Set a workflow's settings, a JSON object
    Set {
        #[arg(add = complete::workflows())]
        workflow: String,
        /// The settings, as JSON
        #[arg(
            value_name = "DATA",
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        data: Option<String>,
        /// Read them from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ActionCmd {
    /// Each action, its files and whether it has args
    List {
        #[arg(long)]
        json: bool,
    },
    /// Which actions broke runs, how often and why; with a name, that action's failures
    Errors {
        /// One action, without #
        #[arg(add = complete::actions())]
        action: Option<String>,
        /// At most this many
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// An action's args: the data its step gets
    Args {
        #[command(subcommand)]
        cmd: ActionArgsCmd,
    },
    /// An action's repeat: its step runs once per item
    Repeat {
        #[command(subcommand)]
        cmd: ActionRepeatCmd,
    },
}

#[derive(Subcommand)]
pub enum ConnectorCmd {
    /// Each connector, its file and whether it has args
    List {
        #[arg(long)]
        json: bool,
    },
    /// A connector's args: the data it starts with
    Args {
        #[command(subcommand)]
        cmd: ConnectorArgsCmd,
    },
}

#[derive(Subcommand)]
pub enum ActionRepeatCmd {
    /// Show an action's repeat
    Get {
        #[arg(add = complete::actions())]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Set an action's repeat: a list of objects, one run of its step for each
    Set {
        #[arg(add = complete::actions())]
        name: String,
        /// The items, as JSON
        #[arg(
            value_name = "DATA",
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        data: Option<String>,
        /// Read them from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ActionArgsCmd {
    /// Show an action's args
    Get {
        #[arg(add = complete::actions())]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Set an action's args: an object, or a list of objects read in order
    Set {
        #[arg(add = complete::actions())]
        name: String,
        /// The args, as JSON
        #[arg(
            value_name = "DATA",
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        data: Option<String>,
        /// Read them from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ConnectorArgsCmd {
    /// Show a connector's args
    Get {
        #[arg(add = complete::connectors())]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Set a connector's args: an object
    Set {
        #[arg(add = complete::connectors())]
        name: String,
        /// The args, as JSON
        #[arg(
            value_name = "DATA",
            required_unless_present = "file",
            conflicts_with = "file"
        )]
        data: Option<String>,
        /// Read them from this file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceCmd {
    /// Add this workspace to the machine's list: the machine's daemon runs it
    Add,
    /// Take a workspace off the list (this one, or one by name); its folder stays
    Remove { name: Option<String> },
    /// The machine's workspaces: name, folder, state and last run
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ServiceCmd {
    /// Run the daemon as a service (per user, at login), kept up by a watchdog
    Install,
    /// Stop and remove the service
    Uninstall,
    /// The service's state and the daemon's health
    Status {
        #[arg(long)]
        json: bool,
    },
}

/// Lists a command's subcommands, and theirs, by name in help: one display
/// order for all, which clap breaks by name.
fn by_name(c: clap::Command) -> clap::Command {
    c.mut_subcommands(|s| by_name(s.display_order(0)))
}

/// The command line, as parsed and as the shell completes it.
fn command() -> clap::Command {
    use clap::CommandFactory;
    by_name(Cli::command())
}

/// Runs the command line; returns the exit code.
pub fn run(args: Vec<String>) -> i32 {
    use clap::FromArgMatches;
    // the shell asking for completions: answered, and the process ends
    complete::answer(command);
    let parsed = command()
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
        Ok(p) if p.delivered + p.worked + p.lost + p.stopped + p.deferred > 0 => eprintln!(
            "{} events, {} jobs, {} lost (run again), {} stopped, {} waiting",
            p.delivered, p.worked, p.lost, p.stopped, p.deferred
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
        crate::ai::keeper::Keeper::global().stop();
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

/// Wakes the running daemon for a message saved in the journal; the
/// message is kept either way, so a failure is said, not fatal.
fn nudge(ws: &crate::workspace::Workspace, now: &str) {
    if let Err(e) = crate::engine::events::nudge(ws, now) {
        eprintln!(
            "itero: saved, but could not wake the daemon ({}: {e}); it takes the message at its next start or wake",
            ws.wake_file().display()
        );
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
            use crate::engine::http;
            // one daemon per journal: held for this daemon's life
            let me = daemon::Daemon::current();
            let lock = crate::engine::lock::take(&ws, &format!("{}, pid {}", me.name, me.pid))
                .map_err(|e| anyhow!(e))?;
            // its port first: a daemon that cannot listen starts nothing
            let listening = if no_serve {
                None
            } else {
                Some(http::listen(&ws)?)
            };
            let running = crate::engine::running::Running::start(ws.clone(), &exe, lock, report)?;
            if let Some((_, Some(note))) = &listening {
                running
                    .log
                    .write("port.changed", serde_json::json!({ "why": note }));
            }
            stop_on_signal(running.log.clone());
            eprintln!(
                "itero: running the workflows in {}, each as it comes due (Ctrl-C stops); log: .itero/itero.log",
                ws.root.display()
            );
            let engine = running.engine.clone();
            match listening {
                None => {
                    // runs until the process is stopped
                    let _running = running;
                    loop {
                        std::thread::park();
                    }
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
            json,
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
                let id = events::emit(&ws, &kind, data, &now)?;
                nudge(&ws, &now);
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
            let id = events::emit(&ws, &kind, data, &now)?;
            nudge(&ws, &now);
            let fate = crate::engine::events::fate(&ws, &kind, &fields)?;
            let running = daemon_running(&ws, &crate::engine::journal::Journal::open(&ws)?)?;
            let v = serde_json::json!({
                "id": id,
                "starts": fate.starts,
                "waiting": fate.waiting,
                "nearest": fate.nearest,
                "daemon": { "running": running },
            });
            out::show(json, &v, |_| {
                println!("{id}");
                // what it will do, on stderr: stdout stays the id for scripts
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
                if !running {
                    eprintln!("no daemon is running: it waits for itero daemon");
                }
            })?;
        }
        Cmd::Workflow { cmd } => {
            let ws = workspace()?;
            match cmd {
                WorkflowCmd::List { json } => workflow::list(&ws, json)?,
                WorkflowCmd::Runs {
                    run: Some(run),
                    json,
                    ..
                } => workflow::run(&ws, &run, json)?,
                WorkflowCmd::Runs {
                    run: None,
                    errors,
                    workflow,
                    action,
                    limit,
                    all,
                    json,
                } => workflow::runs(
                    &ws,
                    &workflow::Filter {
                        errors,
                        workflow,
                        action,
                        limit,
                        all,
                    },
                    json,
                )?,
                WorkflowCmd::Config {
                    cmd: ConfigCmd::Get { workflow, json },
                } => workflow::config_get(&ws, &workflow, json)?,
                WorkflowCmd::Config {
                    cmd:
                        ConfigCmd::Set {
                            workflow,
                            data,
                            file,
                        },
                } => workflow::config_set(&ws, &workflow, data, file)?,
            }
        }
        Cmd::Action { cmd } => {
            let ws = workspace()?;
            match cmd {
                ActionCmd::List { json } => kinds::actions(&ws, json)?,
                ActionCmd::Errors {
                    action,
                    limit,
                    json,
                } => kinds::errors(&ws, action.as_deref(), limit, json)?,
                ActionCmd::Args {
                    cmd: ActionArgsCmd::Get { name, json },
                } => kinds::args_get(&ws, kinds::Kind::Action, &name, json)?,
                ActionCmd::Args {
                    cmd: ActionArgsCmd::Set { name, data, file },
                } => kinds::args_set(&ws, kinds::Kind::Action, &name, data, file)?,
                ActionCmd::Repeat {
                    cmd: ActionRepeatCmd::Get { name, json },
                } => kinds::entry_get(&ws, kinds::Kind::Action, &name, "repeat", json)?,
                ActionCmd::Repeat {
                    cmd: ActionRepeatCmd::Set { name, data, file },
                } => kinds::repeat_set(&ws, &name, data, file)?,
            }
        }
        Cmd::Connector { cmd } => {
            let ws = workspace()?;
            match cmd {
                ConnectorCmd::List { json } => kinds::connectors(&ws, json)?,
                ConnectorCmd::Args {
                    cmd: ConnectorArgsCmd::Get { name, json },
                } => kinds::args_get(&ws, kinds::Kind::Connector, &name, json)?,
                ConnectorCmd::Args {
                    cmd: ConnectorArgsCmd::Set { name, data, file },
                } => kinds::args_set(&ws, kinds::Kind::Connector, &name, data, file)?,
            }
        }
        Cmd::Completion { shell } => complete::script(shell, command),
        Cmd::Workspace { cmd } => match cmd {
            WorkspaceCmd::Add => workspace::add(&workspace()?)?,
            WorkspaceCmd::Remove { name } => workspace::remove(name.as_deref())?,
            WorkspaceCmd::List { json } => workspace::list(json)?,
        },
        Cmd::Ai { cmd } => match cmd {
            AiCmd::Models { cmd } => match cmd {
                ModelsCmd::List { json, all } => ai::list(json, all)?,
                ModelsCmd::Add { name, yes } => ai::add(&name, yes)?,
                ModelsCmd::Remove { name, delete } => ai::remove(&name, delete)?,
            },
            AiCmd::Inspect { file, json } => ai::inspect(&file, json)?,
        },
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
        Cmd::Check { json } => top::check(&workspace()?, json)?,
        Cmd::Which { name, json } => top::which(&workspace()?, &name, json)?,
        Cmd::Info { json } => top::info(&workspace()?, json)?,
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
        Cmd::Service { cmd } => {
            use crate::engine::service;
            let ws = workspace()?;
            match cmd {
                ServiceCmd::Install => {
                    let exe = std::env::current_exe()?;
                    let log = service::log_file(&ws.root);
                    if let Some(dir) = log.parent() {
                        std::fs::create_dir_all(dir)?;
                    }
                    let path = std::env::var("PATH").ok();
                    service::install(service::install_ctx(&ws.root, &exe, path))?;
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
                ServiceCmd::Status { json } => top::service_status(&ws, json)?,
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
            if interval.is_some() {
                bail!(
                    "--interval is gone: the daemon now waits for what is due. Run `itero service install` again in this workspace to update the service"
                );
            }
            let ws = workspace()?;
            run(&Options {
                exe: std::env::current_exe()?,
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
            nudge(&ws, &now);
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
            // tells the daemon it is working, and `said <JSON text>` what it
            // said last on stderr, at most twice a second; the answer after
            // them is read all the same. A daemon gone stops this worker
            // anyway, so a line it cannot take is no error.
            let line = |text: &[u8]| {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(text).and_then(|()| out.flush());
            };
            let last = std::sync::Mutex::new(None::<Instant>);
            let said = std::sync::Mutex::new((None::<String>, None::<String>));
            let ended = std::sync::atomic::AtomicBool::new(false);
            let printed = |now_said: Option<&str>| {
                if let Some(s) = now_said {
                    said.lock().unwrap_or_else(|e| e.into_inner()).0 = Some(s.to_string());
                }
                let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) {
                    *last = Some(Instant::now());
                    line(b"\n");
                }
            };
            let outcome = std::thread::scope(|s| {
                // the latest line goes on even when the action falls silent
                s.spawn(|| {
                    let mut told = Instant::now() - Duration::from_secs(1);
                    while !ended.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                        if told.elapsed() < Duration::from_millis(500) {
                            continue;
                        }
                        let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
                        if said.0.is_some() && said.0 != said.1 {
                            said.1 = said.0.clone();
                            told = Instant::now();
                            let text = serde_json::to_string(&said.1).unwrap_or_default();
                            line(format!("said {text}\n").as_bytes());
                        }
                    }
                });
                let outcome = run_step_telling(&step, &printed);
                ended.store(true, std::sync::atomic::Ordering::Relaxed);
                outcome
            });
            match outcome {
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
