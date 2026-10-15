use itero::workspace;
use std::path::Path;

/// An action that runs in the manager and needs nothing from outside.
const SET: &str = r#"{ "action": "memory.set@1", "with": { "son": "ölçüm" } }"#;

fn flow(name: &str, action: &str) -> String {
    format!(
        r#"{{ "name": "{name}", "description": "ölçüm", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
             "transitions": [ {{ "from": "basla", "to": "bitti", "do": [ {action} ] }} ] }}"#
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
        r#"{ "name": "posta", "version": 2, "type": "executable",
             "targets": { "kontrol": { "executable": "./kontrol.sh" } } }"#,
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
