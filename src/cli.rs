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

fn dispatch(cli: Cli) -> Result<()> {
    match cli.cmd {
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
