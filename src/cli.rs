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
    },
    /// Check every workflow and action; print each with its version
    Check,
    /// The runs, newest first: id, workflow, state, started, ended
    Runs,
    /// A workflow's memory, as JSON
    Memory { workflow: String },
    /// The workflow manager: one decision, JSON in on stdin, JSON out on stdout
    #[command(hide = true)]
    Manager,
    /// A worker: one job, JSON in on stdin, its output as JSON on stdout
    #[command(hide = true)]
    Worker,
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
            daemon::run_loop(&ws, std::time::Duration::from_secs(interval), &exe, report);
        }
        Cmd::Check => {
            let ws = workspace()?;
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
            for r in j.runs()?.iter().rev() {
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
