//! The command line: itero args and itero secret.
use std::path::Path;
use std::process::Output;

fn itero(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap()
}

fn workspace() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.toml"), "").unwrap();
    d
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn args_are_written_for_the_action() {
    let d = workspace();
    let o = itero(d.path(), &["args", "kargola", r#"{"firma": "PTT Kargo"}"#]);
    assert!(o.status.success(), "{o:?}");
    let f = d.path().join("args/kargola.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v["firma"], "PTT Kargo");
    // from a file, a list of objects
    let list = d.path().join("hesaplar.json");
    std::fs::write(
        &list,
        r#"[{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]"#,
    )
    .unwrap();
    let o = itero(
        d.path(),
        &["args", "posta-bak", "--file", list.to_str().unwrap()],
    );
    assert!(o.status.success(), "{o:?}");
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(d.path().join("args/posta-bak.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(v[1]["user"], "can@örnek.test");
}

#[test]
fn args_that_are_not_an_object_or_a_list_of_objects_are_refused() {
    let d = workspace();
    for bad in ["[1, 2]", r#""çay""#, "{", "42"] {
        let o = itero(d.path(), &["args", "kargola", bad]);
        assert!(!o.status.success(), "{bad}");
        assert!(
            err(&o).contains("an object, or a list of objects"),
            "{bad}: {}",
            err(&o)
        );
    }
    assert!(!d.path().join("args/kargola.json").exists());
    let o = itero(d.path(), &["args", "../dışarı", "{}"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("../dışarı"), "{}", err(&o));
}

#[test]
fn a_secret_needs_the_store_and_a_name_like_an_environment_variable() {
    let d = workspace();
    let o = itero(d.path(), &["secret", "API_KEY"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("secret store is off"), "{}", err(&o));
    let o = itero(d.path(), &["secret", "api-anahtarı"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("api-anahtarı"), "{}", err(&o));
    let o = itero(d.path(), &["secret", "API_KEY", "--file", "yok.txt"]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("store is off") || err(&o).contains("yok.txt"),
        "{}",
        err(&o)
    );
}

fn file(d: &Path, rel: &str, text: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, text).unwrap();
    #[cfg(unix)]
    if rel.starts_with("actions/") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn check_says_what_each_workflow_uses_and_what_is_not_there() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": ["order.placed"], "steps": [["#ödeme", "stok"], "#notify"] }"##,
    );
    file(
        d.path(),
        "workflows/stok.json",
        r##"{ "steps": ["#depo"] }"##,
    );
    file(d.path(), "actions/ödeme", "#!/bin/sh\n");
    file(d.path(), "actions/depo", "#!/bin/sh\n");
    let o = itero(d.path(), &["check"]);
    assert!(o.status.success(), "{o:?}");
    let text = out(&o);
    assert!(text.contains("sipariş\n  #ödeme  "), "{text}");
    assert!(text.contains("  stok  workflow"), "{text}");
    assert!(text.contains("  #notify  built in"), "{text}");
    std::fs::remove_file(d.path().join("actions/depo")).unwrap();
    let o = itero(d.path(), &["check"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("no action depo"), "{}", err(&o));
    file(d.path(), "actions/depo", "#!/bin/sh\n");
    file(d.path(), "workflows/stok.json", r#"{ "steps": ["kayıp"] }"#);
    let o = itero(d.path(), &["check"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("kayıp"), "{}", err(&o));
}

#[test]
fn runs_and_errors_say_how_each_run_is_doing() {
    let d = workspace();
    file(
        d.path(),
        "workflows/iyi.json",
        r##"{ "on": ["a.b"], "steps": ["#tamam"] }"##,
    );
    file(
        d.path(),
        "workflows/kötü.json",
        r##"{ "on": ["a.b"], "steps": ["#bozuk"] }"##,
    );
    file(d.path(), "actions/tamam", "#!/bin/sh\n");
    file(
        d.path(),
        "actions/bozuk",
        "#!/bin/sh\necho 'olmadı' >&2\nexit 2\n",
    );
    assert!(
        itero(d.path(), &["emit", "a.b", r#"{"kim": "ayşe"}"#])
            .status
            .success()
    );
    let o = itero(d.path(), &["daemon", "--once"]);
    assert!(o.status.success(), "{o:?}");
    let runs = out(&itero(d.path(), &["runs"]));
    assert!(
        runs.lines()
            .any(|l| l.contains("iyi") && l.contains("done")),
        "{runs}"
    );
    assert!(
        runs.lines()
            .any(|l| l.contains("kötü") && l.contains("error")),
        "{runs}"
    );
    let errors = out(&itero(d.path(), &["errors"]));
    assert_eq!(errors.lines().count(), 1, "{errors}");
    assert!(
        errors.contains("kötü") && errors.contains("#bozuk: exit 2: olmadı"),
        "{errors}"
    );
}
