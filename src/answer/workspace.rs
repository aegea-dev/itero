//! The workspace as a whole: what is wrong in it (`itero ws check`, `GET
//! /check`), what it is and who runs it (`itero ws info`, `GET /info`),
//! and where a name is (`itero ws which`, `GET /which/{name}`).
use super::Fault;
use super::kinds::Connector;
use crate::engine::daemon::Daemon;
use crate::engine::journal::Journal;
use crate::flow::Name;
use crate::problem::Problem;
use crate::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What `itero ws check` found: each workflow and connector, and what is
/// wrong, every problem at once. A report with errors is still an answer:
/// the CLI fails on it, the route gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub workflows: Vec<Checked>,
    pub connectors: Vec<Connector>,
    /// What is worth knowing and not wrong: a folder not there.
    pub notes: Vec<String>,
    pub errors: Vec<Told>,
    pub warnings: Vec<Told>,
}

/// A problem as told: its message, and where (`<workspace>/<address>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Told {
    pub message: String,
    pub at: String,
}

/// A workflow as check reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checked {
    pub name: String,
    pub id: String,
    pub since: String,
    /// None for a built-in workflow.
    pub file: Option<String>,
    pub description: Option<String>,
    pub from: Option<String>,
    pub until: Option<String>,
    pub off: bool,
    pub internal: bool,
    /// Its other keys, as written.
    pub other: Map<String, Value>,
    pub runs: usize,
    pub last_run: Option<LastRun>,
    pub uses: Vec<Use>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastRun {
    pub name: String,
    pub status: String,
    /// When it ended, or started if it has not.
    pub at: String,
}

/// What a workflow's steps run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Use {
    Workflow {
        workflow: String,
    },
    BuiltIn {
        action: String,
        built_in: bool,
        /// Where `#ai` sends, a place for each step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sends: Option<Vec<String>>,
    },
    File {
        action: String,
        file: String,
        /// What it needs that is not there.
        notes: Vec<String>,
    },
}

/// A problem as told: its address under the workspace's name.
pub fn told(ws_name: &str, p: &Problem) -> Told {
    let at = match p.at.as_str() {
        "" => ws_name.to_string(),
        a => format!("{ws_name}/{a}"),
    };
    Told {
        message: p.message.clone(),
        at,
    }
}

/// Reads every workflow, action and connector and says what is wrong:
/// every problem, each with its address; and what it found.
pub fn check(ws: &Workspace) -> Result<Report, Fault> {
    let mut notes = vec![];
    for dir in ws.workflows.iter().chain(&ws.actions) {
        if !dir.exists() {
            notes.push(format!("no folder {}", dir.display()));
        }
    }
    let read = ws.read_flows()?;
    let mut errors = ws.manifest_problems(&read)?;
    let mut flows = read.flows;
    let files = ws.flow_files()?;
    let mut named: Vec<String> = files.keys().cloned().collect();
    named.extend(read.written.keys().cloned());
    errors.extend(read.problems);
    // the internal ones last, apart
    flows.sort_by_key(|(_, f)| f.meta.internal);
    let j = Journal::open(ws)?;
    let ws_name = j.workspace()?.name;
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    // the PATH an action runs with
    let exe = std::env::current_exe().map_err(anyhow::Error::from)?;
    let path = crate::connectors::supervise::path_with(&exe);
    // where an #ai step without a url of its own sends
    let machine = crate::ai::defaults::path()
        .and_then(|p| crate::ai::defaults::read(&p))
        .unwrap_or_default();
    let mut warnings: Vec<Problem> = crate::engine::secrets::shadowed(ws)?
        .iter()
        .map(|n| Problem::new("", crate::engine::secrets::shadow_warning(n)))
        .collect();
    warnings.extend(
        ws.limits
            .warnings()
            .into_iter()
            .map(|w| Problem::new("limits", w)),
    );
    // an arg a built-in does not read: likely a mix-up, not an error (it
    // still joins the state)
    let built_ins: [(&str, &[&str]); 3] = [
        (
            crate::engine::worker::NOTIFY,
            &crate::engine::worker::NOTIFY_ARGS,
        ),
        (crate::engine::worker::AI, &crate::ai::step::ARGS),
        (crate::engine::worker::BROWSER, &crate::browser::step::ARGS),
    ];
    // #ai's own messages name it: the address says the step
    let ai = |m: String| m.strip_prefix("#ai: ").map(String::from).unwrap_or(m);
    // every step that can be read, in every file, whatever else is wrong
    // the actions each workflow runs warm, shown so in its listing
    let mut warm_uses = std::collections::BTreeSet::new();
    for (name, v) in &read.written {
        for (at, step, args, warm) in crate::flow::addressed(v) {
            let (step, args) = (&step, &args);
            let at = format!("workflows/{name}/{at}");
            match step {
                Name::Action { name: a, version } => {
                    let built_in =
                        crate::engine::worker::BUILT_IN.contains(&a.as_str()) && version.is_none();
                    let file = if built_in {
                        None
                    } else {
                        match ws.action_file(a, version.as_deref()) {
                            Ok(f) => Some(f),
                            Err(e) => {
                                errors.push(Problem::new(at.clone(), format!("{e:#}")));
                                None
                            }
                        }
                    };
                    // a warm step runs a Python or Node action's file
                    if warm {
                        let why = match &file {
                            _ if built_in => Some(format!(
                                "warm: only Python and Node actions can be warm: #{a} is built in"
                            )),
                            Some(f) => {
                                let first =
                                    crate::engine::worker::first_line(f).unwrap_or_default();
                                crate::engine::warm::runtime(&first).err()
                            }
                            None => None,
                        };
                        match why {
                            Some(why) => errors.push(Problem::new(at.clone(), why)),
                            None if file.is_some() => {
                                warm_uses.insert((name.clone(), a.clone()));
                            }
                            None => {}
                        }
                    }
                    for (action, taken) in built_ins {
                        if a != action || !built_in {
                            continue;
                        }
                        for arg in crate::flow::arg_names_of(args) {
                            if !taken.contains(&arg.as_str()) {
                                let (last, rest) =
                                    taken.split_last().expect("a built-in takes args");
                                warnings.push(Problem::new(
                                    at.clone(),
                                    format!(
                                        "#{action} takes {} and {last}, not {arg}",
                                        rest.join(", ")
                                    ),
                                ));
                            }
                        }
                    }
                    if built_in
                        && a == crate::engine::worker::BROWSER
                        && let Err(e) = crate::browser::step::check(args)
                    {
                        errors.push(Problem::new(at.clone(), format!("#browser: {e}")));
                    }
                    if built_in && a == crate::engine::worker::AI {
                        match crate::ai::step::check(args) {
                            Ok(seen) => warnings.extend(
                                crate::ai::step::warnings(&seen)
                                    .into_iter()
                                    .map(|w| Problem::new(at.clone(), ai(w))),
                            ),
                            Err(e) => errors.push(Problem::new(at.clone(), ai(e))),
                        }
                    }
                }
                Name::Workflow(w) => {
                    if !named.contains(w) {
                        errors.push(Problem::new(at.clone(), format!("no workflow {w}")));
                    }
                }
            }
        }
    }
    let mut workflows = vec![];
    for (name, f) in &flows {
        let id = j.workflow_id(name, &now)?;
        let m = &f.meta;
        let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
        let mut uses = vec![];
        for u in f.uses() {
            match u {
                Name::Action { name: a, version } => match ws.action_file(&a, version.as_deref()) {
                    Ok(file) => {
                        let first = crate::engine::worker::first_line(&file).unwrap_or_default();
                        let mut notes = crate::needs::notes(&file, &first, Some(&path));
                        if warm_uses.contains(&(name.clone(), a.clone())) {
                            notes.push("warm".into());
                        }
                        uses.push(Use::File {
                            action: a,
                            file: file.display().to_string(),
                            notes,
                        });
                    }
                    Err(_)
                        if crate::engine::worker::BUILT_IN.contains(&a.as_str())
                            && version.is_none() =>
                    {
                        let sends = (a == crate::engine::worker::AI).then(|| {
                            f.args_of(&a)
                                .into_iter()
                                .map(|x| crate::ai::step::place(x, &machine))
                                .collect()
                        });
                        uses.push(Use::BuiltIn {
                            action: a,
                            built_in: true,
                            sends,
                        })
                    }
                    // told among the errors
                    Err(_) => {}
                },
                Name::Workflow(w) => uses.push(Use::Workflow { workflow: w }),
            }
        }
        workflows.push(Checked {
            name: name.clone(),
            id: id.id,
            since: id.created,
            file: files.get(name).map(|f| ws.shown(f)),
            description: m.description.clone(),
            from: m.from.map(crate::engine::daemon::stamp),
            until: m.until.map(crate::engine::daemon::stamp),
            off: m.off,
            internal: m.internal,
            other: m.other.clone(),
            runs: runs.len(),
            last_run: runs.first().map(|r| LastRun {
                name: r.name.clone(),
                status: crate::engine::steps::status(&r.runtime).to_string(),
                at: r.ended.clone().unwrap_or_else(|| r.started.clone()),
            }),
            uses,
        });
    }
    let connectors = ws
        .connector_files()?
        .into_iter()
        .map(|(kind, file)| Connector {
            kind,
            file: file.display().to_string(),
            args: None,
        })
        .collect();
    let told_all = |ps: &[Problem]| ps.iter().map(|p| told(&ws_name, p)).collect();
    Ok(Report {
        workflows,
        connectors,
        notes,
        errors: told_all(&errors),
        warnings: told_all(&warnings),
    })
}

/// What `itero ws info` says: the workspace, its limits, the daemon
/// running it, and how notifications are shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub workspace: About,
    pub limits: Limits,
    pub daemon: Runner,
    pub notifications: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct About {
    pub id: String,
    /// The manifest's name, once given; until then, itero's.
    pub name: String,
    pub created: String,
    pub path: String,
    /// The itero that made its journal.
    pub itero: String,
    /// The manifest's other keys about it.
    pub meta: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub workflows: usize,
    pub runs: usize,
    pub steps: usize,
    pub space: u64,
    /// How long runs are kept, as `30days`.
    pub keep: String,
    pub log: u64,
    pub logs: usize,
}

/// The daemon running a workspace, or why none does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Runner {
    Running {
        #[serde(flatten)]
        daemon: Daemon,
        /// Always true: told apart from `Stopped` by it.
        running: bool,
        port: u16,
        /// Its API's address, ending in `/`.
        api: String,
        run_by: RunBy,
    },
    Stopped {
        running: bool,
        port: Option<u16>,
        why: String,
    },
}

/// Whose daemon runs a workspace: the machine's (`itero daemon`) or its
/// own (`itero ws daemon`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunBy {
    Machine,
    Workspace,
}

/// The workspace, with `daemon` as its asker knows it: the CLI asks the
/// ports, a route says it is the daemon itself.
pub fn info(ws: &Workspace, daemon: Runner) -> Result<Info, Fault> {
    let w = Journal::open(ws)?.workspace()?;
    let named = ws.meta.get("name").and_then(|n| n.as_str());
    let meta = ws
        .meta
        .iter()
        .filter(|(k, _)| *k != "name")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let l = &ws.limits;
    Ok(Info {
        workspace: About {
            id: w.id,
            name: named.map(String::from).unwrap_or(w.name),
            created: w.created,
            path: ws.root.display().to_string(),
            itero: w.itero,
            meta,
        },
        limits: Limits {
            workflows: l.workflows,
            runs: l.runs,
            steps: l.steps,
            space: l.space,
            keep: humantime::format_duration(l.keep).to_string(),
            log: l.log,
            logs: l.logs,
        },
        daemon,
        notifications: crate::engine::worker::notifications(),
    })
}

/// Something with a name, and where it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub kind: Kind,
    /// Its file, or `built in`.
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Workflow,
    Action,
    Connector,
}

/// Where the workflow, action (each version) and connector named `name`
/// are; `#name` looks among the actions only. Nothing so named is a
/// mistake.
pub fn which(ws: &Workspace, name: &str) -> Result<Vec<Found>, Fault> {
    let (action_only, name) = match name.strip_prefix('#') {
        Some(n) => (true, n),
        None => (false, name),
    };
    let found = |kind, path| Found { kind, path };
    let mut out = vec![];
    if !action_only {
        match ws.flow_files()?.get(name) {
            Some(f) => out.push(found(Kind::Workflow, ws.shown(f))),
            None if crate::workspace::BUILT_IN.iter().any(|(n, _)| *n == name) => {
                out.push(found(Kind::Workflow, "built in".into()))
            }
            None => {}
        }
    }
    let actions: Vec<String> = ws
        .action_files()?
        .into_iter()
        .filter(|(file, _)| file == name || file.starts_with(&format!("{name}@")))
        .map(|(_, f)| ws.shown(&f))
        .collect();
    if actions.is_empty() && crate::engine::worker::BUILT_IN.contains(&name) {
        out.push(found(Kind::Action, "built in".into()));
    }
    out.extend(actions.into_iter().map(|a| found(Kind::Action, a)));
    if !action_only {
        for (kind, f) in ws.connector_files()? {
            if kind == name {
                out.push(found(Kind::Connector, ws.shown(&f)));
            }
        }
    }
    if out.is_empty() {
        return Err(Fault::Missing(format!(
            "nothing named {name}: itero ws wf list and itero ws check list them"
        )));
    }
    Ok(out)
}
