//! The workspace: itero.config.json, and the workflows, actions and sources
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
    write(d.path(), "itero.config.json", "{}");
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.port, 8787);
    assert_eq!(ws.workflows, [d.path().join("workflows")]);
    assert_eq!(ws.actions, [d.path().join("actions")]);
}

#[test]
fn a_workspace_without_its_folders_has_no_workflows_and_its_daemon_idles() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", "{}");
    let ws = workspace::find(d.path()).unwrap();
    let names: Vec<String> = ws.flows().unwrap().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["keep", "logs"], "only the built-in ones");
    assert!(ws.source_files().unwrap().is_empty());
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    let pass = itero::engine::daemon::run_once(&ws, chrono::Utc::now(), exe).unwrap();
    assert_eq!(pass, itero::engine::daemon::Pass::default());
}

#[test]
fn no_manifest_is_an_error_saying_how_to_make_one() {
    let d = tempfile::tempdir().unwrap();
    let e = workspace::find(d.path()).unwrap_err().to_string();
    assert!(e.contains("itero.config.json"), "{e}");
    // an itero.toml from before is named, with where it goes
    write(d.path(), "itero.toml", "port = 8787\n");
    let e = workspace::find(d.path()).unwrap_err().to_string();
    assert!(
        e.contains("itero.toml") && e.contains("itero.config.json"),
        "{e}"
    );
}

#[test]
fn the_manifest_says_how_it_runs_and_what_it_is() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "itero.config.json",
        r#"{
          "name": "ev-işleri",
          "version": "0.1.0",
          "description": "Posta ve siparişler",
          "kapı": 1,
          "port": 8799,
          "folders": { "workflows": ["iş akışları"], "actions": ["eylemler", "~/ortak"], "sources": ["kaynaklar"] }
        }"#,
    );
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.port, 8799);
    assert_eq!(ws.workflows, [d.path().join("iş akışları")]);
    assert_eq!(ws.actions[0], d.path().join("eylemler"));
    assert!(ws.actions[1].ends_with("ortak"));
    assert_eq!(ws.sources, [d.path().join("kaynaklar")]);
    // what it is: its own keys too, kept; how it runs is not among them
    assert_eq!(ws.meta["name"], "ev-işleri");
    assert_eq!(ws.meta["version"], "0.1.0");
    assert_eq!(ws.meta["kapı"], 1);
    assert!(!ws.meta.contains_key("port") && !ws.meta.contains_key("folders"));
}

fn executable(root: &Path, rel: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(root, rel, text);
    std::fs::set_permissions(root.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn simple(d: &Path) -> workspace::Workspace {
    write(d, "itero.config.json", "{}");
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
    assert_eq!(names, vec!["keep", "logs", "posta-uyarısı", "sipariş"]);
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

#[test]
fn meta_is_free_and_limits_have_defaults() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    assert!(ws.meta.is_empty());
    let l = &ws.limits;
    assert_eq!((l.workflows, l.runs, l.steps, l.logs), (100, 1000, 8, 3));
    assert_eq!((l.space, l.log), (1 << 30, 10 << 20));
    assert_eq!(l.keep, std::time::Duration::from_secs(86_400));
    assert_eq!(l.quiet, std::time::Duration::from_secs(600));
    assert_eq!(l.most, std::time::Duration::from_secs(86_400));
    write(
        d.path(),
        "itero.config.json",
        r#"{ "description": "Ev: posta ve siparişler", "sahibi": "ayşe", "etiketler": ["ev", "küçük"],
             "limits": { "runs": 5, "space": "500MB", "keep": "2h", "log": "64KB", "quiet": "30m", "most": "6h" } }"#,
    );
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.meta["sahibi"], "ayşe");
    assert_eq!(ws.meta["etiketler"], serde_json::json!(["ev", "küçük"]));
    assert_eq!(ws.limits.runs, 5);
    assert_eq!(ws.limits.workflows, 100);
    assert_eq!(ws.limits.space, 500 << 20);
    assert_eq!(ws.limits.log, 64 << 10);
    assert_eq!(ws.limits.keep, std::time::Duration::from_secs(7200));
    assert_eq!(ws.limits.quiet, std::time::Duration::from_secs(1800));
    assert_eq!(ws.limits.most, std::time::Duration::from_secs(6 * 3600));
}

#[test]
fn limit_mistakes_are_named() {
    for (manifest, says) in [
        (r#"{"limits": {"koşu": 5}}"#, "koşu"),
        (r#"{"limits": {"space": "çok"}}"#, "a size is like 10MB or 1GB"),
        (r#"{"limits": {"keep": "bir gün"}}"#, "a duration is like"),
        (r#"{"limits": {"steps": 0}}"#, "steps is at least 1"),
        (r#"{"limits": {"quiet": "sessiz"}}"#, "quiet: a duration is like"),
        (r#"{"limits": {"most": "0s"}}"#, "most is at least 1s"),
        (r#"{"id": "01a0"}"#, "id is given by itero"),
        (r#"{"port": "sekiz"}"#, "port"),
        (r#"{"folders": {"eylemler": ["x"]}}"#, "eylemler"),
        (r#"["ş"]"#, "an object"),
        ("{ bozuk", "itero.config.json"),
    ] {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "itero.config.json", manifest);
        let e = format!("{:#}", workspace::find(d.path()).err().unwrap());
        assert!(e.contains(says), "{manifest}: {e}");
    }
}

#[test]
fn more_workflows_than_the_limit_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", r#"{"limits": {"workflows": 2}}"#);
    let ws = workspace::find(d.path()).unwrap();
    for n in ["bir", "iki", "üç"] {
        write(
            d.path(),
            &format!("workflows/{n}.json"),
            r##"{ "steps": ["#gönder"] }"##,
        );
    }
    let e = format!("{:#}", ws.flows().unwrap_err());
    assert!(e.contains("3 workflows") && e.contains("limits"), "{e}");
}

#[test]
fn subfolders_group_and_names_stay_the_file_names() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        write(
            d.path(),
            "workflows/siparişler/sipariş.json",
            r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#gönder"] }"##,
        );
        write(
            d.path(),
            "workflows/posta/derin/uyarı.json",
            r##"{ "steps": ["#gönder"] }"##,
        );
        executable(d.path(), "actions/kargo/gönder", "#!/bin/sh\n");
        executable(d.path(), "actions/kargo/eski/gönder@0.1", "#!/bin/sh\n");
        executable(d.path(), "sources/posta/posta.geldi", "#!/bin/sh\n");
        // tools' folders are never looked into
        executable(d.path(), "actions/.venv/bin/python", "#!/bin/sh\n");
        executable(
            d.path(),
            "actions/web/node_modules/.bin/vite",
            "#!/bin/sh\n",
        );
        executable(d.path(), "sources/.cache/eski.kaynak", "#!/bin/sh\n");
        let names: Vec<String> = ws.flows().unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["keep", "logs", "sipariş", "uyarı"]);
        let file = |n: &str, v: Option<&str>| ws.action_file(n, v).unwrap();
        assert!(file("gönder", None).ends_with("actions/kargo/eski/gönder@0.1"));
        assert!(file("gönder", Some("0.1")).ends_with("gönder@0.1"));
        assert!(ws.action_file("python", None).is_err());
        assert!(ws.action_file("vite", None).is_err());
        let sources: Vec<String> = ws
            .source_files()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(sources, ["posta.geldi"]);
    }
}

#[test]
fn one_name_twice_in_a_kind_is_an_error_naming_both_files() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        write(
            d.path(),
            "workflows/a/sipariş.json",
            r##"{ "steps": ["#gönder"] }"##,
        );
        write(
            d.path(),
            "workflows/b/sipariş.json",
            r##"{ "steps": ["#gönder"] }"##,
        );
        let e = format!("{:#}", ws.flows().unwrap_err());
        assert!(
            e.contains("workflows/a/sipariş.json") && e.contains("workflows/b/sipariş.json"),
            "{e}"
        );
        executable(d.path(), "actions/a/gönder", "#!/bin/sh\n");
        executable(d.path(), "actions/b/gönder", "#!/bin/sh\n");
        let e = format!("{:#}", ws.action_file("gönder", None).unwrap_err());
        assert!(
            e.contains("actions/a/gönder") && e.contains("actions/b/gönder"),
            "{e}"
        );
        executable(d.path(), "sources/a/posta.geldi", "#!/bin/sh\n");
        executable(d.path(), "sources/b/posta.geldi", "#!/bin/sh\n");
        let e = format!("{:#}", ws.source_files().unwrap_err());
        assert!(
            e.contains("sources/a/posta.geldi") && e.contains("sources/b/posta.geldi"),
            "{e}"
        );
        // an action and a workflow may share a name
        std::fs::remove_file(d.path().join("workflows/b/sipariş.json")).unwrap();
        std::fs::remove_file(d.path().join("actions/b/gönder")).unwrap();
        executable(d.path(), "actions/sipariş", "#!/bin/sh\n");
        assert!(ws.flows().is_ok());
        assert!(ws.action_file("sipariş", None).is_ok());
    }
}

#[test]
fn files_that_are_not_executable_beside_actions_and_sources_are_left_alone() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        // two projects, each with its own files: none of them an action
        executable(d.path(), "actions/web/ekran", "#!/bin/sh\n");
        write(d.path(), "actions/web/package.json", "{}");
        write(d.path(), "actions/web/README.md", "# ekran görüntüsü\n");
        executable(d.path(), "actions/çeviri/çevir", "#!/bin/sh\n");
        write(d.path(), "actions/çeviri/package.json", "{}");
        write(d.path(), "actions/çeviri/README.md", "# çeviri\n");
        let names: Vec<String> = ws.action_files().unwrap().into_keys().collect();
        assert_eq!(names, ["ekran", "çevir"]);
        // a source's folder may hold notes too
        executable(d.path(), "sources/posta/posta.geldi", "#!/bin/sh\n");
        write(d.path(), "sources/posta/README.md", "# posta\n");
        let sources: Vec<String> = ws
            .source_files()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(sources, ["posta.geldi"]);
        // a step naming one that is not executable still says so
        let e = format!("{:#}", ws.action_file("package.json", None).unwrap_err());
        assert!(e.contains("not executable"), "{e}");
    }
}

/// The workflow `name` as the workspace reads it.
fn flow(ws: &workspace::Workspace, name: &str) -> itero::flow::Flow {
    ws.flows()
        .unwrap()
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap()
        .1
}

#[test]
fn a_workflows_config_gives_its_settings_and_switches() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    write(
        d.path(),
        "workflows/gözcü.json",
        r##"{ "meta": { "description": "Siteler" }, "on": [{ "event": "saat.geldi" }], "steps": ["#bak"] }"##,
    );
    // no config: no settings, its meta as written
    let f = flow(&ws, "gözcü");
    assert!(f.settings.is_empty());
    assert!(!f.meta.off);
    write(
        d.path(),
        "config/gözcü.json",
        r#"{ "siteler": ["https://örnek.test"], "en_az": 10, "off": true, "until": "2026-12-31" }"#,
    );
    let f = flow(&ws, "gözcü");
    assert_eq!(
        serde_json::Value::Object(f.settings),
        serde_json::json!({ "siteler": ["https://örnek.test"], "en_az": 10 }),
        "the switches are not values"
    );
    assert!(f.meta.off);
    assert_eq!(
        f.meta.until.unwrap().to_rfc3339(),
        "2027-01-01T00:00:00+00:00"
    );
    assert_eq!(f.meta.description.as_deref(), Some("Siteler"));
    // a config turns on a workflow its meta turns off
    write(
        d.path(),
        "workflows/kapalı.json",
        r##"{ "meta": { "off": true }, "steps": ["#bak"] }"##,
    );
    write(d.path(), "config/kapalı.json", r#"{ "off": false }"#);
    assert!(!flow(&ws, "kapalı").meta.off);
}

#[test]
fn config_mistakes_are_named() {
    for (text, says) in [
        ("[\"ş\"]", "an object"),
        ("{ \"off\": \"evet\" }", "off is true or false"),
        ("{ \"until\": \"yarın\" }", "until"),
        ("{ bozuk", "config/gözcü.json"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        write(
            d.path(),
            "workflows/gözcü.json",
            r##"{ "steps": ["#bak"] }"##,
        );
        write(d.path(), "config/gözcü.json", text);
        let e = format!("{:#}", ws.flows().unwrap_err());
        assert!(e.contains(says) && e.contains("gözcü"), "{text}: {e}");
    }
}
