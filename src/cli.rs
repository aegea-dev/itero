//! The command line.
mod ai;
mod complete;
mod context;
mod emit;
mod event;
mod kinds;
mod out;
mod token;
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
    /// The itero deployment to ask (itero context list); else the one in use, else this machine
    #[arg(long, global = true, value_name = "NAME", add = complete::contexts())]
    pub context: Option<String>,
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// An event to send, and how: `itero emit` and `itero ws emit`.
#[derive(clap::Args)]
pub struct EmitArgs {
    /// Its type, two or more words joined by dots, like order.placed
    #[arg(value_name = "TYPE")]
    kind: String,
    /// Its data, a JSON object
    #[arg(value_name = "DATA")]
    data: Option<String>,
    /// Its data from this file, a JSON object (- for stdin)
    #[arg(long, value_name = "PATH", conflicts_with = "data")]
    file: Option<std::path::PathBuf>,
    /// Wait for the runs it starts to end, and print them as JSON (exit 1: one failed or broke; 2: time ran out)
    #[arg(long)]
    wait: bool,
    /// With --wait: how long, like 30s or 5m
    #[arg(long, default_value = "60s", requires = "wait")]
    timeout: String,
    /// Print what was sent as JSON (with --wait, the runs are JSON already)
    #[arg(long, conflicts_with = "wait")]
    json: bool,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Run the workspace's workflows: their schedules, events and jobs, as each comes due
    Daemon {
        /// Every workspace on the machine's list (itero workspace list), not this folder's
        #[arg(long, conflicts_with = "once")]
        all: bool,
        /// With --all: its settings from this file, not the config folder's server.json
        #[arg(long, requires = "all", value_name = "PATH")]
        config: Option<std::path::PathBuf>,
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
    /// Moved: `itero ws check`
    #[command(name = "check", hide = true, disable_help_flag = true)]
    MovedCheck {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// Send an event to every workspace on the machine's list
    Emit(EmitArgs),
    /// Keep a secret in the OS secret store, for $$NAME in every workspace (asks for it, without echo)
    Secret {
        /// Its name: a letter or _, then letters, digits or _
        name: String,
        /// Read it from this file (its last newline left out)
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// The machine's daemon: its version, pid, port, workspaces, itero-ai and notifications
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Moved: `itero ws which`
    #[command(name = "which", hide = true, disable_help_flag = true)]
    MovedWhich {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// Moved: `itero ws wf`
    #[command(name = "workflow", alias = "wf", hide = true, disable_help_flag = true)]
    MovedWorkflow {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// Moved: `itero ws ac`
    #[command(name = "action", hide = true, disable_help_flag = true)]
    MovedAction {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// Moved: `itero ws cn`
    #[command(name = "connector", hide = true, disable_help_flag = true)]
    MovedConnector {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        rest: Vec<String>,
    },
    /// One workspace: its workflows, actions and connectors, and the machine's list
    #[command(visible_alias = "ws")]
    Workspace {
        /// The workspace, by its name on the machine's list (itero ws list); else this folder's
        #[arg(long, global = true, value_name = "NAME", add = complete::workspaces())]
        ws: Option<String>,
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
    /// The itero deployments this CLI talks to: each its server, token and workspace
    Context {
        #[command(subcommand)]
        cmd: ContextCmd,
    },
    /// The tokens this server lets in, each shown once and kept as its hash
    Token {
        #[command(subcommand)]
        cmd: TokenCmd,
    },
    /// Start the machine's daemon at login, kept up by a watchdog
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
    /// For connectors: print a line each time a file or folder changes; or
    /// run a command once the watch is set, and again after each change
    Watch {
        path: std::path::PathBuf,
        /// After `--`: the command to run (no shell); its output is the connector's
        #[arg(last = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// For connectors: the browser, kept on a page
    Browser {
        #[command(subcommand)]
        cmd: BrowserCmd,
    },
    /// For connectors: take webhooks from outside, each checked against a secret
    Webhook {
        /// The address to listen on; a tunnel or proxy on this machine reaches 127.0.0.1
        #[arg(long, default_value = "127.0.0.1")]
        host: std::net::IpAddr,
        /// The port to listen on
        #[arg(long)]
        port: u16,
        /// A path, its check (token, hmac or twilio) and its secret's name: /github:hmac:GITHUB_SECRET
        #[arg(long = "hook", value_name = "PATH:CHECK:NAME")]
        hooks: Vec<String>,
    },
    /// Keeps `itero daemon` running: restarts it when it exits, stops
    /// answering or a pass hangs (what the service runs)
    #[command(hide = true)]
    Watchdog {
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
        /// Keep `itero daemon --all` running: every workspace on the machine's list
        #[arg(long)]
        all: bool,
        /// With --all: the daemon's settings from this file, not the config folder's server.json
        #[arg(long, requires = "all", value_name = "PATH")]
        config: Option<std::path::PathBuf>,
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
pub enum BrowserCmd {
    /// For connectors: keep a page open and print a line each time something happens on it
    Watch {
        /// The page: an http or https address
        url: String,
        /// A line when the target's text changes, after a second of quiet: a name, or {"css": "..."}
        #[arg(long, value_name = "TARGET")]
        changed: Vec<String>,
        /// A line when the target comes onto the page: a name, or {"css": "..."}
        #[arg(long, value_name = "TARGET")]
        appeared: Vec<String>,
        /// A line when the page goes to another address, reloads or changes in place
        #[arg(long)]
        navigated: bool,
        /// A line for each JSON response whose address contains the text
        #[arg(long, value_name = "TEXT")]
        responses: Vec<String>,
        /// Load the page again every so many seconds, for one that changes only when loaded
        #[arg(long, value_name = "SECONDS")]
        reload: Option<u64>,
        /// In a headless Chrome, not a window
        #[arg(long)]
        headless: bool,
    },
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
    /// The values #ai steps leave out: each one and where it comes from
    Defaults {
        #[command(subcommand)]
        cmd: Option<DefaultsCmd>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DefaultsCmd {
    /// Sets the machine's: a JSON object of #ai's arg names; null takes one out
    Set { values: String },
}

#[derive(Subcommand)]
pub enum ModelsCmd {
    /// The models found: LM Studio's, Hugging Face's cache, and itero's own
    List {
        /// Only those with this word in their name (any case)
        word: Option<String>,
        /// Sort by this column; numbers smallest first
        #[arg(long, value_enum, default_value_t)]
        sort: ai::Sort,
        /// The other way round
        #[arg(long)]
        reverse: bool,
        /// Only those under this licence; give it again for another (mit, apache-2.0)
        #[arg(long = "licence", value_name = "ID")]
        licences: Vec<String>,
        /// Only those of this maker, like unsloth or local
        #[arg(long, value_name = "ORG")]
        maker: Option<String>,
        /// Only those of this architecture, like qwen35
        #[arg(long, value_name = "NAME")]
        arch: Option<String>,
        /// Only those that think
        #[arg(long)]
        thinks: bool,
        /// Only those that run on this machine
        #[arg(long)]
        runs: bool,
        /// Only those this big or smaller, like 8GB or 500MB
        #[arg(long, value_name = "SIZE")]
        max_size: Option<String>,
        /// Only those with this many parameters or fewer, like 9B
        #[arg(long, value_name = "PARAMS")]
        max_params: Option<String>,
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
    /// Stop a run where it is: its running steps killed, no undo, no error list
    Stop {
        /// The run, by id or name
        #[arg(add = complete::runs())]
        run: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ActionCmd {
    /// Each action and its files
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
        #[arg(long, default_value_t = crate::answer::kinds::ERRORS_SHOWN)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum EventCmd {
    /// The events for the workspace no daemon has handled yet, oldest first
    List {
        #[arg(long)]
        json: bool,
    },
    /// Take back an event no daemon has handled yet: it starts and reaches nothing
    Cancel {
        /// Its id, as itero ws emit printed it
        #[arg(add = complete::queued())]
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ConnectorCmd {
    /// Each connector, once for each set of args the workflows give it
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceCmd {
    /// Add this workspace to the machine's list: the machine's daemon runs it
    Add,
    /// The workspace: its identity, meta and limits, and the daemon running it
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Read every workflow, action and connector, before anything runs, and say what is wrong
    Check {
        #[arg(long)]
        json: bool,
    },
    /// Where a workflow, action or connector of this name is (#name: the action only)
    Which {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Send an event to this workspace: it starts runs, and reaches the runs waiting for it
    Emit(EmitArgs),
    /// Keep a secret for this workspace alone: its $$NAME finds it before the machine's (asks for it, without echo)
    Secret {
        /// Its name: a letter or _, then letters, digits or _
        name: String,
        /// Read it from this file (its last newline left out)
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// The workflows and their runs
    #[command(visible_alias = "wf")]
    Workflow {
        #[command(subcommand)]
        cmd: WorkflowCmd,
    },
    /// The actions, the ones that break, and their repeat
    #[command(visible_alias = "ac")]
    Action {
        #[command(subcommand)]
        cmd: ActionCmd,
    },
    /// The connectors, and their args
    /// The events no daemon has handled yet, and cancelling one
    #[command(visible_alias = "ev")]
    Event {
        #[command(subcommand)]
        cmd: EventCmd,
    },
    #[command(visible_alias = "cn")]
    Connector {
        #[command(subcommand)]
        cmd: ConnectorCmd,
    },
    /// Take a workspace off the list (this one, or one by name); its folder stays
    Remove { name: Option<String> },
    /// The machine's workspaces: name, folder, state and last run
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ContextCmd {
    /// Add one, checked first: its server's URL; its token is asked for (or ITERO_TOKEN)
    Add {
        /// Its name: letters, digits, - and _
        name: String,
        /// Its server's address, like https://itero.example.com
        url: String,
        /// The workspace its commands mean when --ws names none
        #[arg(long, value_name = "NAME")]
        ws: Option<String>,
    },
    /// The one in use from now on (local: this machine)
    Use {
        #[arg(add = complete::contexts())]
        name: String,
    },
    /// Each one, the current one marked
    List {
        #[arg(long)]
        json: bool,
    },
    /// Take one out
    Remove {
        #[arg(add = complete::contexts())]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum TokenCmd {
    /// Make one: printed once; the server keeps only its hash
    Create {
        /// Its name, to tell who comes in and to revoke it: letters, digits, - and _
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Each one's name and when it was made, never a token
    List {
        #[arg(long)]
        json: bool,
    },
    /// Take one out: it lets nobody in from then on
    Revoke { name: String },
}

#[derive(Subcommand)]
pub enum ServiceCmd {
    /// Run the machine's daemon (every workspace on its list) as a service, at login
    Install,
    /// Stop and remove the service
    Uninstall,
    /// The service's state and the daemon's health
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Start the installed service
    Start,
    /// Stop the service; it starts again at login
    Stop,
    /// Stop and start the service: after an upgrade, every workspace on the new itero
    Restart,
}

/// Lists a command's subcommands, and theirs, by name in help: one display
/// order for all, which clap breaks by name.
fn by_name(c: clap::Command) -> clap::Command {
    c.mut_subcommands(|s| by_name(s.display_order(0)))
}

/// Lists each command's subcommands with their aliases after the name,
/// `action (ac)`, as clap does not; clap's `[aliases: ac]` stays after the
/// text.
fn with_aliases(mut c: clap::Command) -> clap::Command {
    c.build();
    let mut shown: Vec<&clap::Command> = c.get_subcommands().filter(|s| !s.is_hide_set()).collect();
    // by name, as `by_name` orders them, clap's own `help` last
    shown.sort_by_key(|s| (s.get_name() == "help", s.get_name().to_string()));
    let subs: Vec<(String, String)> = shown
        .into_iter()
        .map(|s| {
            let aliases: Vec<&str> = s.get_visible_aliases().collect();
            let mut about = s.get_about().map(|a| a.to_string()).unwrap_or_default();
            let name = if aliases.is_empty() {
                s.get_name().to_string()
            } else {
                about = format!("{about} [aliases: {}]", aliases.join(", "));
                format!("{} ({})", s.get_name(), aliases.join(", "))
            };
            (name, about)
        })
        .collect();
    if subs.is_empty() {
        return c;
    }
    let wide = subs
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let list: Vec<String> = subs
        .iter()
        .map(|(n, a)| format!("  {n:<wide$}  {a}"))
        .collect();
    // {all-args} would list the commands again: the options alone
    let template = format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n\nCommands:\n{}\n\nOptions:\n{{options}}{{after-help}}",
        list.join("\n")
    );
    c.help_template(template).mut_subcommands(with_aliases)
}

/// A command that moved under `itero ws`: an error naming its new form.
fn moved(new: &str, rest: &[String]) -> Result<()> {
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    bail!("moved: itero ws {}", [&[new][..], &rest].concat().join(" "))
}

/// The command line, as parsed and as the shell completes it.
fn command() -> clap::Command {
    use clap::CommandFactory;
    with_aliases(by_name(Cli::command()))
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
        Ok(p) if p.delivered + p.worked + p.lost + p.stopped + p.deferred + p.elsewhere > 0 => {
            eprintln!(
                "{} events, {} jobs, {} lost (run again), {} stopped, {} waiting, {} running elsewhere",
                p.delivered, p.worked, p.lost, p.stopped, p.deferred, p.elsewhere
            )
        }
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

/// When the daemon is asked to stop (SIGINT, SIGTERM), calls `first` (its
/// connectors end, while what they use is still there), stops itero-ai
/// and the browser, calls `last` (each workspace's last line,
/// `daemon.stopped`) and ends the process.
fn stop_on_signal(first: impl FnOnce() + Send + 'static, last: impl FnOnce() + Send + 'static) {
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
        first();
        crate::ai::keeper::Keeper::global().stop();
        crate::browser::keeper::Keeper::global().stop();
        last();
        std::process::exit(0);
    });
}

/// `itero browser watch`: it ends when it is stopped (SIGTERM, as the
/// daemon stops a connector), when its stdin closes (the daemon has gone),
/// or when watching fails; its window goes with it, however it ends.
fn browser_watch(o: crate::browser::watch::Options) -> Result<()> {
    // once the window is there: an error before it is said, not cut short
    let ran = crate::browser::watch::run(o, || end_with_stdin(|| {}));
    if ran.is_err() {
        // a daemon stopping ends its Chrome before it closes our stdin:
        // give it the moment, so its stop is quiet, not an error
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    ran.map_err(|e| anyhow!(e))
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
/// The daemon going away closes stdin: a command a connector runs then ends.
/// Ends the process once its stdin closes, calling `last` first.
fn end_with_stdin(last: impl FnOnce() + Send + 'static) {
    std::thread::spawn(|| {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        last();
        std::process::exit(0);
    });
}

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
    health_at(&format!("http://127.0.0.1:{port}/"))
}

/// The `/health` a daemon's API at `api` (ending in `/`) gives.
fn health_at(api: &str) -> Option<serde_json::Value> {
    crate::net::client(std::time::Duration::from_secs(3))
        .ok()?
        .get(format!("{api}health"))
        .send()
        .and_then(|r| r.json())
        .ok()
}

/// The daemon running a workspace, and where its API is.
struct Answering {
    /// The API's address, ending in `/`.
    api: String,
    port: u16,
    /// Run by the machine's daemon, not the workspace's own.
    machine: bool,
    /// Its `/health`.
    health: serde_json::Value,
}

/// The daemon that runs this workspace, when one answers for its journal:
/// the machine's, under `/ws/<name>/`, else the workspace's own on its port.
fn daemon_for(
    ws: &crate::workspace::Workspace,
    j: &crate::engine::journal::Journal,
) -> Result<Option<Answering>> {
    let me = j.workspace()?;
    let mine = |h: &serde_json::Value| h["workspace"]["id"] == me.id.as_str();
    if let Some(port) = crate::engine::http::machine_port() {
        let api = format!("http://127.0.0.1:{port}/ws/{}/", me.name);
        if let Some(health) = health_at(&api).filter(mine) {
            return Ok(Some(Answering {
                api,
                port,
                machine: true,
                health,
            }));
        }
    }
    Ok(ws.daemon_port().and_then(|port| {
        let api = format!("http://127.0.0.1:{port}/");
        health_at(&api).filter(mine).map(|health| Answering {
            api,
            port,
            machine: false,
            health,
        })
    }))
}

/// Whether a daemon runs this workspace: one holds its lock, served or
/// not (`--no-serve`).
fn daemon_running(ws: &crate::workspace::Workspace) -> Result<bool> {
    crate::engine::lock::held(ws).map_err(anyhow::Error::msg)
}

/// The command, when it is about this machine and so cannot go to a
/// context's server: its name as typed.
fn here_only(cmd: &Cmd) -> Option<&'static str> {
    Some(match cmd {
        Cmd::Workspace { cmd, .. } => match cmd {
            WorkspaceCmd::Add => "ws add",
            WorkspaceCmd::Remove { .. } => "ws remove",
            WorkspaceCmd::Secret { .. } => "ws secret",
            WorkspaceCmd::List { .. }
            | WorkspaceCmd::Info { .. }
            | WorkspaceCmd::Check { .. }
            | WorkspaceCmd::Which { .. }
            | WorkspaceCmd::Emit(_)
            | WorkspaceCmd::Workflow { .. }
            | WorkspaceCmd::Action { .. }
            | WorkspaceCmd::Event { .. }
            | WorkspaceCmd::Connector { .. } => return None,
        },
        // the server's daemon, over a context
        Cmd::Info { .. } => return None,
        // they only say where a command moved
        Cmd::MovedCheck { .. }
        | Cmd::MovedWhich { .. }
        | Cmd::MovedWorkflow { .. }
        | Cmd::MovedAction { .. }
        | Cmd::MovedConnector { .. } => return None,
        Cmd::Daemon { .. } => "daemon",
        Cmd::Watchdog { .. } => "watchdog",
        Cmd::Manager => "manager",
        Cmd::Worker => "worker",
        Cmd::Service { .. } => "service",
        Cmd::Emit(_) => "emit",
        Cmd::Secret { .. } => "secret",
        Cmd::Ai { .. } => "ai",
        Cmd::Completion { .. } => "completion",
        Cmd::Context { .. } => "context",
        Cmd::Token { .. } => "token",
        Cmd::NotificationClicked { .. } => "notification-clicked",
        Cmd::Watch { .. } => "watch",
        Cmd::Browser { .. } => "browser",
        Cmd::Webhook { .. } => "webhook",
    })
}

fn dispatch(cli: Cli) -> Result<()> {
    let workspace = || crate::workspace::find(&std::env::current_dir()?);
    let context = cli.context.clone();
    // only a context given to it: the one in use is for the commands that
    // can go to a server, and these are about this machine by nature
    if let Some(name) = context.as_deref().filter(|n| *n != crate::context::LOCAL)
        && let Some(what) = here_only(&cli.cmd)
    {
        bail!("itero {what} works on this machine: run it on {name}'s server");
    }
    if matches!(
        cli.cmd,
        Cmd::Daemon { .. } | Cmd::Manager | Cmd::Worker | Cmd::Watchdog { .. }
    ) {
        ignore_sigpipe();
    }
    match cli.cmd {
        Cmd::Daemon {
            all: true,
            config,
            no_serve,
            ..
        } => {
            // its settings, then its port: a daemon that cannot listen
            // starts nothing
            let server = crate::server::Server::read(config.as_deref())?;
            if let (Some(at), true) = (server.listen, no_serve) {
                bail!("server.json opens the API on {at}, and --no-serve serves none");
            }
            // opened, a token must be able to reach it
            if let Some(at) = server.listen
                && crate::token::none()?
            {
                bail!(
                    "listen {at} opens the API, and no token can reach it: itero token create NAME first"
                );
            }
            let listener = if no_serve {
                None
            } else {
                Some(crate::engine::http::listen_machine(server.port)?)
            };
            // its certificate read and checked now, as its port is bound
            let tls = match &server.tls {
                Some(t) => Some(crate::server::tls_config(t)?),
                None => None,
            };
            let opened =
                match server.listen {
                    Some(at) => Some(std::net::TcpListener::bind(at).map_err(|e| {
                        anyhow!("cannot listen on {at} (server.json's listen): {e}")
                    })?),
                    None => None,
                };
            crate::ai::keeper::Keeper::global().for_machine();
            crate::browser::keeper::Keeper::global().for_machine();
            let served = match &listener {
                Some(l) => Some(l.local_addr()?.port()),
                None => None,
            };
            let machine = std::sync::Arc::new(crate::engine::machine::Machine::new(
                std::env::current_exe()?,
                report,
                served,
            ));
            {
                let machine = machine.clone();
                let stopper = machine.stopper();
                stop_on_signal(
                    // its connectors end with it, those deaf to stdin and
                    // those of a workspace starting now too
                    move || crate::engine::stop::end(&[stopper], crate::engine::stop::GRACE),
                    move || {
                        let mut logs = vec![];
                        machine.each(|r| logs.push(r.log.clone()));
                        crate::engine::log::last(&logs, "daemon.stopped");
                    },
                );
            }
            eprintln!(
                "itero: running the workspaces on the machine's list ({}), each as it comes due (Ctrl-C stops)",
                crate::machine::list_path()?.display()
            );
            machine.start_scans()?;
            match listener {
                None => machine.follow()?,
                Some(listener) => {
                    {
                        let machine = machine.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = machine.follow() {
                                eprintln!("itero: following the machine's list stopped: {e:#}");
                                std::process::exit(1);
                            }
                        });
                    }
                    let port = listener.local_addr()?.port();
                    eprintln!("itero: serving on http://127.0.0.1:{port}/");
                    let mut served = vec![(
                        crate::engine::http::machine_router(machine.clone(), port),
                        listener,
                        None,
                    )];
                    if let Some(opened) = opened {
                        let at = opened.local_addr()?;
                        eprintln!("itero: opened on {at}");
                        match &server.tls {
                            Some(t) => {
                                eprintln!("itero: HTTPS on {at}, with {}", t.cert.display())
                            }
                            None => eprintln!(
                                "itero: serving without TLS on {at}: tokens cross the network in clear unless a proxy in front adds TLS"
                            ),
                        }
                        served.push((
                            crate::engine::http::opened_router(machine, port),
                            opened,
                            tls,
                        ));
                    }
                    crate::engine::http::serve_all(served)?;
                }
            }
        }
        Cmd::Daemon {
            once,
            now,
            no_serve,
            ..
        } => {
            use crate::engine::daemon;
            let ws = workspace()?;
            let exe = std::env::current_exe()?;
            // one daemon per journal, one pass too: held for its life
            let me = daemon::Daemon::current();
            let lock = crate::engine::lock::take(&ws, &format!("{}, pid {}", me.name, me.pid))
                .map_err(|e| anyhow!(e))?;
            if once {
                let at = match &now {
                    Some(t) => t
                        .parse::<chrono::DateTime<chrono::Utc>>()
                        .map_err(|_| anyhow!("--now takes an RFC 3339 time, not {t}"))?,
                    None => chrono::Utc::now(),
                };
                let pass = daemon::run_once(&ws, at, &exe);
                // a Chrome a #browser step started ends with the pass
                crate::browser::keeper::Keeper::global().stop();
                report(&pass, None);
                pass?;
                return Ok(());
            }
            use crate::engine::http;
            // its port first: a daemon that cannot listen starts nothing
            let listening = if no_serve {
                None
            } else {
                Some(http::listen(&ws)?)
            };
            let served = match &listening {
                Some((l, _)) => Some(l.local_addr()?.port()),
                None => None,
            };
            let running = crate::engine::running::Running::start(
                ws.clone(),
                &exe,
                lock,
                report,
                false,
                served,
                None,
            )?;
            if let Some((_, Some(note))) = &listening {
                running
                    .log
                    .write("port.changed", serde_json::json!({ "why": note }));
            }
            let (log, stopper) = (running.log.clone(), running.stopper());
            stop_on_signal(
                // its connectors end with it, those deaf to stdin too
                move || crate::engine::stop::end(&[stopper], crate::engine::stop::GRACE),
                move || crate::engine::log::last(&[log], "daemon.stopped"),
            );
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
                    let api = format!("http://127.0.0.1:{port}/");
                    let routes = http::router(
                        ws.clone(),
                        engine,
                        port,
                        api,
                        crate::answer::workspace::RunBy::Workspace,
                    );
                    http::serve(routes, listener)?;
                }
            }
        }
        Cmd::Emit(args) => emit::all(args)?,
        Cmd::Browser {
            cmd:
                BrowserCmd::Watch {
                    url,
                    changed,
                    appeared,
                    navigated,
                    responses,
                    reload,
                    headless,
                },
        } => browser_watch(crate::browser::watch::Options {
            url,
            changed,
            appeared,
            navigated,
            responses,
            reload,
            headless,
        })?,
        Cmd::Completion { shell } => complete::script(shell, command),
        Cmd::Workspace { ws, cmd } => {
            let chosen = || workspace::chosen(ws.as_deref());
            // where the command's question goes: here, or a context's server
            let client =
                || workspace::target(ws.as_deref(), crate::context::chosen(context.as_deref())?);
            // these are about the list, or this folder: no other workspace
            let none = |what: &str| -> Result<()> {
                match &ws {
                    Some(name) if what == "remove" => {
                        bail!(
                            "itero ws remove takes no --ws: itero ws remove {name} removes it, from any folder"
                        )
                    }
                    Some(_) => {
                        bail!("itero ws {what} takes no --ws: run it in the workspace's folder")
                    }
                    None => Ok(()),
                }
            };
            match cmd {
                WorkspaceCmd::Add => {
                    none("add")?;
                    workspace::add(&workspace()?)?
                }
                WorkspaceCmd::Remove { name } => {
                    none("remove")?;
                    workspace::remove(name.as_deref())?
                }
                WorkspaceCmd::List { json } => {
                    none("list")?;
                    workspace::list(json, crate::context::chosen(context.as_deref())?)?
                }
                WorkspaceCmd::Info { json } => top::info(&client()?, json)?,
                WorkspaceCmd::Check { json } => top::check(&client()?, json)?,
                WorkspaceCmd::Which { name, json } => top::which(&client()?, &name, json)?,
                WorkspaceCmd::Emit(args) => emit::one(&client()?, args)?,
                WorkspaceCmd::Secret { name, file } => top::secret(Some(&chosen()?), &name, file)?,
                WorkspaceCmd::Workflow { cmd } => {
                    let c = client()?;
                    match cmd {
                        WorkflowCmd::List { json } => workflow::list(&c, json)?,
                        WorkflowCmd::Runs {
                            run: Some(run),
                            json,
                            ..
                        } => workflow::run(&c, &run, json)?,
                        WorkflowCmd::Runs {
                            run: None,
                            errors,
                            workflow,
                            action,
                            limit,
                            all,
                            json,
                        } => workflow::runs(
                            &c,
                            &workflow::Filter {
                                errors,
                                workflow,
                                action,
                                limit,
                                all,
                            },
                            json,
                        )?,
                        WorkflowCmd::Stop { run, json } => workflow::stop(&c, &run, json)?,
                    }
                }
                WorkspaceCmd::Action { cmd } => {
                    let c = client()?;
                    match cmd {
                        ActionCmd::List { json } => kinds::actions(&c, json)?,
                        ActionCmd::Errors {
                            action,
                            limit,
                            json,
                        } => kinds::errors(&c, action.as_deref(), limit, json)?,
                    }
                }
                WorkspaceCmd::Event { cmd } => {
                    let c = client()?;
                    match cmd {
                        EventCmd::List { json } => event::list(&c, json)?,
                        EventCmd::Cancel { id, json } => event::cancel(&c, &id, json)?,
                    }
                }
                WorkspaceCmd::Connector { cmd } => {
                    let c = client()?;
                    match cmd {
                        ConnectorCmd::List { json } => kinds::connectors(&c, json)?,
                    }
                }
            }
        }
        Cmd::MovedCheck { rest } => moved("check", &rest)?,
        Cmd::MovedWhich { rest } => moved("which", &rest)?,
        Cmd::MovedWorkflow { rest } => moved("wf", &rest)?,
        Cmd::MovedAction { rest } => moved("ac", &rest)?,
        Cmd::MovedConnector { rest } => moved("cn", &rest)?,
        Cmd::Ai { cmd } => match cmd {
            AiCmd::Models { cmd } => match cmd {
                ModelsCmd::List {
                    word,
                    sort,
                    reverse,
                    licences,
                    maker,
                    arch,
                    thinks,
                    runs,
                    max_size,
                    max_params,
                    json,
                    all,
                } => {
                    let max_params = max_params
                        .map(|p| {
                            ai::billions(&p)
                                .ok_or_else(|| anyhow!("--max-params is like 9B or 500M, not {p}"))
                        })
                        .transpose()?;
                    let q = ai::Query {
                        word,
                        sort,
                        reverse,
                        licences,
                        maker,
                        arch,
                        thinks,
                        runs,
                        max_size: max_size.as_deref().map(ai::bytes_in).transpose()?,
                        max_params,
                    };
                    ai::list(json, all, &q)?
                }
                ModelsCmd::Add { name, yes } => ai::add(&name, yes)?,
                ModelsCmd::Remove { name, delete } => ai::remove(&name, delete)?,
            },
            AiCmd::Inspect { file, json } => ai::inspect(&file, json)?,
            AiCmd::Defaults { cmd, json } => match cmd {
                Some(DefaultsCmd::Set { values }) => ai::defaults_set(&values)?,
                None => ai::defaults(json)?,
            },
        },
        Cmd::Secret { name, file } => top::secret(None, &name, file)?,
        Cmd::Info { json } => top::machine_info(json, crate::context::chosen(context.as_deref())?)?,
        Cmd::Watch { path, command } => {
            let watch = crate::connectors::Watch::new(&path)?;
            end_with_stdin(crate::connectors::end_run);
            if command.is_empty() {
                watch.run(&mut std::io::stdout().lock())?;
            } else {
                // the daemon stops a connector with SIGTERM: its run ends too
                stop_on_signal(|| {}, crate::connectors::end_run);
                watch.run_command(&command)?;
            }
        }
        Cmd::Webhook { host, port, hooks } => {
            let webhook =
                crate::connectors::webhook::Webhook::new(workspace()?, host, port, &hooks)?;
            end_with_stdin(|| {});
            webhook.run()?;
        }
        Cmd::Token { cmd } => match cmd {
            TokenCmd::Create { name, json } => token::create(&name, json)?,
            TokenCmd::List { json } => token::list(json)?,
            TokenCmd::Revoke { name } => token::revoke(&name)?,
        },
        Cmd::Context { cmd } => match cmd {
            ContextCmd::Add { name, url, ws } => context::add(&name, &url, ws)?,
            ContextCmd::Use { name } => context::use_(&name)?,
            ContextCmd::List { json } => context::list(json)?,
            ContextCmd::Remove { name } => context::remove(&name)?,
        },
        Cmd::Service { cmd } => {
            use crate::engine::service;
            let label = service::machine_label();
            match cmd {
                ServiceCmd::Install => top::service_install()?,
                ServiceCmd::Uninstall => {
                    if service::uninstall(&label)? {
                        println!(
                            "stopped and removed: the workspaces run only while an itero daemon runs"
                        );
                    } else {
                        println!("no service for this machine");
                    }
                }
                ServiceCmd::Status { json } => top::service_status(json)?,
                ServiceCmd::Start => {
                    service::start(&label)?;
                    println!("started");
                }
                ServiceCmd::Stop => {
                    service::stop(&label)?;
                    println!(
                        "stopped; it starts again at login (itero service start starts it now)"
                    );
                }
                ServiceCmd::Restart => {
                    service::stop(&label)?;
                    service::start(&label)?;
                    println!("restarted: every workspace runs on this itero");
                }
            }
        }
        Cmd::Watchdog {
            check_every,
            misses,
            stale_after,
            log,
            all,
            config,
        } => {
            use crate::engine::watchdog::{Options, Rules, Target, run};
            let target = if all {
                Target::Machine { config }
            } else {
                Target::Workspace(Box::new(workspace()?))
            };
            #[cfg(unix)]
            stop_on_signal(|| {}, crate::engine::watchdog::stop_daemon);
            // the path it was started by: a service starts it by an
            // absolute one, which an upgrade points at a new file
            let exe = std::env::args_os()
                .next()
                .map(std::path::PathBuf::from)
                .filter(|p| p.is_absolute())
                .map_or_else(std::env::current_exe, Ok)?;
            run(&Options {
                binary: crate::engine::watchdog::Binary::now(&exe)?,
                args: std::env::args_os().skip(1).collect(),
                exe,
                target,
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
