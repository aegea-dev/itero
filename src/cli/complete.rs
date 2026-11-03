//! Tab completion: the shell asks itero itself (`COMPLETE=<shell> itero
//! ...`), so commands, flags and the workspace's names complete as they are
//! now, not as a script generated once had them.
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};

/// The workspace `--ws` names in the words being completed, else the one
/// the shell is in; none outside one.
fn workspace() -> Option<crate::workspace::Workspace> {
    let words: Vec<String> = std::env::args().collect();
    let named = words
        .iter()
        .enumerate()
        .find_map(|(i, w)| match w.as_str() {
            "--ws" => words.get(i + 1).cloned(),
            w => w.strip_prefix("--ws=").map(str::to_string),
        });
    match named {
        // its folder only: a TAB takes no lock on its journal
        Some(name) => crate::engine::machine::folder(&super::workspace::listed(&name).ok()?).ok(),
        None => crate::workspace::find(&std::env::current_dir().ok()?).ok(),
    }
}

fn names(list: impl IntoIterator<Item = String>) -> Vec<CompletionCandidate> {
    list.into_iter().map(CompletionCandidate::new).collect()
}

/// The names on the machine's list.
pub fn workspaces() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let list = crate::machine::list_path()
            .and_then(|p| crate::machine::List::read(&p))
            .unwrap_or_default();
        names(list.workspaces.into_iter().map(|e| e.name))
    })
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

/// The ids of the events no daemon has handled yet.
pub fn queued() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        workspace()
            .map(|ws| names(super::event::ids(&ws)))
            .unwrap_or_default()
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
