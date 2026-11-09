//! Each workspace command's answer, computed once: its CLI command and
//! its API route both call the function here, so the two never say
//! different things. The CLI reaches them through a [`Client`]: on this
//! machine, in the same process, with no daemon needed.
pub mod events;
pub mod runs;
pub mod workflows;

use crate::workspace::Workspace;

/// Why there is no answer: the asker's mistake, or what broke here.
#[derive(Debug)]
pub enum Fault {
    /// A question that cannot be asked so: a bad filter (400).
    Bad(String),
    /// What it names is not there: a run, a workflow (404).
    Missing(String),
    /// It cannot be done now, and why (409), with what is in the way
    /// (the runs an event started, say) as the route's fields.
    Conflict(String, serde_json::Map<String, serde_json::Value>),
    /// What broke here: the journal, a file (500).
    Internal(anyhow::Error),
}

impl Fault {
    /// The HTTP status a route answers with.
    pub fn status(&self) -> u16 {
        match self {
            Fault::Bad(_) => 400,
            Fault::Missing(_) => 404,
            Fault::Conflict(..) => 409,
            Fault::Internal(_) => 500,
        }
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Bad(m) | Fault::Missing(m) | Fault::Conflict(m, _) => f.write_str(m),
            Fault::Internal(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for Fault {}

impl From<anyhow::Error> for Fault {
    fn from(e: anyhow::Error) -> Fault {
        Fault::Internal(e)
    }
}

/// Where the CLI's questions go: this machine's workspace, answered in
/// the same process.
pub enum Client {
    Local(Workspace),
}

impl Client {
    pub fn runs(&self, q: &runs::Query) -> Result<Vec<runs::Row>, Fault> {
        match self {
            Client::Local(ws) => runs::list(ws, q),
        }
    }

    pub fn run(&self, run: &str) -> Result<runs::Story, Fault> {
        match self {
            Client::Local(ws) => runs::one(ws, run),
        }
    }

    pub fn stop(&self, run: &str) -> Result<runs::Stopped, Fault> {
        match self {
            Client::Local(ws) => runs::stop(ws, run),
        }
    }

    pub fn queued(&self) -> Result<Vec<crate::engine::journal::Queued>, Fault> {
        match self {
            Client::Local(ws) => events::queued(ws),
        }
    }

    pub fn cancel(&self, id: &str) -> Result<events::Cancelled, Fault> {
        match self {
            Client::Local(ws) => events::cancel(ws, id),
        }
    }

    pub fn emit(&self, e: &events::Emit) -> Result<events::Emitted, Fault> {
        match self {
            Client::Local(ws) => events::emit(ws, e, || events::nudge(ws)),
        }
    }

    pub fn workflows(&self) -> Result<workflows::List, Fault> {
        match self {
            Client::Local(ws) => workflows::list(ws),
        }
    }
}
