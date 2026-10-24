//! The workspace: itero.config.json, and the workflows, actions and connectors
//! in its folders.
use itero::workspace;
use std::path::Path;

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

#[test]
fn defaults_are_one_workflows_and_one_actions_folder_and_no_port() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", "{}");
    let ws = workspace::find(d.path()).unwrap();
    // the OS gives one when the daemon starts, kept in .itero/port
    assert_eq!(ws.port, None);
    assert_eq!(ws.daemon_port(), None);
    std::fs::create_dir_all(d.path().join(".itero")).unwrap();
    std::fs::write(d.path().join(".itero/port"), "50123\n").unwrap();
    assert_eq!(ws.daemon_port(), Some(50123));
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
    assert!(ws.connector_files().unwrap().is_empty());
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
          "folders": { "workflows": ["iş akışları"], "actions": ["eylemler", "~/ortak"], "connectors": ["kaynaklar"] }
        }"#,
    );
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.port, Some(8799));
    assert_eq!(ws.daemon_port(), Some(8799));
    assert_eq!(ws.workflows, [d.path().join("iş akışları")]);
    assert_eq!(ws.actions[0], d.path().join("eylemler"));
    assert!(ws.actions[1].ends_with("ortak"));
    assert_eq!(ws.connectors, [d.path().join("kaynaklar")]);
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
fn a_connector_sends_messages_of_its_file_name() {
    #[cfg(unix)]
    {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        executable(d.path(), "connectors/mail.received", "#!/bin/sh\n");
        let s = ws.connector_files().unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, "mail.received");
        executable(d.path(), "connectors/posta", "#!/bin/sh\n");
        let e = format!("{:#}", ws.connector_files().unwrap_err());
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
        "itero.config.json",
        r##"{ "action": {
              "kargola": { "description": "Kargoya verir", "args": { "firma": "PTT Kargo" } },
              "posta": { "args": [ { "user": "ayşe@örnek.test" }, { "user": "can@örnek.test" } ] },
              "sade": { "description": "Args yok" } } }"##,
    );
    assert_eq!(
        ws.args("kargola").unwrap(),
        Some(serde_json::json!({"firma": "PTT Kargo"}))
    );
    assert!(ws.args("posta").unwrap().unwrap().is_array());
    assert_eq!(ws.args("sade").unwrap(), None);
    for (args, name) in [("[1, 2]", "bir"), (r#""çay""#, "iki")] {
        write(
            d.path(),
            "itero.config.json",
            &format!(r#"{{"action": {{"{name}": {{"args": {args}}}}}}}"#),
        );
        let e = format!("{:#}", ws.args(name).unwrap_err());
        assert!(
            e.contains(&format!("action.{name}.args")) && e.contains("itero.config.json"),
            "{args}: {e}"
        );
    }
}

#[test]
fn a_connectors_args_are_an_object() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    assert_eq!(ws.connector_args("posta.geldi").unwrap(), None);
    write(
        d.path(),
        "itero.config.json",
        r#"{ "connector": {
              "posta.geldi": { "args": { "user": "ayşe@örnek.test", "password": "$$POSTA_ŞİFRE" } },
              "sade.olay": { "description": "Args yok" } } }"#,
    );
    let a = ws.connector_args("posta.geldi").unwrap().unwrap();
    assert_eq!(a["user"], "ayşe@örnek.test");
    assert_eq!(ws.connector_args("sade.olay").unwrap(), None);
    for args in [r#"[{"user": "can@örnek.test"}]"#, r#""çay""#] {
        write(
            d.path(),
            "itero.config.json",
            &format!(r#"{{"connector": {{"posta.geldi": {{"args": {args}}}}}}}"#),
        );
        let e = format!("{:#}", ws.connector_args("posta.geldi").unwrap_err());
        assert!(
            e.contains("connector.posta.geldi.args are an object")
                && e.contains("itero.config.json"),
            "{args}: {e}"
        );
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
        (
            r#"{"limits": {"space": "çok"}}"#,
            "a size is like 10MB or 1GB",
        ),
        (r#"{"limits": {"keep": "bir gün"}}"#, "a duration is like"),
        (r#"{"limits": {"steps": 0}}"#, "steps is at least 1"),
        (
            r#"{"limits": {"quiet": "sessiz"}}"#,
            "quiet: a duration is like",
        ),
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
    write(
        d.path(),
        "itero.config.json",
        r#"{"limits": {"workflows": 2}}"#,
    );
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
        executable(d.path(), "connectors/posta/posta.geldi", "#!/bin/sh\n");
        // tools' folders are never looked into
        executable(d.path(), "actions/.venv/bin/python", "#!/bin/sh\n");
        executable(
            d.path(),
            "actions/web/node_modules/.bin/vite",
            "#!/bin/sh\n",
        );
        executable(d.path(), "connectors/.cache/eski.kaynak", "#!/bin/sh\n");
        let names: Vec<String> = ws.flows().unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["keep", "logs", "sipariş", "uyarı"]);
        let file = |n: &str, v: Option<&str>| ws.action_file(n, v).unwrap();
        assert!(file("gönder", None).ends_with("actions/kargo/eski/gönder@0.1"));
        assert!(file("gönder", Some("0.1")).ends_with("gönder@0.1"));
        assert!(ws.action_file("python", None).is_err());
        assert!(ws.action_file("vite", None).is_err());
        let connectors: Vec<String> = ws
            .connector_files()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(connectors, ["posta.geldi"]);
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
        executable(d.path(), "connectors/a/posta.geldi", "#!/bin/sh\n");
        executable(d.path(), "connectors/b/posta.geldi", "#!/bin/sh\n");
        let e = format!("{:#}", ws.connector_files().unwrap_err());
        assert!(
            e.contains("connectors/a/posta.geldi") && e.contains("connectors/b/posta.geldi"),
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
fn files_that_are_not_executable_beside_actions_and_connectors_are_left_alone() {
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
        // a connector's folder may hold notes too
        executable(d.path(), "connectors/posta/posta.geldi", "#!/bin/sh\n");
        write(d.path(), "connectors/posta/README.md", "# posta\n");
        let connectors: Vec<String> = ws
            .connector_files()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(connectors, ["posta.geldi"]);
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
fn a_workflows_entry_gives_what_it_is_and_its_settings() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    write(
        d.path(),
        "workflows/gözcü.json",
        r##"{ "on": [{ "event": "saat.geldi" }], "steps": ["#bak"] }"##,
    );
    // no entry: nothing said, no settings
    let f = flow(&ws, "gözcü");
    assert!(f.settings.is_empty() && f.meta.description.is_none());
    write(
        d.path(),
        "itero.config.json",
        r#"{ "workflow": { "gözcü": { "description": "Siteler", "off": true, "until": "2026-12-31",
             "settings": { "siteler": ["https://örnek.test"], "en_az": 10 } } } }"#,
    );
    // read as it is now: the manifest is read again
    let f = flow(&ws, "gözcü");
    assert_eq!(
        serde_json::Value::Object(f.settings),
        serde_json::json!({ "siteler": ["https://örnek.test"], "en_az": 10 })
    );
    assert!(f.meta.off);
    assert_eq!(f.meta.description.as_deref(), Some("Siteler"));
    assert_eq!(
        f.meta.until.unwrap().to_rfc3339(),
        "2027-01-01T00:00:00+00:00"
    );
}

#[test]
fn a_workflow_files_meta_is_refused_and_built_ins_are_internal() {
    let d = tempfile::tempdir().unwrap();
    let ws = simple(d.path());
    let keep = flow(&ws, "keep");
    assert!(keep.meta.internal && keep.meta.description.is_some());
    write(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"keep": {"off": true}}}"#,
    );
    let keep = flow(&ws, "keep");
    assert!(
        keep.meta.internal && keep.meta.off,
        "an entry turns a built-in off"
    );
    write(
        d.path(),
        "workflows/eski.json",
        r##"{ "meta": { "description": "Eski" }, "steps": ["#bak"] }"##,
    );
    let e = format!("{:#}", ws.flows().unwrap_err());
    assert!(
        e.contains("eski.json") && e.contains("itero.config.json"),
        "{e}"
    );
}

#[test]
fn entry_mistakes_name_the_manifest_and_the_workflow() {
    for (manifest, says) in [
        (
            r#"{"workflow": {"gözcü": {"off": "evet"}}}"#,
            "off is true or false",
        ),
        (
            r#"{"workflow": {"gözcü": {"settings": 3}}}"#,
            "settings are an object",
        ),
        (r#"{"workflow": ["gözcü"]}"#, "workflow is an object"),
    ] {
        let d = tempfile::tempdir().unwrap();
        let ws = simple(d.path());
        write(
            d.path(),
            "workflows/gözcü.json",
            r##"{ "steps": ["#bak"] }"##,
        );
        write(d.path(), "itero.config.json", manifest);
        let e = format!("{:#}", ws.flows().unwrap_err());
        assert!(
            e.contains(says) && e.contains("itero.config.json"),
            "{manifest}: {e}"
        );
    }
}

#[test]
fn the_daemon_watches_the_manifest_and_each_workflow_folder_and_file() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.config.json", "{}");
    write(d.path(), "workflows/sipariş.json", "{}");
    write(d.path(), "workflows/kargo/teslim.json", "{}");
    write(d.path(), "workflows/notlar.txt", "bir not");
    write(d.path(), "workflows/.git/HEAD.json", "{}");
    let ws = workspace::find(d.path()).unwrap();
    let r = |p: &str| ws.root.join(p);
    assert_eq!(
        ws.own_files().unwrap(),
        vec![
            r("itero.config.json"),
            r("workflows"),
            r("workflows/kargo"),
            r("workflows/kargo/teslim.json"),
            r("workflows/sipariş.json"),
        ]
    );
}
