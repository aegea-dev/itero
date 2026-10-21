//! The workspace: itero.toml, and the workflows, actions, sources and args
//! in its folders.
use itero::workspace;
use std::path::Path;

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

#[test]
fn defaults_are_one_workflows_and_one_actions_folder_on_8787() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.port, 8787);
    assert_eq!(ws.workflows, [d.path().join("workflows")]);
    assert_eq!(ws.actions, [d.path().join("actions")]);
}

#[test]
fn a_workspace_without_its_folders_has_no_workflows_and_its_daemon_idles() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    let ws = workspace::find(d.path()).unwrap();
    assert!(ws.flows().unwrap().is_empty());
    assert!(ws.source_files().unwrap().is_empty());
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    let pass = itero::engine::daemon::run_once(&ws, chrono::Utc::now(), exe).unwrap();
    assert_eq!(pass, itero::engine::daemon::Pass::default());
}

#[test]
fn no_itero_toml_is_an_error_saying_how_to_make_one() {
    let d = tempfile::tempdir().unwrap();
    let e = workspace::find(d.path()).unwrap_err().to_string();
    assert!(e.contains("itero.toml"), "{e}");
}

#[test]
fn an_unknown_setting_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "kapı = 1\n");
    let e = format!("{:#}", workspace::find(d.path()).err().unwrap());
    assert!(e.contains("kapı"), "{e}");
}

fn executable(root: &Path, rel: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(root, rel, text);
    std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn simple(d: &Path) -> workspace::Workspace {
    write(d, "itero.toml", "");
    workspace::find(d).unwrap()
}

#[test]
fn workflows_are_named_after_their_files() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    write(
        d.path(),
        "workflows/posta-uyarısı.json",
        r##"{ "on": [{ "event": "mail.received" }], "steps": ["#notify"] }"##,
    );
    write(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "steps": ["#ship"] }"##,
    );
    let flows = ws.flows().unwrap();
    let names: Vec<&str> = flows.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["posta-uyarısı", "sipariş"]);
    write(d.path(), "workflows/bozuk.json", r##"{ "steps": [] }"##);
    let e = format!("{:#}", ws.flows().unwrap_err());
    assert!(
        e.contains("bozuk.json") && e.contains("steps is empty"),
        "{e}"
    );
}

#[test]
fn an_action_is_its_latest_version_unless_pinned() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        executable(d.path(), "actions/notify@0.2", "#!/bin/sh\n");
        executable(d.path(), "actions/notify@0.10", "#!/bin/sh\n");
        executable(d.path(), "actions/kargola", "#!/bin/sh\n");
        let file = |n: &str, v: Option<&str>| {
            ws.action_file(n, v)
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(file("notify", None).unwrap(), "notify@0.10");
        assert_eq!(file("notify", Some("0.2")).unwrap(), "notify@0.2");
        assert_eq!(file("kargola", None).unwrap(), "kargola");
        let e = format!("{:#}", file("notify", Some("0.3")).unwrap_err());
        assert!(e.contains("notify@0.3"), "{e}");
        let e = format!("{:#}", file("yok", None).unwrap_err());
        assert!(e.contains("no action yok"), "{e}");
        write(d.path(), "actions/çalışmaz", "#!/bin/sh\n");
        let e = format!("{:#}", file("çalışmaz", None).unwrap_err());
        assert!(e.contains("not executable"), "{e}");
    }
}

#[test]
fn a_source_sends_messages_of_its_file_name() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        executable(d.path(), "sources/mail.received", "#!/bin/sh\n");
        let s = ws.source_files().unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, "mail.received");
        executable(d.path(), "sources/posta", "#!/bin/sh\n");
        let e = format!("{:#}", ws.source_files().unwrap_err());
        assert!(e.contains("posta") && e.contains("message type"), "{e}");
    }
}

#[test]
fn args_are_an_object_or_a_list_of_objects() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    assert_eq!(ws.args("kargola").unwrap(), None);
    write(
        d.path(),
        "args/kargola.json",
        r##"{ "firma": "PTT Kargo" }"##,
    );
    assert_eq!(
        ws.args("kargola").unwrap(),
        Some(serde_json::json!({"firma": "PTT Kargo"}))
    );
    write(
        d.path(),
        "args/posta.json",
        r##"[ { "user": "ayşe@örnek.test" }, { "user": "can@örnek.test" } ]"##,
    );
    assert!(ws.args("posta").unwrap().unwrap().is_array());
    for (text, name) in [("[1, 2]", "bir"), (r#""çay""#, "iki"), ("{", "üç")] {
        write(d.path(), &format!("args/{name}.json"), text);
        let e = format!("{:#}", ws.args(name).unwrap_err());
        assert!(e.contains(&format!("{name}.json")), "{text}: {e}");
    }
}
