use std::path::Path;

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
}

#[test]
fn the_old_services_one_per_workspace_are_found_with_their_folders() {
    use itero::engine::service::{Old, old_services};
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("ev");
    let agents = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents).unwrap();
    let plist = |folder: &str| {
        format!(
            "<?xml version=\"1.0\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key>\n\t<string>x</string>\n\t<key>WorkingDirectory</key>\n\t<string>{folder}</string>\n</dict>\n</plist>\n"
        )
    };
    std::fs::write(
        agents.join("dev.aegea.itero.daemon-6abd1c2f821a.plist"),
        plist("/Users/biri/iş &amp; ev"),
    )
    .unwrap();
    std::fs::write(
        agents.join("dev.aegea.itero.daemon-77a42351f78b.plist"),
        plist("/Users/biri/yok"),
    )
    .unwrap();
    // the machine's own, and another app's: not old ones
    std::fs::write(
        agents.join("dev.aegea.itero.daemon.plist"),
        plist("/Users/biri"),
    )
    .unwrap();
    std::fs::write(agents.join("com.başka.uygulama.plist"), plist("/x")).unwrap();
    let units = d.path().join("ayar/systemd/user");
    std::fs::create_dir_all(&units).unwrap();
    std::fs::write(
        units.join("aegea-itero.daemon-0123456789ab.service"),
        "[Unit]\nDescription=itero\n\n[Service]\nWorkingDirectory=/home/biri/çalışma\nExecStart=/usr/bin/itero watchdog\n",
    )
    .unwrap();
    let mut got = old_services(&home, &d.path().join("ayar"));
    got.sort_by(|a, b| a.label.cmp(&b.label));
    assert_eq!(
        got,
        vec![
            Old {
                label: "dev.aegea.itero.daemon-0123456789ab".into(),
                folder: "/home/biri/çalışma".into(),
            },
            Old {
                label: "dev.aegea.itero.daemon-6abd1c2f821a".into(),
                folder: "/Users/biri/iş & ev".into(),
            },
            Old {
                label: "dev.aegea.itero.daemon-77a42351f78b".into(),
                folder: "/Users/biri/yok".into(),
            },
        ]
    );
}
