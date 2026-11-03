//! Connectors: executables in `connectors/` that keep running and tell itero
//! when something happens outside:
//!
//! - each line a connector prints is the data of one message, a JSON object;
//!   the message's type is the connector's file name (`mail.received`);
//! - its stdin stays open while the daemon runs, and closes when it goes;
//! - its stderr goes to the daemon's log;
//! - `itero` on its PATH is the daemon's own.
//!
//! `itero watch <path>` is there for connectors that watch a file or folder.
use crate::flow::Listen;
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub mod supervise;

/// A copy of a connector: its file, run with one set of args. The daemon
/// runs one for each different set the workflows' `on` entries give it.
#[derive(Debug, Clone, PartialEq)]
pub struct Copy {
    /// Its event's type (the file's name) and its args, as written.
    pub listen: Listen,
    pub file: PathBuf,
}

impl Copy {
    /// A copy with no args, as a connector no `on` entry gives args runs.
    pub fn plain(kind: &str, file: PathBuf) -> Copy {
        Copy {
            listen: Listen::of(kind),
            file,
        }
    }

    /// How the log and `/health` name it: its type, then its args as
    /// written (`mail.received {"MAILBOX":"sales"}`). No secret: a
    /// `$$NAME` is not looked up there.
    pub fn name(&self) -> String {
        match self.listen.key().as_str() {
            "" => self.listen.kind.clone(),
            key => format!("{} {key}", self.listen.kind),
        }
    }

    /// The source of the events it sends: `connector/<type>`, then
    /// `?<args as written>` when it has args. Which workflows an event
    /// reaches is read from it.
    pub fn source(&self) -> String {
        match self.listen.key().as_str() {
            "" => format!("connector/{}", self.listen.kind),
            key => format!("connector/{}?{key}", self.listen.kind),
        }
    }
}

/// The copies to run: each connector once for every different set of
/// args a workflow's `on` entry gives its event, or once with none when no
/// entry gives it args. A workflow that cannot be read is left out (the
/// daemon's pass tells why).
pub fn copies(ws: &Workspace) -> Result<Vec<Copy>> {
    let flows = ws.read_flows()?.flows;
    let mut out = vec![];
    for (kind, file) in ws.connector_files()? {
        let mut sets: BTreeMap<String, Listen> = BTreeMap::new();
        for (_, f) in &flows {
            for e in f.on.events.iter().filter(|e| e.kind == kind) {
                sets.insert(e.key(), e.clone());
            }
        }
        if sets.is_empty() {
            out.push(Copy::plain(&kind, file));
            continue;
        }
        for (_, listen) in sets {
            out.push(Copy {
                listen,
                file: file.clone(),
            });
        }
    }
    Ok(out)
}

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
