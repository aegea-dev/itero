//! The workspace as a whole: `check`, `info`, `which`, `service status`.
use super::out::{show, text};
use crate::engine::journal::Journal;
use crate::flow::{Flow, Name};
use crate::workspace::Workspace;
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

/// What `itero check` refuses in the manifest: an entry naming no workflow,
/// action or connector, and the folders from before it (`args/`, `config/`).
fn check_manifest(ws: &Workspace, flows: &[(String, Flow)]) -> Result<()> {
    let manifest = crate::workspace::MANIFEST;
    let mut wrong = vec![];
    for (dir, now) in [
        ("args", "\"action\": {\"<name>\": {\"args\": ...}}"),
        ("config", "\"workflow\": {\"<name>\": {\"settings\": ...}}"),
    ] {
        if ws.root.join(dir).is_dir() {
            wrong.push(format!("{dir}/: this goes in {manifest} now, as {now}"));
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
            wrong.push(why);
        }
    }
    // with no state, filling an action's args refuses only a secret inside text
    for (name, e) in ws.entries("action")? {
        if let Some(args) = e.get("args")
            && let Err(why) = crate::flow::fill(args, &serde_json::Map::new())
        {
            wrong.push(format!("action.{name}.args: {why}"));
        }
        // its error names the manifest, which the list below names once
        if let Err(why) = ws.step_limits(&name) {
            let why = format!("{why:#}");
            let path = format!("{}: ", ws.root.join(manifest).display());
            wrong.push(why.strip_prefix(&path).unwrap_or(&why).to_string());
        }
    }
    for (kind, known) in [
        (
            "workflow",
            flows.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        ),
        ("action", actions),
        ("connector", connectors),
    ] {
        for name in ws.entries(kind)?.keys() {
            if !known.contains(name) {
                wrong.push(format!("{kind}.{name}: no {kind} {name}"));
            }
        }
    }
    if !wrong.is_empty() {
        bail!("{}: {}", ws.root.join(manifest).display(), wrong.join("; "));
    }
    Ok(())
}

/// Reads every workflow, action and connector and says what is wrong: an
/// error stops it, naming the file; what it found is printed.
pub fn check(ws: &Workspace, json: bool) -> Result<()> {
    let mut notes = vec![];
    for dir in ws.workflows.iter().chain(&ws.actions) {
        if !dir.exists() {
            notes.push(format!("no folder {}", dir.display()));
        }
    }
    let mut flows = ws.flows()?;
    check_manifest(ws, &flows)?;
    // the internal ones last, apart
    flows.sort_by_key(|(_, f)| f.meta.internal);
    let j = Journal::open(ws)?;
    let now = crate::engine::daemon::stamp(chrono::Utc::now());
    let files = ws.flow_files()?;
    // the PATH an action runs with
    let path = crate::connectors::supervise::path_with(&std::env::current_exe()?);
    let mut out = vec![];
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
                        let notes = crate::needs::notes(&file, &first, Some(&path));
                        uses.push(json!({ "action": a, "file": file.display().to_string(), "notes": notes }));
                    }
                    Err(_)
                        if crate::engine::worker::BUILT_IN.contains(&a.as_str())
                            && version.is_none() =>
                    {
                        uses.push(json!({ "action": a, "built_in": true }))
                    }
                    Err(e) => bail!("{name}: {e:#}"),
                },
                Name::Workflow(w) => {
                    if !flows.iter().any(|(n, _)| *n == w) {
                        bail!("{name} uses workflow {w}, which is not there");
                    }
                    uses.push(json!({ "workflow": w }));
                }
            }
        }
        out.push(json!({
            "name": name,
            "id": id.id,
            "since": id.created,
            "file": files.get(name).map(|f| ws.shown(f)),
            "settings": Value::Object(f.settings.clone()),
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
    let mut warnings: Vec<String> = crate::engine::secrets::shadowed(ws)?
        .iter()
        .map(|n| crate::engine::secrets::shadow_warning(n))
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
    for (name, f) in &flows {
        for (action, taken) in built_ins {
            for a in f.arg_names(action) {
                if !taken.contains(&a.as_str()) {
                    let (last, rest) = taken.split_last().expect("a built-in takes args");
                    warnings.push(format!(
                        "{name}: #{action} takes {} and {last}, not {a}",
                        rest.join(", ")
                    ));
                }
            }
        }
        for args in f.args_of(crate::engine::worker::AI) {
            match crate::ai::step::check(args) {
                Ok(Some(model)) => {
                    if let Some(w) = crate::ai::step::model_warning(&model) {
                        warnings.push(format!("{name}: {w}"));
                    }
                }
                Ok(None) => {}
                Err(e) => bail!("{name}: {e}"),
            }
        }
    }
    let v =
        json!({ "workflows": out, "connectors": connectors, "notes": notes, "warnings": warnings });
    show(json, &v, |v| {
        for n in v["notes"].as_array().into_iter().flatten() {
            eprintln!("note: {}", text(n));
        }
        for w in v["warnings"].as_array().into_iter().flatten() {
            eprintln!("warning: {}", text(w));
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
            if w["settings"].as_object().is_some_and(|s| !s.is_empty()) {
                println!("  settings {}", w["settings"]);
            }
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
        bail!("nothing named {name}: itero workflow list and itero check list them");
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

/// The service's state, and the daemon's health.
pub fn service_status(ws: &Workspace, json: bool) -> Result<()> {
    let port = ws.daemon_port();
    let v = json!({
        "service": crate::engine::service::status(&ws.root)?.to_string(),
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
        println!(
            "daemon: pid {}, {} passes, last {}{}",
            h["pid"],
            h["passes"],
            h["last_pass"].as_str().unwrap_or("none yet"),
            h["last_error"]
                .as_str()
                .map(|e| format!(", last error: {e}"))
                .unwrap_or_default()
        );
    })
}
