//! The browser: the user's real Chrome, driven over the DevTools
//! Protocol (CDP). The daemon keeps it (`keeper`); a `#browser` step acts
//! on its run's tab.
pub mod blockers;
pub mod cdp;
pub mod chrome;
pub mod inspect;
pub mod keeper;
pub mod network;
pub mod page;
pub mod save;
pub mod step;
pub mod target;
pub mod text;
pub mod watch;

use std::path::PathBuf;

/// How a Chrome runs: in a window, or headless where there is no screen
/// or a site does not check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Window,
    Headless,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Window => "window",
            Mode::Headless => "headless",
        }
    }
}

/// The profile of `mode`'s Chrome: logins and passed checks live in the
/// windowed one. A profile is used by one Chrome at a time, so each mode
/// has its own.
pub fn profile_dir(mode: Mode) -> anyhow::Result<PathBuf> {
    Ok(crate::machine::state_dir()?
        .join("browser")
        .join(mode.name()))
}
