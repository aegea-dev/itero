//! Each workspace command's answer, computed once: its CLI command and
//! its API route both call the function here, so the two never say
//! different things. The CLI reaches them through a [`Client`]: on this
//! machine, in the same process, with no daemon needed.
pub mod apply;
pub mod events;
pub mod kinds;
pub mod machine;
pub mod remote;
pub mod runs;
pub mod workflows;
pub mod workspace;

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
/// the same process, or a workspace on a context's server, over HTTP.
pub enum Client {
    Local(Box<Workspace>),
    Remote(remote::Remote),
}

/// A count as a query parameter.
fn n(v: usize) -> String {
    v.to_string()
}

impl Client {
    pub fn runs(&self, q: &runs::Query) -> Result<Vec<runs::Row>, Fault> {
        match self {
            Client::Local(ws) => runs::list(ws, q),
            Client::Remote(r) => {
                let mut query = vec![];
                for (k, v) in [
                    ("workflow", &q.workflow),
                    ("status", &q.status),
                    ("action", &q.action),
                ] {
                    if let Some(v) = v {
                        query.push((k, v.clone()));
                    }
                }
                // none is all, as here: the route's own default is 50
                query.push(("limit", n(q.limit.unwrap_or(usize::MAX))));
                if q.all {
                    query.push(("all", "true".into()));
                }
                r.get(&["runs"], &query)
            }
        }
    }

    pub fn run(&self, run: &str) -> Result<runs::Story, Fault> {
        match self {
            Client::Local(ws) => runs::one(ws, run),
            Client::Remote(r) => r.get(&["runs", run], &[]),
        }
    }

    pub fn stop(&self, run: &str) -> Result<runs::Stopped, Fault> {
        match self {
            Client::Local(ws) => runs::stop(ws, run),
            Client::Remote(r) => r.post(&["runs", run, "stop"], &[], None, remote::TIMEOUT),
        }
    }

    pub fn queued(&self) -> Result<Vec<crate::engine::journal::Queued>, Fault> {
        match self {
            Client::Local(ws) => events::queued(ws),
            Client::Remote(r) => r.get(&["events"], &[]),
        }
    }

    pub fn cancel(&self, id: &str) -> Result<events::Cancelled, Fault> {
        match self {
            Client::Local(ws) => events::cancel(ws, id),
            Client::Remote(r) => r.delete(&["events", id]),
        }
    }

    pub fn emit(&self, e: &events::Emit) -> Result<events::Emitted, Fault> {
        match self {
            Client::Local(ws) => events::emit(ws, e, || events::nudge(ws)),
            Client::Remote(r) => {
                let body = serde_json::json!({ "type": e.kind, "data": e.data });
                let (query, timeout) = match e.wait {
                    None => (vec![], remote::TIMEOUT),
                    Some(w) => {
                        let secs = w.as_secs() + u64::from(w.subsec_nanos() > 0);
                        if secs > events::MAX_WAIT {
                            return Err(Fault::Bad(format!(
                                "over a context, --wait waits at most {}",
                                humantime::format_duration(std::time::Duration::from_secs(
                                    events::MAX_WAIT
                                ))
                            )));
                        }
                        (vec![("wait", secs.to_string())], w + remote::TIMEOUT)
                    }
                };
                r.post(&["events"], &query, Some(&body), timeout)
            }
        }
    }

    pub fn actions(&self) -> Result<Vec<kinds::Action>, Fault> {
        match self {
            Client::Local(ws) => kinds::actions(ws),
            Client::Remote(r) => r.get(&["actions"], &[]),
        }
    }

    pub fn connectors(&self) -> Result<Vec<kinds::Connector>, Fault> {
        match self {
            Client::Local(ws) => kinds::connectors(ws),
            Client::Remote(r) => r.get(&["connectors"], &[]),
        }
    }

    pub fn errors_by_action(&self, limit: usize) -> Result<Vec<kinds::Breaks>, Fault> {
        match self {
            Client::Local(ws) => kinds::errors_by_action(ws, limit),
            Client::Remote(r) => r.get(&["actions", "errors"], &[("limit", n(limit))]),
        }
    }

    pub fn failures_of(&self, action: &str, limit: usize) -> Result<Vec<kinds::Broke>, Fault> {
        match self {
            Client::Local(ws) => kinds::failures_of(ws, action, limit),
            Client::Remote(r) => r.get(
                &["actions", "errors"],
                &[("action", action.to_string()), ("limit", n(limit))],
            ),
        }
    }

    pub fn workflows(&self) -> Result<workflows::List, Fault> {
        match self {
            Client::Local(ws) => workflows::list(ws),
            Client::Remote(r) => r.get(&["workflows"], &[]),
        }
    }

    pub fn check(&self) -> Result<workspace::Report, Fault> {
        match self {
            Client::Local(ws) => workspace::check(ws),
            Client::Remote(r) => r.get(&["check"], &[]),
        }
    }

    /// The workspace; on this machine, the daemon running it is what
    /// `probe` finds; on a server, the route that answers is that daemon.
    pub fn info(
        &self,
        probe: impl FnOnce(&Workspace) -> Result<workspace::Runner, Fault>,
    ) -> Result<workspace::Info, Fault> {
        match self {
            Client::Local(ws) => workspace::info(ws, probe(ws)?),
            Client::Remote(r) => r.get(&["info"], &[]),
        }
    }

    pub fn which(&self, name: &str) -> Result<Vec<workspace::Found>, Fault> {
        match self {
            Client::Local(ws) => workspace::which(ws, name),
            Client::Remote(r) => r.get(&["which", name], &[]),
        }
    }
}
