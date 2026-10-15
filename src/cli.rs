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
        /// One pass, then stop
        #[arg(long)]
        once: bool,
        /// The time to run at, with --once (RFC 3339; for tests)
        #[arg(long, requires = "once")]
        now: Option<String>,
        /// Seconds between passes
        #[arg(long, default_value_t = 1)]
        interval: u64,
        /// The passes only: no HTTP API
        #[arg(long)]
        no_serve: bool,
    },
    /// Check every workflow and action; print each with its version
    Check,
    /// Send an event: to a run, or to the workspace, where it may start runs
    Emit {
        /// The event's type, like photo.added
        #[arg(value_name = "TYPE")]
        kind: String,
        /// Its data, a JSON object
        #[arg(long)]
        data: Option<String>,
        /// The run it is for; without, it is for the workspace
        #[arg(long)]
        run: Option<String>,
    },
    /// The runs, newest first: id, workflow, state, started, ended
    Runs,
    /// A workflow's memory, as JSON
    Memory { workflow: String },
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

/// What a pass did, on stderr: only when it did something, or failed.
fn report(pass: &Result<crate::engine::daemon::Pass>) {
    match pass {
        Ok(p) if p.delivered + p.worked + p.lost + p.deferred > 0 => eprintln!(
            "{} events, {} jobs, {} lost (run again), {} waiting",
            p.delivered, p.worked, p.lost, p.deferred
        ),
        Ok(_) => {}
        Err(e) => eprintln!("error: {e:#}"),
    }
}

fn dispatch(cli: Cli) -> Result<()> {
    let workspace = || crate::workspace::find(&std::env::current_dir()?);
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
                report(&pass);
                pass?;
                return Ok(());
            }
            eprintln!(
                "itero: running the workflows in {}, a pass every {interval}s (Ctrl-C stops)",
                ws.root.display()
            );
            use crate::engine::http::{self, Engine};
            let (wake, woken) = std::sync::mpsc::channel();
            let engine = std::sync::Arc::new(Engine {
                health: std::sync::Mutex::new(daemon::Health::starting(&daemon::stamp(
                    chrono::Utc::now(),
                ))),
                wake,
            });
            let looping = {
                let (ws, engine) = (ws.clone(), engine.clone());
                let interval = std::time::Duration::from_secs(interval);
                std::thread::spawn(move || {
                    daemon::run_loop(&ws, interval, &exe, &woken, &engine.health, report)
                })
            };
            if no_serve {
                // the loop's sender lives in `engine`: the loop runs for good
                looping
                    .join()
                    .map_err(|_| anyhow!("the daemon's loop stopped unexpectedly"))?;
            } else {
                eprintln!("itero: serving on http://127.0.0.1:{}/", ws.port);
                http::serve(http::router(ws.clone(), engine), ws.port)?;
            }
        }
        Cmd::Emit { kind, data, run } => {
            let ws = workspace()?;
            let data = data
                .map(|d| {
                    serde_json::from_str(&d).map_err(|_| anyhow!("--data takes a JSON object"))
                })
                .transpose()?;
            let now = crate::engine::daemon::stamp(chrono::Utc::now());
            let id = crate::engine::events::emit(&ws, &kind, data, run.as_deref(), &now)?;
            println!("{id}");
        }
        Cmd::Check => {
            let ws = workspace()?;
            for dir in ws.workflows.iter().chain(&ws.actions) {
                if !dir.exists() {
                    eprintln!("note: no folder {}", dir.display());
                }
            }
            for (_, f) in ws.workflows()? {
                let version = crate::workflow::version(&f);
                println!("{}  {}", f.name, crate::workflow::short(&version));
            }
            for (_, d) in ws.actions()? {
                for target in d.targets.keys() {
                    println!("{}.{target}@{}", d.name, d.version);
                }
            }
        }
        Cmd::Runs => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            for r in j.runs_page(None, usize::MAX)? {
                println!(
                    "{}  {:<20} {:<12} {}  {}",
                    r.id,
                    r.workflow,
                    r.runtime.state,
                    r.started,
                    r.ended.as_deref().unwrap_or("-")
                );
            }
        }
        Cmd::Memory { workflow } => {
            let ws = workspace()?;
            let j = crate::engine::journal::Journal::open(&ws)?;
            let m = j.memory(&workflow)?;
            println!("{}", serde_json::to_string_pretty(&m)?);
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
                    println!("API: http://127.0.0.1:{}/", ws.port);
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
                    let health = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(3))
                        .build()?
                        .get(format!("http://127.0.0.1:{}/health", ws.port))
                        .send()
                        .and_then(|r| r.json::<serde_json::Value>());
                    match health {
                        Ok(h) => println!(
                            "daemon: pid {}, {} passes, last {}{}",
                            h["pid"],
                            h["passes"],
                            h["last_pass"].as_str().unwrap_or("none yet"),
                            h["last_error"]
                                .as_str()
                                .map(|e| format!(", last error: {e}"))
                                .unwrap_or_default()
                        ),
                        Err(_) => println!("daemon: no answer on port {}", ws.port),
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
                port: ws.port,
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
            let input: crate::engine::Input =
                serde_json::from_str(&text).map_err(|e| anyhow!("the manager's input: {e}"))?;
            let answer = crate::engine::manager::decide(&input).map_err(|e| anyhow!(e))?;
            println!("{}", serde_json::to_string(&answer)?);
        }
        Cmd::Worker => {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
            let job: crate::engine::Job =
                serde_json::from_str(&text).map_err(|e| anyhow!("the worker's job: {e}"))?;
            use crate::engine::worker::{Failure, LOST_EXIT, run};
            #[cfg(unix)]
            crate::engine::worker::stop_with_parent();
            crate::engine::die_at("worker");
            match run(&job) {
                Ok(output) => {
                    // the job's effect is done; its answer is not yet given
                    crate::engine::die_at("worker-done");
                    crate::engine::die_at(&format!("worker-done:{}", job.action));
                    println!("{}", serde_json::json!({ "output": output }))
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
