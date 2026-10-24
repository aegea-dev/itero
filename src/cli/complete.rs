//! Tab completion: the shell asks itero itself (`COMPLETE=<shell> itero
//! ...`), so commands, flags and the workspace's names complete as they are
//! now, not as a script generated once had them.
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};

/// The workspace the shell is in; none outside one.
fn workspace() -> Option<crate::workspace::Workspace> {
    crate::workspace::find(&std::env::current_dir().ok()?).ok()
}

fn names(list: impl IntoIterator<Item = String>) -> Vec<CompletionCandidate> {
    list.into_iter().map(CompletionCandidate::new).collect()
}

pub fn workflows() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        names(
            workspace()
                .and_then(|ws| ws.flows().ok())
                .into_iter()
                .flatten()
                .map(|(n, _)| n),
        )
    })
}

pub fn actions() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let mut all: Vec<String> = workspace()
            .and_then(|ws| ws.action_files().ok())
            .into_iter()
            .flatten()
            .map(|(f, _)| f.split('@').next().unwrap_or(&f).to_string())
            .chain(
                crate::engine::worker::BUILT_IN
                    .iter()
                    .map(|b| b.to_string()),
            )
            .collect();
        all.sort();
        all.dedup();
        names(all)
    })
}

pub fn connectors() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        names(
            workspace()
                .and_then(|ws| ws.connector_files().ok())
                .into_iter()
                .flatten()
                .map(|(n, _)| n),
        )
    })
}

/// The newest runs' names, the ones a person types.
pub fn runs() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Some(ws) = workspace() else {
            return vec![];
        };
        crate::engine::journal::Journal::open(&ws)
            .and_then(|j| j.runs_page(None, None, &[], 50))
            .map(|runs| names(runs.into_iter().map(|r| r.name)))
            .unwrap_or_default()
    })
}

/// Answers the shell when it asks (`COMPLETE` set); else does nothing.
pub fn answer(command: fn() -> clap::Command) {
    clap_complete::env::CompleteEnv::with_factory(command).complete();
}

/// Prints the line a shell sources to complete itero: what the shell's
/// own request (`COMPLETE=<shell> itero`) answers.
pub fn script(shell: super::Shell, command: fn() -> clap::Command) {
    let name = match shell {
        super::Shell::Bash => "bash",
        super::Shell::Zsh => "zsh",
        super::Shell::Fish => "fish",
    };
    // SAFETY: before this short command starts any thread
    unsafe { std::env::set_var("COMPLETE", name) };
    answer(command);
}
