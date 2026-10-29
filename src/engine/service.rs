//! The daemon as an OS service: a per-user launchd agent (macOS) or systemd
//! user unit (Linux) that runs `itero watchdog --all`, starts at login, and
//! is restarted by the OS if the watchdog itself ends. One service for the
//! machine, running every workspace on its list, through
//! `service-manager`. Before, each workspace had a service of its own:
//! `install` removes those.
use anyhow::{Result, anyhow};
use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx,
    ServiceStatus, ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The machine's service's label.
pub const MACHINE: &str = "dev.aegea.itero.daemon";

pub fn machine_label() -> ServiceLabel {
    MACHINE.parse().expect("a valid label")
}

/// The machine's service: `itero watchdog --all` from the home folder,
/// its log in itero's state folder, with `path` (the installing shell's
/// `PATH`: a service starts with a bare one, which would not find mail, uv
/// or bun).
pub fn machine_ctx(
    exe: &Path,
    home: &Path,
    state: &Path,
    port: Option<u16>,
    path: Option<String>,
) -> ServiceInstallCtx {
    let mut args: Vec<OsString> = vec!["watchdog".into(), "--all".into()];
    if let Some(p) = port {
        args.extend(["--port".into(), p.to_string().into()]);
    }
    args.extend(["--log".into(), state.join("daemon.log").into_os_string()]);
    ServiceInstallCtx {
        label: machine_label(),
        program: exe.to_path_buf(),
        args,
        contents: None,
        username: None,
        working_directory: Some(home.to_path_buf()),
        environment: path.map(|p| vec![("PATH".to_string(), p)]),
        autostart: true,
        restart_policy: RestartPolicy::Always {
            delay_secs: Some(5),
        },
    }
}

fn manager() -> Result<Box<dyn ServiceManager>> {
    let mut m =
        <dyn ServiceManager>::native().map_err(|e| anyhow!("no service manager here: {e}"))?;
    m.set_level(ServiceLevel::User)
        .map_err(|e| anyhow!("cannot use the per-user service manager: {e}"))?;
    if !m.available().unwrap_or(false) {
        return Err(anyhow!("the service manager is not available here"));
    }
    Ok(m)
}

/// Installs (or reinstalls) and starts the workspace's service.
pub fn install(ctx: ServiceInstallCtx) -> Result<()> {
    let m = manager()?;
    let label = ctx.label.clone();
    if !matches!(
        m.status(ServiceStatusCtx {
            label: label.clone()
        }),
        Ok(ServiceStatus::NotInstalled)
    ) {
        uninstall_with(&*m, &label)?;
    }
    m.install(ctx)
        .map_err(|e| anyhow!("cannot install the service: {e}"))?;
    m.start(ServiceStartCtx { label })
        .map_err(|e| anyhow!("installed, but cannot start the service: {e}"))
}

fn uninstall_with(m: &dyn ServiceManager, label: &ServiceLabel) -> Result<()> {
    // stopping a service that is not running is not a problem
    let _ = m.stop(ServiceStopCtx {
        label: label.clone(),
    });
    m.uninstall(ServiceUninstallCtx {
        label: label.clone(),
    })
    .map_err(|e| anyhow!("cannot uninstall the service: {e}"))
}

/// Stops and removes the service `label`; false when there was none.
pub fn uninstall(label: &ServiceLabel) -> Result<bool> {
    let m = manager()?;
    if let Ok(ServiceStatus::NotInstalled) = m.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        return Ok(false);
    }
    uninstall_with(&*m, label)?;
    Ok(true)
}

/// Starts the installed service `label`.
pub fn start(label: &ServiceLabel) -> Result<()> {
    manager()?
        .start(ServiceStartCtx {
            label: label.clone(),
        })
        .map_err(|e| anyhow!("cannot start the service: {e}"))
}

/// Stops the service `label`; it stays installed, and starts at login.
pub fn stop(label: &ServiceLabel) -> Result<()> {
    manager()?
        .stop(ServiceStopCtx {
            label: label.clone(),
        })
        .map_err(|e| anyhow!("cannot stop the service: {e}"))
}

/// The service `label`'s state, in words.
pub fn status(label: &ServiceLabel) -> Result<String> {
    let m = manager()?;
    Ok(
        match m
            .status(ServiceStatusCtx {
                label: label.clone(),
            })
            .map_err(|e| anyhow!("cannot ask the service manager: {e}"))?
        {
            ServiceStatus::NotInstalled => "not installed".into(),
            ServiceStatus::Running => "running".into(),
            ServiceStatus::Stopped(why) => match why {
                Some(w) => format!("stopped ({w})"),
                None => "stopped".into(),
            },
        },
    )
}

/// A service from before one per machine: one workspace's, by its label
/// (`dev.aegea.itero.daemon-<hash>`).
#[derive(Debug, Clone, PartialEq)]
pub struct Old {
    pub label: String,
    /// The workspace it ran in.
    pub folder: PathBuf,
}

/// The services, one per workspace, that itero installed before one per
/// machine: launchd agents in `home`'s `Library/LaunchAgents`, systemd
/// user units in `config`'s `systemd/user`, each with the folder it ran in.
pub fn old_services(home: &Path, config: &Path) -> Vec<Old> {
    let mut out = vec![];
    let prefix = format!("{MACHINE}-");
    for (dir, ext, unit) in [
        (home.join("Library/LaunchAgents"), "plist", false),
        (config.join("systemd/user"), "service", true),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != ext) {
                continue;
            }
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            // systemd names the unit after the label's organization and
            // application: aegea-itero.daemon-<hash>
            let hash = if unit {
                stem.strip_prefix("aegea-itero.daemon-")
            } else {
                stem.strip_prefix(prefix.as_str())
            };
            let Some(hash) = hash.filter(|h| !h.is_empty()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let folder = if unit {
                text.lines()
                    .find_map(|l| l.strip_prefix("WorkingDirectory="))
                    .map(str::to_string)
            } else {
                crate::plist::string(&text, "WorkingDirectory")
            };
            if let Some(folder) = folder {
                out.push(Old {
                    label: format!("{prefix}{hash}"),
                    folder: folder.trim().into(),
                });
            }
        }
    }
    out
}
