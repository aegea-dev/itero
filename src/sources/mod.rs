//! Sources: executables in `sources/` that keep running and tell itero
//! when something happens outside (`docs/design-simple.md`):
//!
//! - each line a source prints is the data of one message, a JSON object;
//!   the message's type is the source's file name (`mail.received`);
//! - its stdin stays open while the daemon runs, and closes when it goes;
//! - its stderr goes to the daemon's log;
//! - `itero` on its PATH is the daemon's own.
//!
//! `itero watch <path>` is there for sources that watch a file or folder.
use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

pub mod supervise;

/// `itero watch <path>`: a line each time a file or folder changes, from
/// the OS's own file events, debounced (quiet for 500 ms, at most 5 s).
pub struct Watch {
    backend: Box<dyn crate::watch::Backend>,
    debouncer: crate::watch::Debouncer,
}

impl Watch {
    /// Sets the watch: what can go wrong goes wrong here.
    pub fn new(path: &std::path::Path) -> Result<Watch> {
        let backend =
            crate::watch::open(path).with_context(|| format!("cannot watch {}", path.display()))?;
        Ok(Watch {
            backend,
            debouncer: crate::watch::Debouncer::new(
                Duration::from_millis(500),
                Duration::from_secs(5),
            ),
        })
    }

    /// Prints the changes, one JSON line each, until the watch fails or
    /// `out` does.
    pub fn run(mut self, out: &mut dyn std::io::Write) -> Result<()> {
        crate::watch::run(self.backend.as_mut(), &mut self.debouncer, |c| {
            writeln!(out, "{}", json!(c))?;
            out.flush()
        })?;
        Ok(())
    }
}
