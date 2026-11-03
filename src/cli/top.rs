//! The machine and the workspace as a whole: `info`, `service status`, and
//! `ws info`, `ws check`, `ws which`.
use super::out::{show, text};
use crate::engine::journal::Journal;
use crate::flow::Name;
use crate::problem::Problem;
use crate::workspace::Workspace;
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// A manifest entry's problem at the address of what it is about
/// (`action/depo`), its text after the manifest's name, its path and the
/// entry's key taken off: `itero.config.json: limits: ...`.
fn entry_problem(ws: &Workspace, kind: &str, name: &str, why: &str) -> Problem {
    let path = format!("{}: ", ws.root.join(crate::workspace::MANIFEST).display());
    let why = why.strip_prefix(&path).unwrap_or(why);
    let why = why
        .strip_prefix(&format!("{kind}.{name}."))
        .or_else(|| why.strip_prefix(&format!("{kind}.{name}: ")))
        .unwrap_or(why);
    Problem::new(
        format!("{kind}/{name}"),
        format!("{}: {why}", crate::workspace::MANIFEST),
    )
}

/// What `itero ws check` refuses in the manifest: an entry naming no
/// workflow, action or connector, an entry that is wrong, and the folders
/// from before it (`args/`, `config/`). `workflows` are the workflows'
/// names, read or not: a broken file's entry is not missing.
fn check_manifest(ws: &Workspace, workflows: &[String]) -> Result<Vec<Problem>> {
    let mut wrong = vec![];
    for (dir, now) in [
        (
            "args",
            "an action's args are written in the step that runs it",
        ),
        (
            "config",
            "a workflow's values go in .env, a step's args or the event",
        ),
    ] {
        if ws.root.join(dir).is_dir() {
            wrong.push(Problem::new("", format!("{dir}/: is gone: {now}")));
        }
    }
    let actions: Vec<String> = ws
        .action_files()?
        .into_keys()
        .map(|f| f.split('@').next().unwrap_or(&f).to_string())
        .chain(
            crate::engine::worker::BUILT_IN
                .iter()
                .map(|b| b.to_string()),
        )
        .collect();
    let connectors: Vec<String> = ws.connector_files()?.into_iter().map(|(n, _)| n).collect();
    for name in ws.entries("connector")?.keys() {
        if let Err(why) = crate::engine::daemon::connector_args(ws, name) {
            wrong.push(entry_problem(ws, "connector", name, &why));
        }
    }
    for (name, _) in ws.entries("action")? {
        if let Err(why) = ws.no_args(&name) {
            wrong.push(entry_problem(ws, "action", &name, &format!("{why:#}")));
        }
        if let Err(why) = ws.step_limits(&name) {
            wrong.push(entry_problem(ws, "action", &name, &format!("{why:#}")));
        }
    }
    for (kind, known) in [
        ("workflow", workflows.to_vec()),
        ("action", actions),
        ("connector", connectors),
    ] {
        for name in ws.entries(kind)?.keys() {
            if !known.contains(name) {
                wrong.push(Problem::new(
                    format!("{kind}/{name}"),
                    format!("{}: no {kind} {name}", crate::workspace::MANIFEST),
                ));
            }
        }
    }
    Ok(wrong)
}

/// A problem as printed: `<message> (<workspace>/<address>)`.
fn told(ws_name: &str, p: &Problem) -> Value {
    let at = match p.at.as_str() {
        "" => ws_name.to_string(),
        a => format!("{ws_name}/{a}"),
    };
    json!({ "message": p.message, "at": at })
}

/// Reads every workflow, action and connector and says what is wrong:
/// every problem, each with its address, then how many; what it found is
/// printed when nothing is wrong.
pub fn check(ws: &Workspace, json: bool) -> Result<()> {
    let mut notes = vec![];
    for dir in ws.workflows.iter().chain(&ws.actions) {
        if !dir.exists() {
            notes.push(format!("no folder {}", dir.display()));
        }
    }
    let (mut flows, read) = ws.read_flows()?;
    let files = ws.flow_files()?;
    let mut named: Vec<String> = files.keys().cloned().collect();
    named.extend(flows.iter().map(|(n, _)| n.clone()));
    let mut errors = check_manifest(ws, &named)?;
    errors.extend(read);
    // the internal ones last, apart
    flows.sort_by_key(|(_, f)| f.meta.internal);
    let j = Journal::open(ws)?;
    let ws_name = j.workspace()?.name;
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    // the PATH an action runs with
    let path = crate::connectors::supervise::path_with(&std::env::current_exe()?);
    // where an #ai step without a url of its own sends
    let machine = crate::ai::defaults::path()
        .and_then(|p| crate::ai::defaults::read(&p))
        .unwrap_or_default();
    let mut warnings: Vec<Problem> = crate::engine::secrets::shadowed(ws)?
        .iter()
        .map(|n| Problem::new("", crate::engine::secrets::shadow_warning(n)))
        .collect();
    // an arg a built-in does not read: likely a mix-up, not an error (it
    // still joins the state)
    let built_ins: [(&str, &[&str]); 2] = [
        (
            crate::engine::worker::NOTIFY,
            &crate::engine::worker::NOTIFY_ARGS,
        ),
        (crate::engine::worker::AI, &crate::ai::step::ARGS),
    ];
    // #ai's own messages name it: the address says the step
    let ai = |m: String| m.strip_prefix("#ai: ").map(String::from).unwrap_or(m);
    let mut out = vec![];
    for (name, f) in &flows {
        for (at, step, args) in f.addressed() {
            let at = format!("workflow/{name}/{at}");
            match step {
                Name::Action { name: a, version } => {
                    let built_in =
                        crate::engine::worker::BUILT_IN.contains(&a.as_str()) && version.is_none();
                    if !built_in && let Err(e) = ws.action_file(a, version.as_deref()) {
                        errors.push(Problem::new(at.clone(), format!("{e:#}")));
                    }
                    for (action, taken) in built_ins {
                        if a != action || !built_in {
                            continue;
                        }
                        for arg in crate::flow::arg_names_of(args) {
                            // an arg refused is an error already: told once
                            let refused = action == crate::engine::worker::AI
                                && crate::ai::step::REFUSED.contains(&arg.as_str());
                            if !taken.contains(&arg.as_str()) && !refused {
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
        let id = j.workflow_id(name, &now)?;
        let m = &f.meta;
        let runs = j.runs_page(Some(name), None, &[], usize::MAX)?;
        let mut uses = vec![];
        for u in f.uses() {
            match u {
                Name::Action { name: a, version } => match ws.action_file(&a, version.as_deref()) {
                    Ok(file) => {
                        let first = crate::engine::worker::first_line(&file).unwrap_or_default();
                        let notes = crate::needs::notes(&file, &first, Some(&path));
                        uses.push(json!({ "action": a, "file": file.display().to_string(), "notes": notes }));
                    }
                    Err(_)
                        if crate::engine::worker::BUILT_IN.contains(&a.as_str())
                            && version.is_none() =>
                    {
                        let mut u = json!({ "action": a, "built_in": true });
                        if a == crate::engine::worker::AI {
                            let sends: Vec<String> = f
                                .args_of(&a)
                                .into_iter()
                                .map(|x| crate::ai::step::place(x, &machine))
                                .collect();
                            u["sends"] = json!(sends);
                        }
                        uses.push(u)
                    }
                    // told among the errors
                    Err(_) => {}
                },
                Name::Workflow(w) => uses.push(json!({ "workflow": w })),
            }
        }
        out.push(json!({
            "name": name,
            "id": id.id,
            "since": id.created,
            "file": files.get(name).map(|f| ws.shown(f)),
            "description": m.description,
            "from": m.from.map(crate::engine::daemon::stamp),
            "until": m.until.map(crate::engine::daemon::stamp),
            "off": m.off,
            "internal": m.internal,
            "other": Value::Object(m.other.clone().into_iter().collect::<Map<_, _>>()),
            "runs": runs.len(),
            "last_run": runs.first().map(|r| json!({
                "name": r.name,
                "status": crate::engine::steps::status(&r.runtime),
                "at": r.ended.as_deref().unwrap_or(&r.started),
            })),
            "uses": uses,
        }));
    }
    let connectors: Vec<Value> = ws
        .connector_files()?
        .into_iter()
        .map(|(kind, file)| json!({ "type": kind, "file": file.display().to_string() }))
        .collect();
    let told_all = |ps: &[Problem]| ps.iter().map(|p| told(&ws_name, p)).collect::<Vec<_>>();
    let v = json!({ "workflows": out, "connectors": connectors, "notes": notes,
                    "errors": told_all(&errors), "warnings": told_all(&warnings) });
    if !errors.is_empty() {
        if json {
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            for (kind, list) in [("error", &v["errors"]), ("warning", &v["warnings"])] {
                for p in list.as_array().into_iter().flatten() {
                    eprintln!("{kind}: {} ({})", text(&p["message"]), text(&p["at"]));
                }
            }
            eprintln!(
                "{}",
                crate::problem::count_line(errors.len(), warnings.len())
            );
        }
        std::process::exit(1);
    }
    show(json, &v, |v| {
        for n in v["notes"].as_array().into_iter().flatten() {
            eprintln!("note: {}", text(n));
        }
        for w in v["warnings"].as_array().into_iter().flatten() {
            eprintln!("warning: {} ({})", text(&w["message"]), text(&w["at"]));
        }
        let mut apart = false;
        for w in v["workflows"].as_array().into_iter().flatten() {
            if w["internal"] == true && !apart {
                apart = true;
                println!("\ninternal");
            }
            println!("{}", text(&w["name"]));
            println!("  id {}  since {}", text(&w["id"]), text(&w["since"]));
            println!("  file {}", w["file"].as_str().unwrap_or("built in"));
            if let Some(d) = w["description"].as_str() {
                println!("  description {d}");
            }
            for k in ["from", "until"] {
                if let Some(t) = w[k].as_str() {
                    println!("  {k} {t}");
                }
            }
            for k in ["off", "internal"] {
                if w[k] == true {
                    println!("  {k}");
                }
            }
            for (k, x) in w["other"].as_object().into_iter().flatten() {
                println!("  {k} {x}");
            }
            println!("  runs so far {}", w["runs"]);
            if let Some(l) = w["last_run"].as_object() {
                println!(
                    "  last run {} {}  {}",
                    text(&l["name"]),
                    text(&l["status"]),
                    text(&l["at"])
                );
            }
            for u in w["uses"].as_array().into_iter().flatten() {
                if let Some(wf) = u["workflow"].as_str() {
                    println!("  {wf}  workflow");
                } else if u["built_in"] == true {
                    println!("  #{}  built in", text(&u["action"]));
                    for p in u["sends"].as_array().into_iter().flatten() {
                        println!("    {}", text(p));
                    }
                } else {
                    println!("  #{}  {}", text(&u["action"]), text(&u["file"]));
                    for n in u["notes"].as_array().into_iter().flatten() {
                        println!("    {}", text(n));
                    }
                }
            }
        }
        for c in v["connectors"].as_array().into_iter().flatten() {
            println!("connector {}  {}", text(&c["type"]), text(&c["file"]));
        }
    })
}

/// The workspace: its identity, meta and limits, and the daemon running it.
pub fn info(ws: &Workspace, json: bool) -> Result<()> {
    let j = Journal::open(ws)?;
    let w = j.workspace()?;
    // the manifest's name, once given; until then, itero's
    let named = ws.meta.get("name").and_then(|n| n.as_str());
    let meta: Map<String, Value> = ws
        .meta
        .iter()
        .filter(|(k, _)| *k != "name")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let l = &ws.limits;
    let port = ws.daemon_port();
    let answering = super::daemon_for(ws, &Journal::open(ws)?)?;
    let daemon = match (answering, port, port.and_then(super::daemon_health)) {
        (Some(a), _, _) => {
            let mut d = a.health["daemon"].clone();
            d["running"] = json!(true);
            d["port"] = json!(a.port);
            d["api"] = json!(a.api);
            d["run_by"] = json!(if a.machine { "machine" } else { "workspace" });
            d
        }
        (None, Some(p), Some(h)) => json!({
            "running": false,
            "port": p,
            "why": format!(
                "port {p} answers, but for another workspace ({})",
                h["workspace"]["path"].as_str().unwrap_or("unknown")
            ),
        }),
        (None, Some(p), None) => {
            json!({ "running": false, "port": p, "why": format!("no answer on port {p}") })
        }
        (None, None, _) => json!({
            "running": false,
            "port": null,
            "why": "no port yet: the OS gives one at its start",
        }),
    };
    let v = json!({
        "workspace": {
            "id": w.id,
            "name": named.unwrap_or(&w.name),
            "created": w.created,
            "path": ws.root.display().to_string(),
            "itero": w.itero,
            "meta": meta,
        },
        "limits": {
            "workflows": l.workflows,
            "runs": l.runs,
            "steps": l.steps,
            "space": l.space,
            "keep": humantime::format_duration(l.keep).to_string(),
            "log": l.log,
            "logs": l.logs,
        },
        "daemon": daemon,
        "notifications": crate::engine::worker::notifications(),
    });
    show(json, &v, |v| {
        let w = &v["workspace"];
        println!("workspace");
        for k in ["id", "name", "created", "path", "itero"] {
            println!("  {k} {}", text(&w[k]));
        }
        if let Some(m) = w["meta"].as_object().filter(|m| !m.is_empty()) {
            println!("  meta");
            for (k, x) in m {
                println!("    {k} {x}");
            }
        }
        println!("  limits");
        for (k, x) in v["limits"].as_object().into_iter().flatten() {
            println!("    {k} {}", text(x));
        }
        let d = &v["daemon"];
        if d["running"] == true {
            println!("daemon");
            for k in ["id", "name", "started", "pid", "host", "itero"] {
                println!("  {k} {}", text(&d[k]));
            }
            if d["run_by"] == "machine" {
                println!("  run by the machine's daemon (itero workspace list)");
            }
            println!("  port {}", d["port"]);
            println!("  api {}", text(&d["api"]));
        } else {
            println!("daemon: not running ({})", text(&d["why"]));
        }
        println!("{}", notifications_line(&v["notifications"]));
    })
}

/// What `notifications()` says, as a line.
fn notifications_line(n: &Value) -> String {
    let mut line = format!("notifications: {}", text(&n["state"]));
    if let Some(why) = n["why"].as_str() {
        line.push_str(&format!(" ({why})"));
    }
    if let (Some(app), Some(version)) = (n["app"].as_str(), n["version"].as_u64()) {
        line.push_str(&format!(", Itero.app {version} in {app}"));
    }
    if let Some(p) = n["notifier"].as_str() {
        line.push_str(&format!(", through {p}"));
    }
    line
}

/// What the machine's `/health` says of each workspace, a line each.
fn workspace_lines(workspaces: &Value) {
    for (name, w) in workspaces.as_object().into_iter().flatten() {
        println!(
            "  {name}: {} passes, last {}{}",
            w["passes"],
            w["last_pass"].as_str().unwrap_or("none yet"),
            w["last_error"]
                .as_str()
                .map(|e| format!(", last error: {e}"))
                .unwrap_or_default()
        );
    }
}

/// `itero info`: this itero, and the machine's daemon as its `/health` says.
pub fn machine_info(json: bool) -> Result<()> {
    let this = env!("CARGO_PKG_VERSION");
    let port = crate::engine::http::machine_port();
    let health = port.and_then(super::daemon_health);
    let of = |k: &str| health.as_ref().map(|h| h[k].clone());
    let v = json!({
        "itero": this,
        "port": port,
        "daemon": of("daemon"),
        "workspaces": of("workspaces"),
        "ai": of("ai"),
        "notifications": crate::engine::worker::notifications(),
    });
    show(json, &v, |v| {
        println!("itero {this}");
        let d = &v["daemon"];
        if d.is_null() {
            match v["port"].as_u64() {
                Some(p) => println!("daemon: none answering on port {p}"),
                None => println!("daemon: none answering: no port yet"),
            }
            println!("itero service status says why");
        } else {
            println!(
                "daemon: pid {}, port {}, itero {}, started {}",
                d["pid"],
                v["port"],
                text(&d["itero"]),
                text(&d["started"])
            );
            if d["itero"] != this {
                println!("this itero is {this}: itero service restart runs it");
            }
            workspace_lines(&v["workspaces"]);
        }
        if let Some(state) = v["ai"]["state"].as_str() {
            println!("ai: {state} {}", text(&v["ai"]["model"]));
        }
        println!("{}", notifications_line(&v["notifications"]));
    })
}

/// Where a workflow, action or connector of this name is.
pub fn which(ws: &Workspace, name: &str, json: bool) -> Result<()> {
    let (action_only, name) = match name.strip_prefix('#') {
        Some(n) => (true, n.to_string()),
        None => (false, name.to_string()),
    };
    let mut found: Vec<(&str, String)> = vec![];
    if !action_only {
        match ws.flow_files()?.get(&name) {
            Some(f) => found.push(("workflow", ws.shown(f))),
            None if crate::workspace::BUILT_IN.iter().any(|(n, _)| *n == name) => {
                found.push(("workflow", "built in".into()))
            }
            None => {}
        }
    }
    // the action, and each of its versions
    let actions: Vec<String> = ws
        .action_files()?
        .into_iter()
        .filter(|(file, _)| *file == name || file.starts_with(&format!("{name}@")))
        .map(|(_, f)| ws.shown(&f))
        .collect();
    if actions.is_empty() && crate::engine::worker::BUILT_IN.contains(&name.as_str()) {
        found.push(("action", "built in".into()));
    }
    found.extend(actions.into_iter().map(|a| ("action", a)));
    if !action_only {
        for (kind, f) in ws.connector_files()? {
            if kind == name {
                found.push(("connector", ws.shown(&f)));
            }
        }
    }
    if found.is_empty() {
        bail!("nothing named {name}: itero ws wf list and itero ws check list them");
    }
    let v: Vec<Value> = found
        .into_iter()
        .map(|(kind, path)| json!({ "kind": kind, "path": path }))
        .collect();
    show(json, &Value::Array(v), |v| {
        for f in v.as_array().into_iter().flatten() {
            println!("{:<10}{}", text(&f["kind"]), text(&f["path"]));
        }
    })
}

/// `itero secret` and `itero ws secret`: kept in the store for every
/// workspace, or for `ws` alone.
pub fn secret(ws: Option<&Workspace>, name: &str, file: Option<PathBuf>) -> Result<()> {
    use crate::engine::secrets;
    secrets::ready(name)?;
    let value = match file {
        Some(f) => {
            let t = std::fs::read_to_string(&f)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", f.display()))?;
            let t = t.strip_suffix('\n').unwrap_or(&t);
            t.strip_suffix('\r').unwrap_or(t).to_string()
        }
        None => rpassword::prompt_password(format!("{name}: "))?,
    };
    if value.is_empty() {
        bail!("{name}: an empty secret is not kept");
    }
    match ws {
        None => {
            secrets::set(name, &value)?;
            println!("{name}: kept in the OS secret store");
        }
        Some(ws) => {
            secrets::set_for(&ws.id()?, name, &value)?;
            let me = Journal::open(ws)?.workspace()?;
            println!(
                "{name}: kept for workspace {} in the OS secret store",
                me.name
            );
        }
    }
    Ok(())
}

/// `itero service install`: the machine's service, `itero watchdog
/// --all`, installed and started.
pub fn service_install(port: Option<u16>) -> Result<()> {
    use crate::engine::service;
    let exe = std::env::current_exe()?;
    let state = crate::machine::state_dir()?;
    std::fs::create_dir_all(&state)?;
    let home = std::path::PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?,
    );
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|c| !c.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    remove_old_services(&service::old_services(&home, &config))?;
    let path = std::env::var("PATH").ok();
    service::install(service::machine_ctx(&exe, &home, &state, port, path))?;
    println!(
        "installed: {} runs every workspace on this machine's list (itero workspace list); it starts at login",
        service::MACHINE
    );
    match port {
        Some(p) => println!("API: http://127.0.0.1:{p}/"),
        None => println!("API: on the port it keeps, as itero service status says"),
    }
    println!("log: {}", state.join("daemon.log").display());
    println!("it keeps this shell's PATH; install again after changing it");
    Ok(())
}

/// The services from before, one per workspace, removed: their
/// workspaces run once added to the machine's list.
fn remove_old_services(old: &[crate::engine::service::Old]) -> Result<()> {
    use crate::engine::service;
    for o in old {
        let label = o
            .label
            .parse()
            .map_err(|e| anyhow::anyhow!("{}: {e}", o.label))?;
        service::uninstall(&label)?;
        println!(
            "removed {}, which ran {} alone: itero workspace add there puts it on the list",
            o.label,
            o.folder.display()
        );
    }
    Ok(())
}

/// The machine's service's state, and its daemon's health.
pub fn service_status(json: bool) -> Result<()> {
    let port = crate::engine::http::machine_port();
    let v = json!({
        "service": crate::engine::service::status(&crate::engine::service::machine_label())?,
        "port": port,
        "daemon": port.and_then(super::daemon_health),
    });
    show(json, &v, |v| {
        println!("service: {}", text(&v["service"]));
        let h = &v["daemon"];
        if h.is_null() {
            match v["port"].as_u64() {
                Some(p) => println!("daemon: no answer on port {p}"),
                None => println!("daemon: no port yet"),
            }
            return;
        }
        println!("daemon: pid {}, port {}", h["pid"], v["port"]);
        workspace_lines(&h["workspaces"]);
    })
}
