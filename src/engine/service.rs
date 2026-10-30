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

/// Installs (or reinstalls) and starts the machine's service.
pub fn install(ctx: ServiceInstallCtx) -> Result<()> {
    install_on(&*manager()?, ctx)
}

fn install_on(m: &dyn ServiceManager, ctx: ServiceInstallCtx) -> Result<()> {
    let label = ctx.label.clone();
    if !matches!(
        m.status(ServiceStatusCtx {
            label: label.clone()
        }),
        Ok(ServiceStatus::NotInstalled)
    ) {
        uninstall_with(m, &label)?;
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
    uninstall_on(&*manager()?, label)
}

fn uninstall_on(m: &dyn ServiceManager, label: &ServiceLabel) -> Result<bool> {
    if let Ok(ServiceStatus::NotInstalled) = m.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        return Ok(false);
    }
    uninstall_with(m, label)?;
    Ok(true)
}

/// Starts the installed service `label`.
pub fn start(label: &ServiceLabel) -> Result<()> {
    start_on(&*manager()?, label)
}

fn start_on(m: &dyn ServiceManager, label: &ServiceLabel) -> Result<()> {
    m.start(ServiceStartCtx {
        label: label.clone(),
    })
    .map_err(|e| anyhow!("cannot start the service: {e}"))
}

/// Stops the service `label`; it stays installed, and starts at login.
pub fn stop(label: &ServiceLabel) -> Result<()> {
    stop_on(&*manager()?, label)
}

fn stop_on(m: &dyn ServiceManager, label: &ServiceLabel) -> Result<()> {
    m.stop(ServiceStopCtx {
        label: label.clone(),
    })
    .map_err(|e| anyhow!("cannot stop the service: {e}"))
}

/// The service `label`'s state, in words.
pub fn status(label: &ServiceLabel) -> Result<String> {
    status_on(&*manager()?, label)
}

fn status_on(m: &dyn ServiceManager, label: &ServiceLabel) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;

    /// A service manager that keeps one service's state, says what it was
    /// asked, and fails the call named in `fails`.
    struct Fake {
        state: RefCell<ServiceStatus>,
        calls: RefCell<Vec<String>>,
        fails: Option<&'static str>,
    }

    impl Fake {
        fn new(state: ServiceStatus, fails: Option<&'static str>) -> Fake {
            Fake {
                state: RefCell::new(state),
                calls: RefCell::new(vec![]),
                fails,
            }
        }
        fn call(&self, what: &'static str) -> io::Result<()> {
            self.calls.borrow_mut().push(what.into());
            if self.fails == Some(what) {
                return Err(io::Error::other("izin yok"));
            }
            Ok(())
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl ServiceManager for Fake {
        fn available(&self) -> io::Result<bool> {
            Ok(true)
        }
        fn install(&self, _: ServiceInstallCtx) -> io::Result<()> {
            self.call("install")?;
            *self.state.borrow_mut() = ServiceStatus::Stopped(None);
            Ok(())
        }
        fn uninstall(&self, _: ServiceUninstallCtx) -> io::Result<()> {
            self.call("uninstall")?;
            *self.state.borrow_mut() = ServiceStatus::NotInstalled;
            Ok(())
        }
        fn start(&self, _: ServiceStartCtx) -> io::Result<()> {
            self.call("start")?;
            *self.state.borrow_mut() = ServiceStatus::Running;
            Ok(())
        }
        fn stop(&self, _: ServiceStopCtx) -> io::Result<()> {
            self.call("stop")?;
            *self.state.borrow_mut() = ServiceStatus::Stopped(Some("durduruldu".into()));
            Ok(())
        }
        fn level(&self) -> ServiceLevel {
            ServiceLevel::User
        }
        fn set_level(&mut self, _: ServiceLevel) -> io::Result<()> {
            Ok(())
        }
        fn status(&self, _: ServiceStatusCtx) -> io::Result<ServiceStatus> {
            self.call("status")?;
            Ok(self.state.borrow().clone())
        }
    }

    fn ctx() -> ServiceInstallCtx {
        machine_ctx(
            Path::new("/usr/local/bin/itero"),
            Path::new("/Users/çalışan"),
            Path::new("/Users/çalışan/.local/state/itero"),
            None,
            None,
        )
    }

    #[test]
    fn install_replaces_one_there_and_starts_it() {
        let fresh = Fake::new(ServiceStatus::NotInstalled, None);
        install_on(&fresh, ctx()).unwrap();
        assert_eq!(fresh.calls(), ["status", "install", "start"]);
        assert_eq!(status_on(&fresh, &machine_label()).unwrap(), "running");

        // one there is stopped and removed first
        let there = Fake::new(ServiceStatus::Running, None);
        install_on(&there, ctx()).unwrap();
        assert_eq!(
            there.calls(),
            ["status", "stop", "uninstall", "install", "start"]
        );
    }

    #[test]
    fn install_says_which_step_failed() {
        let e = install_on(
            &Fake::new(ServiceStatus::NotInstalled, Some("install")),
            ctx(),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(e, "cannot install the service: izin yok");
        let e = install_on(
            &Fake::new(ServiceStatus::NotInstalled, Some("start")),
            ctx(),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(e, "installed, but cannot start the service: izin yok");
        let e = install_on(&Fake::new(ServiceStatus::Running, Some("uninstall")), ctx())
            .unwrap_err()
            .to_string();
        assert_eq!(e, "cannot uninstall the service: izin yok");
        // a status that cannot be read: removed first, to be sure
        let unknown = Fake::new(ServiceStatus::Running, Some("status"));
        install_on(&unknown, ctx()).unwrap();
        assert_eq!(
            unknown.calls(),
            ["status", "stop", "uninstall", "install", "start"]
        );
    }

    #[test]
    fn uninstall_says_whether_there_was_one() {
        let label = machine_label();
        let none = Fake::new(ServiceStatus::NotInstalled, None);
        assert!(!uninstall_on(&none, &label).unwrap());
        assert_eq!(none.calls(), ["status"]);
        // stopping one that is not running is no problem
        let stopped = Fake::new(ServiceStatus::Stopped(None), Some("stop"));
        assert!(uninstall_on(&stopped, &label).unwrap());
        assert_eq!(stopped.calls(), ["status", "stop", "uninstall"]);
        assert_eq!(status_on(&stopped, &label).unwrap(), "not installed");
    }

    #[test]
    fn start_stop_and_status_in_words() {
        let label = machine_label();
        let m = Fake::new(ServiceStatus::Stopped(None), None);
        assert_eq!(status_on(&m, &label).unwrap(), "stopped");
        start_on(&m, &label).unwrap();
        assert_eq!(status_on(&m, &label).unwrap(), "running");
        stop_on(&m, &label).unwrap();
        assert_eq!(status_on(&m, &label).unwrap(), "stopped (durduruldu)");

        let broken = Fake::new(ServiceStatus::Running, Some("start"));
        assert_eq!(
            start_on(&broken, &label).unwrap_err().to_string(),
            "cannot start the service: izin yok"
        );
        let broken = Fake::new(ServiceStatus::Running, Some("stop"));
        assert_eq!(
            stop_on(&broken, &label).unwrap_err().to_string(),
            "cannot stop the service: izin yok"
        );
        let broken = Fake::new(ServiceStatus::Running, Some("status"));
        assert_eq!(
            status_on(&broken, &label).unwrap_err().to_string(),
            "cannot ask the service manager: izin yok"
        );
    }
}
