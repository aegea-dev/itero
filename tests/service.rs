use itero::engine::service::{install_ctx, label, log_file};
use std::path::Path;

#[test]
fn the_service_runs_the_watchdog_in_the_workspace() {
    let root = Path::new("/Users/biri/iş akışları");
    let exe = Path::new("/opt/homebrew/bin/itero");
    let ctx = install_ctx(root, exe, Some("/opt/homebrew/bin:/usr/bin".into()));
    assert_eq!(ctx.program, exe);
    let args: Vec<String> = ctx
        .args
        .iter()
        .map(|a| a.to_string_lossy().into())
        .collect();
    assert_eq!(
        args,
        [
            "watchdog",
            "--log",
            "/Users/biri/iş akışları/.itero/daemon.log"
        ]
    );
    assert_eq!(ctx.working_directory.as_deref(), Some(root));
    assert_eq!(
        ctx.environment,
        Some(vec![(
            "PATH".to_string(),
            "/opt/homebrew/bin:/usr/bin".to_string()
        )])
    );
    assert!(ctx.autostart);
    assert_eq!(log_file(root), root.join(".itero/daemon.log"));
}

#[test]
fn each_workspace_has_its_own_label() {
    let a = label(Path::new("/Users/biri/ev")).to_qualified_name();
    let b = label(Path::new("/Users/biri/iş")).to_qualified_name();
    assert_ne!(a, b);
    assert!(
        a.starts_with("dev.aegea.itero.daemon-") && a.len() == "dev.aegea.itero.daemon-".len() + 12,
        "{a}"
    );
}

#[test]
fn the_machines_service_runs_the_watchdog_over_every_workspace() {
    use itero::engine::service::{MACHINE, machine_ctx};
    let home = Path::new("/Users/biri");
    let state = Path::new("/Users/biri/.local/state/itero");
    let exe = Path::new("/opt/homebrew/bin/itero");
    let ctx = machine_ctx(exe, home, state, Some(7600), Some("/usr/bin".into()));
    assert_eq!(ctx.label.to_qualified_name(), MACHINE);
    assert_eq!(ctx.program, exe);
    let args: Vec<String> = ctx
        .args
        .iter()
        .map(|a| a.to_string_lossy().into())
        .collect();
    assert_eq!(
        args,
        [
            "watchdog",
            "--all",
            "--port",
            "7600",
            "--log",
            "/Users/biri/.local/state/itero/daemon.log"
        ]
    );
    assert_eq!(ctx.working_directory.as_deref(), Some(home));
    assert!(ctx.autostart);
    // no port: the daemon keeps the one it is given
    let ctx = machine_ctx(exe, home, state, None, None);
    let args: Vec<String> = ctx
        .args
        .iter()
        .map(|a| a.to_string_lossy().into())
        .collect();
    assert_eq!(&args[..2], ["watchdog", "--all"]);
    assert!(!args.contains(&"--port".to_string()));
    // the old services, one per workspace, have labels of their own
    assert_ne!(label(home).to_qualified_name(), MACHINE);
}
