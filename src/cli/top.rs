//! The machine and the workspace as a whole: `info`, `service status`, and
//! `ws info`, `ws check`, `ws which`.
use super::out::{show, text};
use crate::answer::Client;
use crate::answer::workspace::{RunBy, Runner};
use crate::engine::journal::Journal;
use crate::workspace::Workspace;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::PathBuf;

/// Reads every workflow, action and connector and says what is wrong:
/// every problem, each with its address, then how many; what it found is
/// printed when nothing is wrong.
pub fn check(c: &Client, json: bool) -> Result<()> {
    let report = c.check()?;
    let (errors, warnings) = (report.errors.len(), report.warnings.len());
    let v = serde_json::to_value(report)?;
    if errors > 0 {
        if json {
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            for (kind, list) in [("error", &v["errors"]), ("warning", &v["warnings"])] {
                for p in list.as_array().into_iter().flatten() {
                    eprintln!("{kind}: {} ({})", text(&p["message"]), text(&p["at"]));
                }
            }
            eprintln!("{}", crate::problem::count_line(errors, warnings));
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

/// The daemon running `ws`, as this machine's ports say: the machine's
/// daemon, else the workspace's own, else why none answers for it.
fn probe(ws: &Workspace) -> Result<Runner> {
    let port = ws.daemon_port();
    let answering = super::daemon_for(ws, &Journal::open(ws)?)?;
    Ok(
        match (answering, port, port.and_then(super::daemon_health)) {
            (Some(a), _, _) => Runner::Running {
                daemon: serde_json::from_value(a.health["daemon"].clone()).map_err(|e| {
                    anyhow!(
                        "the daemon at {} says who it is in a way this itero cannot read: {e}",
                        a.api
                    )
                })?,
                running: true,
                port: a.port,
                api: a.api,
                run_by: if a.machine {
                    RunBy::Machine
                } else {
                    RunBy::Workspace
                },
            },
            (None, Some(p), Some(h)) => Runner::Stopped {
                running: false,
                port: Some(p),
                why: format!(
                    "port {p} answers, but for another workspace ({})",
                    h["workspace"]["path"].as_str().unwrap_or("unknown")
                ),
            },
            (None, Some(p), None) => Runner::Stopped {
                running: false,
                port: Some(p),
                why: format!("no answer on port {p}"),
            },
            (None, None, _) => Runner::Stopped {
                running: false,
                port: None,
                why: "no port yet: the OS gives one at its start".into(),
            },
        },
    )
}

/// The workspace: its identity, meta and limits, and the daemon running it.
pub fn info(c: &Client, json: bool) -> Result<()> {
    let v = serde_json::to_value(c.info(|ws| Ok(probe(ws)?))?)?;
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
/// With a context, the server's daemon as its `/health` says, under its
/// address: notifications are this machine's, so not its.
pub fn machine_info(json: bool, context: Option<crate::context::Context>) -> Result<()> {
    let Some(c) = context else {
        return info_here(json);
    };
    let this = env!("CARGO_PKG_VERSION");
    let h: Value = crate::answer::remote::server(&c.name, &c.url, c.token.as_deref(), &["health"])?;
    let v = json!({
        "itero": this,
        "url": c.url,
        "daemon": h["daemon"],
        "workspaces": h["workspaces"],
        "ai": h["ai"],
    });
    show(json, &v, |v| {
        println!("itero {this}");
        println!("{}: {}", c.name, c.url);
        let d = &v["daemon"];
        println!(
            "daemon: pid {}, itero {}, started {}",
            d["pid"],
            text(&d["itero"]),
            text(&d["started"])
        );
        workspace_lines(&v["workspaces"]);
        if let Some(state) = v["ai"]["state"].as_str() {
            println!("ai: {state} {}", text(&v["ai"]["model"]));
        }
    })
}

/// `itero info` on this machine.
fn info_here(json: bool) -> Result<()> {
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
pub fn which(c: &Client, name: &str, json: bool) -> Result<()> {
    let v = serde_json::to_value(c.which(name)?)?;
    show(json, &v, |v| {
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
pub fn service_install() -> Result<()> {
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
    service::install(service::machine_ctx(&exe, &home, &state, path))?;
    println!(
        "installed: {} runs every workspace on this machine's list (itero workspace list); it starts at login",
        service::MACHINE
    );
    println!(
        "API: on the port {} gives, else the one it keeps, as itero service status says",
        crate::server::Server::default_path()?.display()
    );
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
