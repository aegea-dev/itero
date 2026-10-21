use itero::workspace;
use std::path::Path;

/// An action that runs in the manager and needs nothing from outside.
const SET: &str = r##"{ "action": "memory.set@1", "with": { "son": "ölçüm" } }"##;

fn flow(name: &str, action: &str) -> String {
    format!(
        r##"{{ "name": "{name}", "description": "ölçüm", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ {{ "from": "basla", "to": "bitti", "do": [ {action} ] }} ] }}"##
    )
}

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

#[test]
fn a_workspace_lists_its_folders_and_finds_workflows_in_each() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "itero.toml",
        "port = 8790\nworkflows = [\"workflows\", \"başka/workflows\"]\nactions = [\"actions\"]\n",
    );
    write(d.path(), "workflows/sayac.json", &flow("sayac", SET));
    write(
        d.path(),
        "başka/workflows/temizlik.json",
        &flow("temizlik", SET),
    );
    let sub = d.path().join("workflows");
    let ws = workspace::find(&sub).unwrap();
    assert_eq!(ws.port, 8790);
    assert_eq!(ws.root, d.path());
    assert_eq!(ws.journal(), d.path().join(".itero/journal.sqlite"));
    let mut names: Vec<String> = ws
        .workflows()
        .unwrap()
        .into_iter()
        .map(|(_, f)| f.name)
        .collect();
    names.sort();
    assert_eq!(names, ["sayac", "temizlik"]);
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
    assert!(ws.workflows().unwrap().is_empty());
    assert!(ws.actions().unwrap().is_empty());
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

#[test]
fn workspace_actions_are_found_by_name_and_checked_against_workflows() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(
        d.path(),
        "actions/posta/action.json",
        r##"{ "name": "posta", "version": 2, "type": "executable",
             "targets": { "kontrol": { "executable": "./kontrol.sh" } } }"##,
    );
    write(
        d.path(),
        "workflows/okur.json",
        &flow("okur", "\"posta.kontrol@2\""),
    );
    let ws = workspace::find(d.path()).unwrap();
    let (folder, def) = ws.action("posta").unwrap();
    assert_eq!(folder, d.path().join("actions/posta"));
    assert_eq!(def.version, 2);
    assert_eq!(ws.workflows().unwrap().len(), 1);
    write(
        d.path(),
        "workflows/okur.json",
        &flow("okur", "\"posta.kontrol@1\""),
    );
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(e.contains("at version 2, not 1"), "{e}");
}

#[test]
fn two_workflows_with_one_name_are_an_error_naming_both() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "workflows = [\"bir\", \"iki\"]\n");
    write(d.path(), "bir/cift.json", &flow("cift", SET));
    write(d.path(), "iki/cift.json", &flow("cift", SET));
    let ws = workspace::find(d.path()).unwrap();
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(
        e.contains("two workflows named cift")
            && e.contains("bir/cift.json")
            && e.contains("iki/cift.json"),
        "{e}"
    );
}

#[test]
fn workspace_triggers_add_to_or_replace_a_workflows_own() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    let own = r##"{ "name": "posta", "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti" } ] }"##;
    write(d.path(), "workflows/posta.json", own);
    let ws = workspace::find(d.path()).unwrap();
    assert_eq!(ws.triggers, d.path().join("triggers"));
    let triggers = |ws: &workspace::Workspace| ws.workflows().unwrap()[0].1.triggers.clone();
    assert_eq!(triggers(&ws).len(), 1);
    write(
        d.path(),
        "triggers/posta.json",
        r##"{ "triggers": [ { "on": "posta.geldi", "args": { "kutu": "gelen kutusu" } } ] }"##,
    );
    let t = triggers(&ws);
    assert_eq!(t.len(), 2, "added");
    assert_eq!(t[1].on.as_deref(), Some("posta.geldi"));
    write(
        d.path(),
        "triggers/posta.json",
        r##"{ "replace": true, "triggers": [ { "schedule": "0 9 * * *", "args": { "kutu": "iş" } } ] }"##,
    );
    let t = triggers(&ws);
    assert_eq!(t.len(), 1, "replaced");
    assert_eq!(t[0].schedule.as_deref(), Some("0 9 * * *"));
    assert_eq!(t[0].args["kutu"], "iş");
}

#[test]
fn a_trigger_file_for_no_workflow_or_a_bad_trigger_is_an_error_naming_it() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(d.path(), "workflows/posta.json", &flow("posta", SET));
    write(
        d.path(),
        "triggers/yok.json",
        r##"{ "triggers": [ { "on": "x" } ] }"##,
    );
    let ws = workspace::find(d.path()).unwrap();
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(
        e.contains("triggers/yok.json") && e.contains("no workflow yok"),
        "{e}"
    );
    std::fs::remove_file(d.path().join("triggers/yok.json")).unwrap();
    write(
        d.path(),
        "triggers/posta.json",
        r##"{ "triggers": [ { "schedule": "her gün" } ] }"##,
    );
    let e = format!("{:#}", ws.workflows().err().unwrap());
    assert!(
        e.contains("triggers/posta.json") && e.contains("her gün"),
        "{e}"
    );
}

#[test]
fn a_container_action_names_its_image_and_nothing_else_does() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(
        d.path(),
        "actions/posta/action.json",
        r##"{ "name": "posta", "version": 1, "type": "container", "image": "python:3.13-alpine",
             "description": "postaya bakış", "environment": { "DİL": "tr" },
             "limits": { "timeout": "30s", "prepare": "15m" },
             "targets": { "bak": { "executable": "python3", "arguments": ["bak.py", "--kutu", "Gelen"] } } }"##,
    );
    let ws = workspace::find(d.path()).unwrap();
    let (_, def) = ws.action("posta").unwrap();
    assert_eq!(def.image.as_deref(), Some("python:3.13-alpine"));
    assert_eq!(def.limits.unwrap().prepare.as_deref(), Some("15m"));
    for (text, says) in [
        (
            r##"{ "name": "posta", "version": 1, "type": "container", "targets": { "bak": { "executable": "python3" } } }"##,
            "a container needs image",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "executable", "image": "python:3.13-alpine", "targets": { "bak": { "executable": "./bak.sh" } } }"##,
            "image is for the container type",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "container", "image": "--privileged", "targets": { "bak": { "executable": "sh" } } }"##,
            "is not an image name",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "container", "image": "alpine:3.22", "limits": { "heartbeat": "10s" }, "targets": { "bak": { "executable": "sh" } } }"##,
            "heartbeat does not reach a container",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "executable", "limits": { "prepare": "5m" }, "targets": { "bak": { "executable": "./bak.sh" } } }"##,
            "prepare is for the container type",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "container", "image": "alpine:3.22", "limits": { "prepare": "bir saat" }, "targets": { "bak": { "executable": "sh" } } }"##,
            "bir saat",
        ),
        (
            r##"{ "name": "posta", "version": 1, "type": "container", "image": "alpine:3.22", "targets": { "bak": { "module": "bakış" } } }"##,
            "an executable target",
        ),
    ] {
        write(d.path(), "actions/posta/action.json", text);
        let e = format!("{:#}", ws.actions().unwrap_err());
        assert!(e.contains("action.json") && e.contains(says), "{text}: {e}");
    }
}

#[test]
fn a_workspace_reads_its_sources() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(
        d.path(),
        "sources/posta.json",
        r##"{ "source": "watch@1", "with": { "path": "/var/mail/ayşe" }, "emit": "posta.degisti" }"##,
    );
    write(
        d.path(),
        "sources/takvim.json",
        r##"{ "program": ["./takvim.sh", "--gün", "bugün"], "with": { "hesap": "iş" } }"##,
    );
    write(
        d.path(),
        "sources/kutu.json",
        r##"{ "program": ["python3", "bekle.py"], "image": "python:3.13-alpine", "emit": "posta.geldi",
             "with": { "parola": "${env.IMAP_PAROLA}" } }"##,
    );
    let ws = workspace::find(d.path()).unwrap();
    let all = ws.sources().unwrap();
    let names: Vec<&str> = all.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, ["kutu", "posta", "takvim"]);
    assert_eq!(all[2].2.program, ["./takvim.sh", "--gün", "bugün"]);
    assert_eq!(all[0].2.image.as_deref(), Some("python:3.13-alpine"));
    assert_eq!(all[1].1, d.path().join("sources"));
}

#[test]
fn a_bad_source_file_is_an_error_naming_it() {
    for (text, says) in [
        (
            r##"{ "source": "watch@1", "emit": "a.b" }"##,
            "watch@1 needs with.path",
        ),
        (
            r##"{ "source": "watch@1", "with": { "path": "/x" } }"##,
            "watch@1 needs emit",
        ),
        (
            r##"{ "source": "izle@1", "with": {}, "emit": "a.b" }"##,
            "no built-in source izle@1",
        ),
        (
            r##"{ "program": [], "emit": "a.b" }"##,
            "a source is a built-in (source) or a program",
        ),
        (
            r##"{ "source": "watch@1", "program": ["x"], "with": { "path": "/x" }, "emit": "a.b" }"##,
            "a source is a built-in (source) or a program",
        ),
        (
            r##"{ "source": "watch@1", "image": "alpine:3.22", "with": { "path": "/x" }, "emit": "a.b" }"##,
            "image is for a program",
        ),
        (
            r##"{ "program": ["sh"], "image": "-v", "emit": "a.b" }"##,
            "is not an image name",
        ),
        (
            r##"{ "program": ["x"], "emit": "workflow.paused" }"##,
            "the engine's own",
        ),
        (
            r##"{ "program": ["x"], "emit": "Büyük.Harf" }"##,
            "an event type",
        ),
        (
            r##"{ "program": ["x"], "with": { "s": "${env.}" } }"##,
            "bad expression",
        ),
        (r##"{ "program": ["x"], "renk": "mavi" }"##, "unknown field"),
    ] {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "itero.toml", "");
        write(d.path(), "sources/kotu.json", text);
        let e = format!(
            "{:#}",
            workspace::find(d.path()).unwrap().sources().unwrap_err()
        );
        assert!(e.contains("kotu.json") && e.contains(says), "{text}: {e}");
    }
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "itero.toml", "");
    write(
        d.path(),
        "sources/Kötü Ad.json",
        r##"{ "program": ["x"] }"##,
    );
    let e = format!(
        "{:#}",
        workspace::find(d.path()).unwrap().sources().unwrap_err()
    );
    assert!(e.contains("lowercase letters"), "{e}");
}

#[test]
fn a_workflows_version_leaves_its_triggers_out() {
    let a = itero::workflow::parse(
        r##"{ "name": "akis", "triggers": [ { "schedule": "*/5 * * * *" } ],
      "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti" } ] }"##,
    )
    .unwrap();
    let mut b = a.clone();
    b.triggers.clear();
    assert_eq!(itero::workflow::version(&a), itero::workflow::version(&b));
}

// The simple form: workflows, action files, source files, args.

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
        r##"{ "on": ["mail.received"], "steps": ["#notify"] }"##,
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
