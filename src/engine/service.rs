//! The daemon as an OS service: a per-user launchd agent (macOS) or systemd
//! user unit (Linux) that runs `itero watchdog` in the workspace, starts at
//! login, and is restarted by the OS if the watchdog itself ends. One
//! service per workspace, through `service-manager`.
use anyhow::{Result, anyhow};
use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx,
    ServiceStatus, ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The service's label: one per workspace, from its path.
pub fn label(root: &Path) -> ServiceLabel {
    let hash: String = Sha256::digest(root.to_string_lossy().as_bytes())
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("dev.aegea.itero.daemon-{hash}")
        .parse()
        .expect("a valid label")
}

/// The workspace's log file, inside `.itero/` (local, never synced).
pub fn log_file(root: &Path) -> PathBuf {
    root.join(".itero").join("daemon.log")
}

/// What the service runs: `itero watchdog` in `root` (which reads its port
/// from itero.config.json), with `path` (the installing shell's `PATH`: a service
/// starts with a bare one, which would not find mail, uv or bun).
pub fn install_ctx(
    root: &Path,
    exe: &Path,
    interval: u64,
    path: Option<String>,
) -> ServiceInstallCtx {
    ServiceInstallCtx {
        label: label(root),
        program: exe.to_path_buf(),
        args: [
            "watchdog".into(),
            "--interval".into(),
            interval.to_string().into(),
            "--log".into(),
            log_file(root).into_os_string(),
        ]
        .into_iter()
        .collect::<Vec<OsString>>(),
        contents: None,
        username: None,
        working_directory: Some(root.to_path_buf()),
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

/// Stops and removes the workspace's service; false when there was none.
pub fn uninstall(root: &Path) -> Result<bool> {
    let m = manager()?;
    let label = label(root);
    if let Ok(ServiceStatus::NotInstalled) = m.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        return Ok(false);
    }
    uninstall_with(&*m, &label)?;
    Ok(true)
}

/// The service's state, in words.
pub fn status(root: &Path) -> Result<String> {
    let m = manager()?;
    Ok(
        match m
            .status(ServiceStatusCtx { label: label(root) })
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
