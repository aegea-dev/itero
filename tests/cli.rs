//! The command line: the commands by kind, their args and settings, secrets.
#[path = "common/stop.rs"]
mod stop;
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
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    d
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn action_args_and_workflow_settings_are_gone() {
    let d = workspace();
    file(d.path(), "actions/kargola", "#!/bin/sh\n");
    for args in [
        &["ws", "ac", "args", "get", "kargola"][..],
        &["ws", "wf", "config", "get", "kargola"],
        &["ws", "cn", "args", "get", "kargola"],
        &["ws", "ac", "repeat", "get", "kargola"],
    ] {
        let o = itero(d.path(), args);
        assert!(
            !o.status.success() && err(&o).contains("unrecognized subcommand"),
            "{o:?}"
        );
    }
}

#[test]
fn check_refuses_values_in_the_manifest() {
    let d = workspace();
    file(d.path(), "actions/kargola", "#!/bin/sh\n");
    file(
        d.path(),
        "workflows/gözcü.json",
        r##"{ "on": [{ "event": "saat.geldi" }], "steps": ["#kargola"] }"##,
    );
    file(
        d.path(),
        "itero.config.json",
        r#"{"action": {"kargola": {"args": {"firma": "PTT Kargo"}}}}"#,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("itero.config.json: args are gone")
            && err(&o).contains("/actions/kargola)"),
        "{o:?}"
    );
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"gözcü": {"settings": {"siteler": ["https://örnek.test"]}}}}"#,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success() && err(&o).contains("settings are gone"),
        "{o:?}"
    );
    // an action's repeat: onto the step
    file(
        d.path(),
        "itero.config.json",
        r#"{"action": {"kargola": {"repeat": [{"firma": "PTT Kargo"}]}}}"#,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("repeat is gone")
            && err(&o).contains("/actions/kargola)"),
        "{o:?}"
    );
    // a connector's args: into each on entry that listens
    file(d.path(), "connectors/posta.geldi", "#!/bin/sh\n");
    file(
        d.path(),
        "itero.config.json",
        r#"{"connector": {"posta.geldi": {"args": {"KUTU": "satış"}}}}"#,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("itero.config.json: args are gone")
            && err(&o).contains("on entry")
            && err(&o).contains("/connectors/posta.geldi)"),
        "{o:?}"
    );
}

#[test]
fn ws_add_keeps_env_out_of_git() {
    let state = tempfile::tempdir().unwrap();
    let d = workspace();
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join(".gitignore")).unwrap(),
        ".env\n"
    );
    // a .gitignore of its own: the line added once, after what is there
    let d = workspace();
    file(d.path(), ".gitignore", "node_modules");
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join(".gitignore")).unwrap(),
        "node_modules\n.env\n"
    );
    let d = workspace();
    file(d.path(), ".gitignore", "/.env\n");
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join(".gitignore")).unwrap(),
        "/.env\n"
    );
}

#[test]
#[cfg(unix)]
fn connectors_are_listed_once_for_each_set_of_args_the_workflows_give() {
    let d = workspace();
    file(d.path(), "connectors/posta.geldi", "#!/bin/sh\n");
    file(d.path(), "connectors/saat.geldi", "#!/bin/sh\n");
    file(
        d.path(),
        "workflows/satış.json",
        r##"{ "on": [{ "event": "posta.geldi", "args": { "KUTU": "satış" } }], "steps": ["#notify"] }"##,
    );
    file(
        d.path(),
        "workflows/destek.json",
        r##"{ "on": [{ "event": "posta.geldi", "args": { "KUTU": "destek" } }], "steps": ["#notify"] }"##,
    );
    let v: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["ws", "cn", "list", "--json"]))).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 3, "{v}");
    assert_eq!(v[0]["type"], "posta.geldi");
    assert_eq!(v[0]["args"], serde_json::json!({ "KUTU": "destek" }));
    assert_eq!(v[1]["args"], serde_json::json!({ "KUTU": "satış" }));
    assert_eq!(
        (v[2]["type"].as_str(), v[2].get("args")),
        (Some("saat.geldi"), None)
    );
    let text = out(&itero(d.path(), &["ws", "cn", "list"]));
    assert!(text.contains(r#"{"KUTU":"satış"}"#), "{text}");
}

#[test]
fn a_workspaces_secret_needs_the_store_and_a_workspace() {
    let d = workspace();
    file(d.path(), "değer", "gizli");
    let o = itero(d.path(), &["ws", "secret", "POSTA", "--file", "değer"]);
    assert!(
        !o.status.success() && err(&o).contains("ITERO_NO_SECRET_STORE"),
        "{}",
        err(&o)
    );
    let o = itero(d.path(), &["ws", "secret", "ad-soyad", "--file", "değer"]);
    assert!(
        !o.status.success() && err(&o).contains("a secret's name"),
        "{}",
        err(&o)
    );
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
    if rel.starts_with("actions/") || rel.starts_with("connectors/") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn check_refuses_an_entry_naming_nothing_and_folders_from_before() {
    let d = workspace();
    file(
        d.path(),
        "workflows/gözcü.json",
        r##"{ "on": [{ "event": "saat.geldi" }], "steps": ["#bak"] }"##,
    );
    file(d.path(), "actions/bak", "#!/bin/sh\n");
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"gözcü": {"description": "Siteler"}}}"#,
    );
    assert!(itero(d.path(), &["ws", "check"]).status.success());
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"gözcü": {}, "yok-böyle": {}}, "action": {"hayalet": {}}}"#,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("no workflow yok-böyle (")
            && err(&o).contains("/workflows/yok-böyle)")
            && err(&o).contains("no action hayalet (")
            && err(&o).contains("/actions/hayalet)"),
        "{o:?}"
    );
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), "args/bak.json", "{}");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("args/: is gone")
            && err(&o).contains("the step that runs it"),
        "{o:?}"
    );
}

#[test]
fn check_says_what_each_workflow_uses_and_what_is_not_there() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "order.placed" }], "steps": [["#ödeme", "stok"], "#notify"] }"##,
    );
    file(
        d.path(),
        "workflows/stok.json",
        r##"{ "steps": ["#depo"] }"##,
    );
    file(d.path(), "actions/ödeme", "#!/bin/sh\n");
    file(d.path(), "actions/depo", "#!/bin/sh\n");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(o.status.success(), "{o:?}");
    let text = out(&o);
    assert!(
        text.contains("sipariş\n  id ") && text.contains("  #ödeme  "),
        "{text}"
    );
    assert!(text.contains("  stok  workflow"), "{text}");
    assert!(text.contains("  #notify  built in"), "{text}");
    // the internal ones last, apart: the built-in keep and logs
    let (work, internal) = text.split_once("\ninternal\n").expect(&text);
    assert!(!work.contains("#forget"), "{text}");
    for says in ["keep", "#forget  built in", "logs", "#roll-log  built in"] {
        assert!(internal.contains(says), "{says}: {text}");
    }
    std::fs::remove_file(d.path().join("actions/depo")).unwrap();
    let o = itero(d.path(), &["ws", "check"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("no action depo"), "{}", err(&o));
    file(d.path(), "actions/depo", "#!/bin/sh\n");
    file(d.path(), "workflows/stok.json", r#"{ "steps": ["kayıp"] }"#);
    let o = itero(d.path(), &["ws", "check"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("kayıp"), "{}", err(&o));
}

#[test]
fn check_names_a_wrong_limit_of_an_action_once() {
    let d = tempfile::tempdir().unwrap();
    file(
        d.path(),
        "itero.config.json",
        r#"{"action": {"depo": {"limits": {"runs": 2}}}}"#,
    );
    file(
        d.path(),
        "actions/depo",
        "#!/bin/sh
",
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(
        e.contains("itero.config.json: limits: runs: an action's limits are quiet and most (")
            && e.contains("/actions/depo)"),
        "{e}"
    );
    assert_eq!(e.matches("itero.config.json").count(), 1, "{e}");
}

#[test]
fn runs_and_errors_say_how_each_run_is_doing() {
    let d = workspace();
    file(
        d.path(),
        "workflows/iyi.json",
        r##"{ "on": [{ "event": "a.b" }], "steps": ["#tamam"] }"##,
    );
    file(
        d.path(),
        "workflows/kötü.json",
        r##"{ "on": [{ "event": "a.b" }], "steps": ["#bozuk"] }"##,
    );
    file(d.path(), "actions/tamam", "#!/bin/sh\n");
    file(
        d.path(),
        "actions/bozuk",
        "#!/bin/sh\necho 'olmadı' >&2\nexit 2\n",
    );
    assert!(
        itero(d.path(), &["ws", "emit", "a.b", r#"{"kim": "ayşe"}"#])
            .status
            .success()
    );
    let o = itero(d.path(), &["ws", "daemon", "--once"]);
    assert!(o.status.success(), "{o:?}");
    let runs = out(&itero(d.path(), &["ws", "wf", "runs"]));
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
    file(
        d.path(),
        "workflows/bakım.json",
        r##"{ "on": [{ "event": "a.b" }], "steps": ["#tamam"] }"##,
    );
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"bakım": {"internal": true}}}"#,
    );
    itero(d.path(), &["ws", "emit", "a.b", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    assert!(!out(&itero(d.path(), &["ws", "wf", "runs"])).contains("bakım"));
    assert!(out(&itero(d.path(), &["ws", "wf", "runs", "--all"])).contains("bakım"));
    let errors = out(&itero(d.path(), &["ws", "wf", "runs", "--errors"]));
    assert_eq!(errors.lines().count(), 2, "{errors}");
    assert!(
        errors.contains("kötü") && errors.contains("#bozuk: exit 2: olmadı"),
        "{errors}"
    );
}

#[test]
fn runs_with_a_name_or_id_tells_the_runs_story() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#öde", "#gönder"] }"##,
    );
    file(
        d.path(),
        "actions/öde",
        "#!/bin/sh\ncase \"$event\" in *'\"tamam\":true'*) exit 0 ;; esac\necho '{\"ödeme\": \"bekliyor\", \"sipariş\": 1}'; exit 75\n",
    );
    file(
        d.path(),
        "actions/gönder",
        "#!/bin/sh\necho '{\"kargo\": \"TR-ü1\"}'\n",
    );
    itero(
        d.path(),
        &["ws", "emit", "sipariş.geldi", r#"{"sipariş": 1}"#],
    );
    itero(d.path(), &["ws", "daemon", "--once"]);
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    let name = list.split_whitespace().nth(1).unwrap().to_string();
    let waiting = out(&itero(d.path(), &["ws", "wf", "runs", &name]));
    assert!(
        waiting.contains("sipariş") && waiting.contains("waiting"),
        "{waiting}"
    );
    assert!(
        waiting.contains("#öde") && waiting.contains("bekliyor"),
        "{waiting}"
    );
    itero(
        d.path(),
        &[
            "ws",
            "emit",
            "ödeme.geldi",
            r#"{"sipariş": 1, "tamam": true}"#,
        ],
    );
    itero(d.path(), &["ws", "daemon", "--once"]);
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["ws", "wf", "runs", id]));
    for says in ["done", "ödeme.geldi", "#gönder", "TR-ü1", "state"] {
        assert!(story.contains(says), "{says}: {story}");
    }
    let o = itero(d.path(), &["ws", "wf", "runs", "yok-böyle"]);
    assert!(
        !o.status.success() && err(&o).contains("no run yok-böyle"),
        "{o:?}"
    );
}

#[test]
fn a_step_that_lost_a_race_is_stopped_in_the_story() {
    let d = workspace();
    file(
        d.path(),
        "workflows/yarış.json",
        r##"{ "on": [{ "event": "yarış.başladı" }], "steps": [{ "race": ["#hızlı", "#yavaş"] }] }"##,
    );
    file(d.path(), "actions/hızlı", "#!/bin/sh\necho hızlı\n");
    file(d.path(), "actions/yavaş", "#!/bin/sh\nsleep 30\n");
    itero(d.path(), &["ws", "emit", "yarış.başladı", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["ws", "wf", "runs", id]));
    assert!(story.contains("#yavaş  stopped"), "{story}");
    assert!(story.contains("#hızlı  done"), "{story}");
}

#[test]
fn a_waiting_step_and_a_workflow_that_lost_are_stopped_in_the_story_after_the_winner() {
    let d = workspace();
    file(
        d.path(),
        "workflows/yarış.json",
        r##"{ "on": [{ "event": "yarış.başladı" }],
              "steps": [{ "race": ["#bekle", "demleme", "#hızlı"] }] }"##,
    );
    file(
        d.path(),
        "workflows/demleme.json",
        r##"{ "steps": ["#su"] }"##,
    );
    file(
        d.path(),
        "actions/bekle",
        "#!/bin/sh\necho '{\"sipariş\": 1}'; exit 75\n",
    );
    file(d.path(), "actions/su", "#!/bin/sh\nsleep 3\n");
    file(
        d.path(),
        "actions/hızlı",
        "#!/bin/sh\nsleep 1; echo hızlı\n",
    );
    itero(d.path(), &["ws", "emit", "yarış.başladı", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let list = out(&itero(
        d.path(),
        &["ws", "wf", "runs", "--workflow", "yarış"],
    ));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["ws", "wf", "runs", id]));
    // the workflow that lost is a run of its own: it ran to its end
    let child = out(&itero(
        d.path(),
        &["ws", "wf", "runs", "--workflow", "demleme"],
    ));
    assert!(child.contains(" done "), "{child}");
    let at = |says: &str| {
        story
            .find(says)
            .unwrap_or_else(|| panic!("no {says}: {story}"))
    };
    let won = at("#hızlı  done");
    assert!(at("#bekle  stopped") > won, "{story}");
    assert!(at("demleme  stopped") > won, "{story}");
    assert!(!story.contains("#su  stopped"), "{story}");
}

#[test]
fn a_workflow_used_as_a_step_is_listed_from_its_parent_and_named_in_its_story() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["stok", "#son"] }"##,
    );
    file(
        d.path(),
        "workflows/stok.json",
        r##"{ "steps": ["#say"] }"##,
    );
    file(
        d.path(),
        "actions/say",
        "#!/bin/sh
echo 3
",
    );
    file(
        d.path(),
        "actions/son",
        "#!/bin/sh
echo tamam
",
    );
    itero(d.path(), &["ws", "emit", "sipariş.geldi", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let json_of = |args: &[&str]| -> serde_json::Value {
        serde_json::from_str(&out(&itero(d.path(), args))).unwrap()
    };
    let parent = json_of(&["ws", "wf", "runs", "--workflow", "sipariş", "--json"]);
    let child = json_of(&["ws", "wf", "runs", "--workflow", "stok", "--json"]);
    let (pid, pname) = (
        parent[0]["id"].as_str().unwrap(),
        parent[0]["name"].as_str().unwrap(),
    );
    assert_eq!(child[0]["parent"], pid, "{child}");
    assert_eq!(parent[0]["parent"], serde_json::Value::Null);
    // the list says which run it is from
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    let line = list.lines().find(|l| l.contains(" stok ")).unwrap();
    assert!(line.contains(&format!("from {pname}")), "{list}");
    // the parent's story names the run at its step
    let cname = child[0]["name"].as_str().unwrap();
    let story = out(&itero(d.path(), &["ws", "wf", "runs", pid]));
    assert!(
        story.contains(&format!("stok  done  3  run {cname}")),
        "{story}"
    );
    // and the child's, the run it is from
    let story = out(&itero(d.path(), &["ws", "wf", "runs", cname]));
    assert!(story.contains(&format!("from {pname}")), "{story}");
}

#[test]
fn an_event_not_yet_handled_is_listed_and_cancelled() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kayıt.json",
        r##"{ "on": [{ "event": "kayıt.geldi" }], "steps": ["#yaz"] }"##,
    );
    file(d.path(), "actions/yaz", "#!/bin/sh\necho yazıldı\n");
    let first = out(&itero(
        d.path(),
        &["ws", "emit", "kayıt.geldi", r#"{"n": 1}"#],
    ));
    let first = first.trim();
    itero(d.path(), &["ws", "emit", "kayıt.geldi", r#"{"n": 2}"#]);
    let v: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["ws", "ev", "list", "--json"]))).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v[0]["id"], first);
    assert_eq!(v[0]["type"], "kayıt.geldi");
    assert_eq!(v[0]["data"], serde_json::json!({"n": 1}));
    let text = out(&itero(d.path(), &["ws", "ev", "list"]));
    assert!(
        text.contains(first) && text.contains("kayıt.geldi"),
        "{text}"
    );
    let o = itero(d.path(), &["ws", "ev", "cancel", first]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o).trim(), format!("cancelled {first}"));
    let v: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["ws", "ev", "list", "--json"]))).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let runs: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["ws", "wf", "runs", "--json"]))).unwrap();
    assert_eq!(
        runs.as_array().unwrap().len(),
        1,
        "the cancelled one started nothing"
    );
    // handled already, and an id that is no event: refused
    let o = itero(d.path(), &["ws", "ev", "cancel", first]);
    assert!(
        !o.status.success() && err(&o).contains("already handled"),
        "{}",
        err(&o)
    );
    let o = itero(d.path(), &["ws", "ev", "cancel", "yok"]);
    assert!(
        !o.status.success() && err(&o).contains("no event yok"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_run_is_stopped_by_command_and_its_story_says_so() {
    let d = workspace();
    file(
        d.path(),
        "workflows/bekleyen.json",
        r##"{ "on": [{ "event": "bekle.başla" }], "steps": ["#bekle"] }"##,
    );
    file(
        d.path(),
        "actions/bekle",
        "#!/bin/sh\necho '{\"sipariş\": 1}'; exit 75\n",
    );
    itero(d.path(), &["ws", "emit", "bekle.başla", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let runs: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["ws", "wf", "runs", "--json"]))).unwrap();
    let name = runs[0]["name"].as_str().unwrap().to_string();
    let o = itero(d.path(), &["ws", "wf", "stop", &name]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o).trim(), format!("stopping {name}"));
    assert!(err(&o).contains("no daemon is running"), "{}", err(&o));
    itero(d.path(), &["ws", "daemon", "--once"]);
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    assert!(list.contains(" stopped "), "{list}");
    let story = out(&itero(d.path(), &["ws", "wf", "runs", &name]));
    assert!(
        story.contains("stopped by command") && story.contains("ended stopped"),
        "{story}"
    );
    let o = itero(d.path(), &["ws", "wf", "stop", &name]);
    assert!(
        !o.status.success() && err(&o).contains(&format!("{name} has ended: stopped")),
        "{}",
        err(&o)
    );
    let o = itero(d.path(), &["ws", "wf", "stop", "yok-yok"]);
    assert!(
        !o.status.success() && err(&o).contains("no run yok-yok"),
        "{}",
        err(&o)
    );
}

#[test]
fn a_step_that_breaks_undoes_those_before_it_and_the_story_says_so() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kurulum.json",
        r##"{ "on": [{ "event": "küme.istendi" }],
              "steps": [{ "#kur": { "undo": { "#sil": { "args": { "kume": "$output.küme" } } } } }, "#dene"],
              "error": ["#bildir"] }"##,
    );
    file(
        d.path(),
        "actions/kur",
        "#!/bin/sh\necho kur >> sıra.txt\necho '{\"küme\": \"k-1\"}'\n",
    );
    file(
        d.path(),
        "actions/dene",
        "#!/bin/sh\necho 'yanıt yok' >&2; exit 1\n",
    );
    file(
        d.path(),
        "actions/sil",
        "#!/bin/sh\necho \"sil $kume\" >> sıra.txt\n",
    );
    file(
        d.path(),
        "actions/bildir",
        "#!/bin/sh\necho bildir >> sıra.txt\n",
    );
    itero(
        d.path(),
        &["ws", "emit", "küme.istendi", r#"{"ad": "deneme"}"#],
    );
    itero(d.path(), &["ws", "daemon", "--once"]);
    let order = std::fs::read_to_string(d.path().join("sıra.txt")).unwrap();
    assert_eq!(order, "kur\nsil k-1\nbildir\n");
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["ws", "wf", "runs", id]));
    assert!(story.contains("#sil (undo)  done"), "{story}");
    assert!(story.contains("#dene  error"), "{story}");
}

#[test]
fn info_shows_the_workspace_and_check_each_workflows_identity() {
    let d = workspace();
    // a port nothing listens on, so no daemon of this machine answers
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    file(
        d.path(),
        "itero.config.json",
        &format!(
            r#"{{"name": "dükkân-işleri", "port": {port}, "sahibi": "ayşe", "limits": {{"runs": 7}},
                 "workflow": {{"sipariş": {{"description": "Dükkân", "ekip": "ödeme"}}}}}}"#
        ),
    );
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let info = out(&itero(d.path(), &["ws", "info"]));
    for says in [
        "id ",
        "name dükkân-işleri",
        "created ",
        "path ",
        "sahibi",
        "ayşe",
        "runs 7",
        "daemon: not running",
    ] {
        assert!(info.contains(says), "{says}: {info}");
    }
    let check = out(&itero(d.path(), &["ws", "check"]));
    for says in ["sipariş", "id ", "Dükkân", "ekip", "runs so far 0"] {
        assert!(check.contains(says), "{says}: {check}");
    }
}

#[test]
fn workflows_lists_each_workflow_and_what_waits_for_a_daemon() {
    let d = workspace();
    file(
        d.path(),
        "itero.config.json",
        r#"{"port": 8799, "workflow": {"özet": {"off": true}}}"#,
    );
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "order.placed" }], "steps": ["#gönder"] }"##,
    );
    file(
        d.path(),
        "workflows/özet.json",
        r##"{ "on": [{ "schedule": "0 9 * * *" }, { "path": "gelen" }], "steps": ["#gönder"] }"##,
    );
    file(
        d.path(),
        "workflows/adım.json",
        r##"{ "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    itero(
        d.path(),
        &["ws", "emit", "order.placed", r#"{"sipariş": 1}"#],
    );
    let before = out(&itero(d.path(), &["ws", "wf", "list"]));
    let line = |text: &str, name: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("{name}: {text}"))
            .to_string()
    };
    let s = line(&before, "sipariş");
    assert!(
        s.contains("on order.placed") && s.contains("runs 0") && s.contains("last: none"),
        "{before}"
    );
    let o = line(&before, "özet");
    assert!(
        o.contains("at 0 9 * * *") && o.contains("path gelen") && o.contains("off"),
        "{before}"
    );
    assert!(line(&before, "adım").contains("a step only"), "{before}");
    assert!(line(&before, "keep").contains("internal"), "{before}");
    assert!(
        before.lines().last().unwrap() == "daemon: not running, 1 event waiting",
        "{before}"
    );
    itero(d.path(), &["ws", "daemon", "--once"]);
    let after = out(&itero(d.path(), &["ws", "wf", "list"]));
    let s = line(&after, "sipariş");
    assert!(s.contains("runs 1") && s.contains("last: done"), "{after}");
    assert_eq!(
        after.lines().last().unwrap(),
        "daemon: not running, no event waiting"
    );
}

#[test]
fn emit_says_what_a_message_starts() {
    let d = workspace();
    file(d.path(), "itero.config.json", r#"{"port": 8798}"#);
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "order.placed" }], "steps": ["#gönder"] }"##,
    );
    file(
        d.path(),
        "workflows/kayıt.json",
        r##"{ "on": [{ "event": "order.placed" }], "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let o = itero(
        d.path(),
        &["ws", "emit", "order.placed", r#"{"sipariş": 1}"#],
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(out(&o).trim().len(), 36, "the message's id on stdout");
    let said = err(&o);
    assert!(said.contains("starts: kayıt, sipariş"), "{said}");
    assert!(
        said.contains("no daemon is running: it waits for one (itero daemon"),
        "{said}"
    );
    let o = itero(d.path(), &["ws", "emit", "order.places", "{}"]);
    assert!(o.status.success(), "{o:?}");
    let said = err(&o);
    assert!(
        said.contains("nothing starts on order.places: did you mean order.placed?"),
        "{said}"
    );
    let o = itero(d.path(), &["ws", "emit", "çok.başka.bir.şey", "{}"]);
    assert!(!err(&o).contains("did you mean"), "{}", err(&o));
    // every one is saved all the same
    let ws = itero::workspace::find(d.path()).unwrap();
    let j = itero::engine::journal::Journal::open(&ws).unwrap();
    assert_eq!(j.events(Some("itero.app")).unwrap().len(), 3);
}

/// A daemon serving the workspace in `d` on a port the OS gives it, once
/// it answers there.
fn serving(d: &Path) -> stop::Daemon {
    // held before the wait, so a daemon that never answers is stopped too
    let daemon = stop::Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["ws", "daemon"])
            .current_dir(d)
            .env("ITERO_NO_SECRET_STORE", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // the port it took, as it saves it, then an answer there
    loop {
        assert!(std::time::Instant::now() < end, "the daemon never answered");
        if let Some(port) = std::fs::read_to_string(d.join(".itero/port"))
            .ok()
            .and_then(|p| p.trim().parse::<u16>().ok())
            && std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
        {
            return daemon;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn emit_wait_answers_with_the_runs_end() {
    let d = workspace();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": [{ "#selamla": { "args": { "kim": "$event.kim" } } }] }"##,
    );
    file(
        d.path(),
        "workflows/kötü.json",
        r##"{ "on": [{ "event": "kişi.bozuk" }], "steps": ["#kır"] }"##,
    );
    file(
        d.path(),
        "workflows/bekle.json",
        r##"{ "on": [{ "event": "kişi.bekler" }], "steps": ["#bekle"] }"##,
    );
    file(
        d.path(),
        "actions/selamla",
        "#!/bin/sh\necho \"{\\\"selam\\\": \\\"merhaba $kim\\\"}\"\n",
    );
    file(
        d.path(),
        "actions/kır",
        "#!/bin/sh\necho çöktü >&2\nexit 3\n",
    );
    file(d.path(), "actions/bekle", "#!/bin/sh\nexit 75\n");
    // a workflow posts its own result: there is no --reply
    let o = itero(
        d.path(),
        &[
            "ws",
            "emit",
            "--reply",
            "http://örnek.test",
            "kişi.geldi",
            "{}",
        ],
    );
    assert!(!o.status.success() && err(&o).contains("--reply"), "{o:?}");
    // no daemon: nothing would answer, so it says so at once
    let o = itero(d.path(), &["ws", "emit", "--wait", "kişi.geldi", "{}"]);
    assert!(
        !o.status.success() && err(&o).contains("no daemon is running"),
        "{o:?}"
    );

    let _daemon = serving(d.path());
    let o = itero(
        d.path(),
        &["ws", "emit", "--wait", "kişi.geldi", r#"{"kim": "ayşe"}"#],
    );
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["message"].as_str().unwrap().len(), 36, "{answer}");
    let run = &answer["runs"][0];
    assert_eq!(run["workflow"], "selam");
    assert_eq!(run["status"], "done");
    assert_eq!(run["state"]["output"]["selam"], "merhaba ayşe");
    assert!(run["name"].as_str().unwrap().contains('-'));

    let o = itero(d.path(), &["ws", "emit", "--wait", "kişi.bozuk", "{}"]);
    assert_eq!(o.status.code(), Some(1), "an error is exit 1: {o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"][0]["status"], "error");

    let o = itero(
        d.path(),
        &[
            "ws",
            "emit",
            "--wait",
            "--timeout",
            "2s",
            "kişi.bekler",
            "{}",
        ],
    );
    assert_eq!(
        o.status.code(),
        Some(2),
        "past the timeout is exit 2: {o:?}"
    );
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    // the run so far: on a busy machine its step may not have said yet
    let status = &answer["runs"][0]["status"];
    assert!(status == "waiting" || status == "running", "{answer}");

    let o = itero(d.path(), &["ws", "emit", "--wait", "kimse.istemez", "{}"]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"], serde_json::json!([]));
}

#[test]
fn a_daemon_without_its_api_is_running_all_the_same() {
    let d = workspace();
    let state = d.path().join("durum");
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }"##,
    );
    file(
        d.path(),
        "actions/selamla",
        "#!/bin/sh\necho '\"merhaba\"'\n",
    );
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(args)
            .current_dir(d.path())
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("XDG_STATE_HOME", &state)
            .output()
            .unwrap()
    };
    let _daemon = stop::Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["ws", "daemon", "--no-serve"])
            .current_dir(d.path())
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("XDG_STATE_HOME", &state)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    // it holds the workspace's lock once it writes who it is in it
    let lock = d.path().join(".itero/daemon.lock");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::fs::read_to_string(&lock)
        .unwrap_or_default()
        .is_empty()
    {
        assert!(std::time::Instant::now() < end, "the daemon never started");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let o = run(&["ws", "emit", "kişi.geldi", "{}"]);
    assert!(o.status.success(), "{o:?}");
    assert!(!err(&o).contains("no daemon is running"), "{}", err(&o));
    let o = run(&["ws", "emit", "--wait", "kişi.geldi", "{}"]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"][0]["state"]["output"], "merhaba");
    let o = run(&["ws", "wf", "list", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["daemon"]["running"], true, "{v}");
}

#[test]
fn which_says_where_each_kind_of_one_name_is() {
    let d = workspace();
    file(
        d.path(),
        "workflows/dükkân/gönder.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/kargo/gönder", "#!/bin/sh\n");
    file(d.path(), "actions/kargo/eski/gönder@0.1", "#!/bin/sh\n");
    file(d.path(), "connectors/posta/posta.geldi", "#!/bin/sh\n");
    let found = out(&itero(d.path(), &["ws", "which", "gönder"]));
    assert_eq!(
        found,
        "workflow  workflows/dükkân/gönder.json\naction    actions/kargo/gönder\naction    actions/kargo/eski/gönder@0.1\n",
    );
    assert_eq!(
        out(&itero(d.path(), &["ws", "which", "#gönder"])),
        "action    actions/kargo/gönder\naction    actions/kargo/eski/gönder@0.1\n",
        "#name: the action only"
    );
    assert_eq!(
        out(&itero(d.path(), &["ws", "which", "posta.geldi"])),
        "connector connectors/posta/posta.geldi\n"
    );
    assert_eq!(
        out(&itero(d.path(), &["ws", "which", "notify"])),
        "action    built in\n"
    );
    assert_eq!(
        out(&itero(d.path(), &["ws", "which", "keep"])),
        "workflow  built in\n"
    );
    let o = itero(d.path(), &["ws", "which", "yok"]);
    assert!(
        !o.status.success() && err(&o).contains("nothing named yok"),
        "{o:?}"
    );
    let list = out(&itero(d.path(), &["ws", "wf", "list"]));
    let line = list.lines().find(|l| l.starts_with("gönder ")).unwrap();
    assert!(line.contains("workflows/dükkân/gönder.json"), "{list}");
    let keep = list.lines().find(|l| l.starts_with("keep ")).unwrap();
    assert!(keep.contains("built in"), "{list}");
}

#[test]
fn check_says_what_an_action_needs_that_is_not_there() {
    let d = workspace();
    file(
        d.path(),
        "workflows/dene.json",
        r##"{ "steps": ["#kabuk", "#dil", "#tamam"] }"##,
    );
    file(
        d.path(),
        "actions/kabuk",
        "#!/bin/sh\nyok-böyle-komut --bir | sort\necho \"$x\"\n",
    );
    file(d.path(), "actions/dil", "#!/usr/bin/env yok-böyle-dil\n");
    file(d.path(), "actions/tamam", "#!/bin/sh\nsort dosya\n");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(o.status.success(), "a hint, not an error: {o:?}");
    let text = out(&o);
    assert!(
        text.contains("may need yok-böyle-komut: not found on PATH"),
        "{text}"
    );
    assert!(
        text.contains("needs yok-böyle-dil: not found on PATH"),
        "{text}"
    );
    assert!(!text.contains("sort:"), "found: no line {text}");
    let tamam = text.split("#tamam").nth(1).unwrap();
    assert!(!tamam.contains("need"), "{text}");
}

#[test]
fn action_errors_say_which_actions_broke_runs_and_why() {
    let d = workspace();
    for (name, steps) in [
        ("sipariş", r##"["#öde", "#kargola"]"##),
        ("iade", r##"["#kargola"]"##),
    ] {
        file(
            d.path(),
            &format!("workflows/{name}.json"),
            &format!(r#"{{ "on": [{{ "event": "{name}.geldi" }}], "steps": {steps} }}"#),
        );
    }
    file(d.path(), "actions/öde", "#!/bin/sh\n");
    file(
        d.path(),
        "actions/kargola",
        "#!/bin/sh\necho 'kargo kapalı' >&2\nexit 3\n",
    );
    for e in ["sipariş.geldi", "iade.geldi"] {
        assert!(itero(d.path(), &["ws", "emit", e, "{}"]).status.success());
    }
    assert!(
        itero(d.path(), &["ws", "daemon", "--once"])
            .status
            .success()
    );
    // each action that broke a run, how often, and why last
    let all = out(&itero(d.path(), &["ws", "ac", "errors"]));
    assert_eq!(all.lines().count(), 1, "{all}");
    assert!(
        all.contains("#kargola") && all.contains("2 errors") && all.contains("kargo kapalı"),
        "{all}"
    );
    // one action's failures, each with its run
    let v: serde_json::Value = serde_json::from_str(&out(&itero(
        d.path(),
        &["ws", "ac", "errors", "kargola", "--json"],
    )))
    .unwrap();
    let mut workflows: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["workflow"].as_str().unwrap())
        .collect();
    workflows.sort();
    assert_eq!(workflows, ["iade", "sipariş"]);
    assert!(v[0]["why"].as_str().unwrap().contains("exit 3"), "{v}");
    // an action that never broke anything
    let none = out(&itero(d.path(), &["ws", "ac", "errors", "öde"]));
    assert!(none.contains("no run ended in error"), "{none}");
    // the same runs, from the workflow side
    let runs = out(&itero(
        d.path(),
        &["ws", "wf", "runs", "--action", "kargola"],
    ));
    assert_eq!(runs.lines().count(), 2, "{runs}");
    let one = out(&itero(
        d.path(),
        &[
            "ws",
            "wf",
            "runs",
            "--action",
            "kargola",
            "--workflow",
            "iade",
        ],
    ));
    assert_eq!(one.lines().count(), 1, "{one}");
}

#[test]
fn commands_that_print_also_print_json() {
    let d = workspace();
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": [{ "#selamla": { "args": { "kim": "$event.kim" } } }] }"##,
    );
    file(d.path(), "actions/selamla", "#!/bin/sh\n");
    itero(d.path(), &["ws", "emit", "kişi.geldi", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let json = |args: &[&str]| -> serde_json::Value {
        let o = itero(d.path(), args);
        assert!(o.status.success(), "{args:?}: {o:?}");
        serde_json::from_str(&out(&o)).unwrap_or_else(|e| panic!("{args:?}: {e}"))
    };
    let list = json(&["ws", "wf", "list", "--json"]);
    assert!(
        list["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["name"] == "selam" && w["runs"] == 1),
        "{list}"
    );
    let runs = json(&["ws", "wf", "runs", "--json"]);
    assert_eq!(runs[0]["workflow"], "selam");
    assert_eq!(runs[0]["status"], "done");
    let story = json(&[
        "ws",
        "wf",
        "runs",
        runs[0]["name"].as_str().unwrap(),
        "--json",
    ]);
    assert_eq!(story["status"], "done");
    assert!(story["steps"].as_array().unwrap().len() >= 2, "{story}");
    let actions = json(&["ws", "ac", "list", "--json"]);
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "selamla" && a["built_in"] == false),
        "{actions}"
    );
    assert!(json(&["ws", "info", "--json"])["workspace"]["id"].is_string());
    assert_eq!(
        json(&["ws", "which", "selam", "--json"])[0]["kind"],
        "workflow"
    );
    assert_eq!(
        json(&["ws", "check", "--json"])["workflows"][0]["name"],
        "selam"
    );
    assert!(json(&["ws", "emit", "kişi.geldi", "{}", "--json"])["id"].is_string());
}

#[test]
fn completion_prints_a_script_for_each_shell() {
    let d = workspace();
    for shell in ["bash", "zsh", "fish"] {
        let o = itero(d.path(), &["completion", shell]);
        assert!(o.status.success(), "{shell}: {o:?}");
        assert!(out(&o).contains("itero"), "{shell}: {}", out(&o));
    }
}

/// What the shell offers for the last of `words` (`""`: nothing typed
/// yet), asked as fish asks: `COMPLETE=fish itero -- itero <words>`.
fn completions(dir: &Path, words: &[&str]) -> Vec<String> {
    completions_on(dir, &dir.join("durum"), words)
}

/// What the shell's Tab offers, with the machine's state in `state`.
fn completions_on(dir: &Path, state: &Path, words: &[&str]) -> Vec<String> {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["--", "itero"])
        .args(words)
        .current_dir(dir)
        .env("COMPLETE", "fish")
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("XDG_STATE_HOME", state)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    out(&o)
        .lines()
        .map(|l| l.split('\t').next().unwrap().to_string())
        .filter(|w| !w.starts_with('-'))
        .collect()
}

#[test]
fn tab_completes_the_workspaces_own_names() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\necho '{}'\n");
    file(d.path(), "actions/öde@2", "#!/bin/sh\n");
    file(d.path(), "connectors/posta.geldi", "#!/bin/sh\n");
    // the workspace's workflows, beside itero's own
    let workflows = completions(d.path(), &["ws", "wf", "runs", "--workflow", ""]);
    assert!(workflows.contains(&"sipariş".to_string()), "{workflows:?}");
    // an action's version is not part of its name; the built-ins are there too
    let actions = completions(d.path(), &["ws", "wf", "runs", "--action", ""]);
    assert!(actions.contains(&"gönder".to_string()), "{actions:?}");
    assert!(actions.contains(&"öde".to_string()), "{actions:?}");
    assert!(actions.contains(&"notify".to_string()), "{actions:?}");
    assert!(!actions.iter().any(|a| a.contains('@')), "{actions:?}");
    // no runs yet, then the run's name
    assert!(completions(d.path(), &["ws", "wf", "runs", ""]).is_empty());
    itero(d.path(), &["ws", "emit", "sipariş.geldi", "{}"]);
    itero(d.path(), &["ws", "daemon", "--once"]);
    let list = out(&itero(d.path(), &["ws", "wf", "runs"]));
    let name = list.split_whitespace().nth(1).unwrap().to_string();
    assert_eq!(completions(d.path(), &["ws", "wf", "runs", ""]), [name]);
    // outside a workspace: no names, and no error
    let outside = tempfile::tempdir().unwrap();
    assert!(completions(outside.path(), &["ws", "wf", "runs", "--workflow", ""]).is_empty());
    assert!(completions(outside.path(), &["ws", "wf", "runs", ""]).is_empty());
}

#[test]
fn tab_completes_the_names_of_the_workspace_ws_names() {
    let state = tempfile::tempdir().unwrap();
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "sipariş.geldi" }], "steps": ["#notify"] }"##,
    );
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    let name = listed(state.path()).remove(0);
    let elsewhere = tempfile::tempdir().unwrap();
    let tab = |words: &[&str]| completions_on(elsewhere.path(), state.path(), words);
    // the names on the list, beside the subcommands, right after ws
    let first = tab(&["ws", ""]);
    assert!(
        first.contains(&name) && first.contains(&"wf".to_string()),
        "{first:?}"
    );
    let flows = tab(&["ws", &name, "wf", "runs", "--workflow", ""]);
    assert!(flows.contains(&"sipariş".to_string()), "{flows:?}");
    // by its path too
    let path = d.path().display().to_string();
    let flows = tab(&["ws", &path, "wf", "runs", "--workflow", ""]);
    assert!(flows.contains(&"sipariş".to_string()), "{flows:?}");
    // the folder's when none is named, and none for a name not listed
    assert!(tab(&["ws", "wf", "runs", "--workflow", ""]).is_empty());
    assert!(tab(&["ws", "yok-böyle", "wf", "runs", "--workflow", ""]).is_empty());
}

#[test]
fn check_says_a_steps_values_go_under_args() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kargo.json",
        r##"{ "on": [{ "event": "kargo.hazır" }], "steps": [{ "#gönder": { "firma": "PTT" } }] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(!o.status.success(), "{o:?}");
    let said = format!("{}{}", out(&o), err(&o));
    assert!(
        said.contains("\"firma\"") && said.contains("go under args"),
        "{said}"
    );
}

#[test]
fn check_refuses_a_secret_inside_text() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kargo.json",
        r##"{ "on": [{ "event": "kargo.hazır" }],
             "steps": [{ "#gönder": { "args": { "başlık": "Bearer $$POSTA_ANAHTARI" } } }] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let o = itero(d.path(), &["ws", "check"]);
    assert!(!o.status.success(), "{o:?}");
    let said = format!("{}{}", out(&o), err(&o));
    assert!(said.contains("a secret is a whole value only"), "{said}");
}

#[test]
fn check_warns_of_an_arg_notify_does_not_take() {
    let d = workspace();
    file(
        d.path(),
        "workflows/fiyat.json",
        r##"{"on": [{"event": "fiyat.soruldu"}], "steps": [{"#notify": {"args": [{"title": "Fiyat", "text": "$output"}]}}]}"##,
    );
    let o = itero(d.path(), &["ws", "check"]);
    assert!(o.status.success(), "{}", err(&o));
    let e = err(&o);
    assert!(
        e.contains("warning: #notify takes title, text, subtitle, sound, url, buttons, wait and data, not Fiyat (")
            && e.contains("/workflows/fiyat/steps/#notify)"),
        "{e}"
    );
    // the args it takes give no warning
    file(
        d.path(),
        "workflows/fiyat.json",
        r##"{"on": [{"event": "fiyat.soruldu"}], "steps": [{"#notify": {"args": {"title": "Fiyat", "text": "$output"}}}]}"##,
    );
    assert!(!err(&itero(d.path(), &["ws", "check"])).contains("#notify takes"));
}

#[cfg(target_os = "macos")]
#[test]
fn info_says_whether_itero_may_notify() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
    let fake = |name: &str, body: &str| {
        let p = d.path().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let info = |notifier: &std::path::Path, json: bool| {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_itero"));
        c.args(["ws", "info"]).current_dir(d.path());
        if json {
            c.arg("--json");
        }
        let o = c
            .env("ITERO_NOTIFIER", notifier)
            .env("ITERO_NO_SECRET_STORE", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let allowed = fake("izinli", r#"[ "$1" = --status ] && echo allowed"#);
    // itero gives a notifier three seconds, which a busy machine can use up
    // in starting one; what is checked is what it says once it answers
    let answered = |json: bool| {
        (0..10)
            .map(|_| info(&allowed, json))
            .find(|said| !said.contains("no answer in three seconds"))
            .expect("the notifier never answered")
    };
    assert!(answered(false).contains("notifications: allowed"));
    let v: serde_json::Value = serde_json::from_str(&answered(true)).unwrap();
    assert_eq!(v["notifications"]["state"], "allowed", "{v}");
    // a notifier that does not answer: unknown, and info does not wait for it
    let slow = fake("yavaş", "sleep 30");
    let t = std::time::Instant::now();
    assert!(info(&slow, false).contains("notifications: unknown"));
    assert!(
        t.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn help_shows_each_alias_after_its_command() {
    let d = workspace();
    let top = String::from_utf8(itero(d.path(), &["--help"]).stdout).unwrap();
    let line = top
        .lines()
        .find(|l| l.trim_start().starts_with("workspace (ws)"))
        .unwrap_or_else(|| panic!("{top}"));
    assert!(line.contains("[aliases: ws]"), "{line}");
    let ws = String::from_utf8(itero(d.path(), &["ws", "--help"]).stdout).unwrap();
    for (name, alias) in [("workflow", "wf"), ("action", "ac"), ("connector", "cn")] {
        let line = ws
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{name} ({alias})")))
            .unwrap_or_else(|| panic!("{name}: {ws}"));
        assert!(line.contains(&format!("[aliases: {alias}]")), "{line}");
    }
}

/// itero with its machine state in `state`: never the real one.
fn itero_on(dir: &Path, state: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("XDG_STATE_HOME", state)
        .output()
        .unwrap()
}

/// The names on the machine's list in `state`, in its order.
fn listed(state: &Path) -> Vec<String> {
    let list = std::fs::read_to_string(state.join("itero/workspaces.json")).unwrap();
    let list: serde_json::Value = serde_json::from_str(&list).unwrap();
    list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_ws_command_picks_a_workspace_by_name_from_any_folder() {
    let state = tempfile::tempdir().unwrap();
    let d = workspace();
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "selam.ver" }], "steps": ["#notify"] }"##,
    );
    assert!(
        itero_on(d.path(), state.path(), &["ws", "add"])
            .status
            .success()
    );
    let name = listed(state.path()).remove(0);
    let elsewhere = tempfile::tempdir().unwrap();
    let o = itero_on(elsewhere.path(), state.path(), &["ws", &name, "wf", "list"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("selam"), "{}", out(&o));
    let o = itero_on(elsewhere.path(), state.path(), &["ws", &name, "ac", "list"]);
    assert!(o.status.success(), "{}", err(&o));
    let o = itero_on(
        elsewhere.path(),
        state.path(),
        &["ws", "yok-böyle", "wf", "list"],
    );
    assert!(
        !o.status.success() && err(&o).contains("itero ws list"),
        "{}",
        err(&o)
    );
    // by its path, absolute or relative, listed or not
    let path = d.path().display().to_string();
    let o = itero_on(elsewhere.path(), state.path(), &["ws", &path, "wf", "list"]);
    assert!(out(&o).contains("selam"), "{}", err(&o));
    let o = itero_on(d.path(), state.path(), &["ws", ".", "wf", "list"]);
    assert!(out(&o).contains("selam"), "{}", err(&o));
    let parent = d.path().parent().unwrap();
    let rel = format!("./{}", d.path().file_name().unwrap().to_str().unwrap());
    let o = itero_on(parent, state.path(), &["ws", &rel, "wf", "list"]);
    assert!(out(&o).contains("selam"), "{}", err(&o));
    // none named, outside any workspace: the list's names are said
    let o = itero_on(elsewhere.path(), state.path(), &["ws", "wf", "list"]);
    assert!(
        !o.status.success() && err(&o).contains("which workspace") && err(&o).contains(&name),
        "{}",
        err(&o)
    );
}

#[test]
fn the_lists_own_commands_take_no_workspace_before_them() {
    let state = tempfile::tempdir().unwrap();
    let d = workspace();
    for args in [
        &["ws", "güneş-ay", "add"][..],
        &["ws", "güneş-ay", "list"],
        &["ws", "güneş-ay", "remove"],
    ] {
        let o = itero_on(d.path(), state.path(), args);
        assert!(
            !o.status.success() && err(&o).contains("takes no workspace before it"),
            "{args:?}: {}",
            err(&o)
        );
    }
    // remove takes the name itself, from any folder
    let o = itero_on(d.path(), state.path(), &["ws", "güneş-ay", "remove"]);
    assert!(err(&o).contains("itero ws remove güneş-ay"), "{}", err(&o));
}

#[test]
fn a_command_that_moved_names_its_new_form() {
    let d = workspace();
    for (old, new) in [
        (vec!["wf", "runs"], "itero ws wf runs"),
        (vec!["workflow", "list"], "itero ws wf list"),
        (vec!["action", "errors"], "itero ws ac errors"),
        (vec!["connector", "list"], "itero ws cn list"),
        (vec!["check", "--json"], "itero ws check --json"),
        (vec!["which", "x"], "itero ws which x"),
    ] {
        let o = itero(d.path(), &old);
        assert!(
            !o.status.success() && err(&o).contains(new),
            "{old:?}: {}",
            err(&o)
        );
    }
    let help = out(&itero(d.path(), &["--help"]));
    assert!(!help.contains("Moved"), "{help}");
}

fn itero_with_stdin(dir: &Path, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn events_waiting(d: &Path) -> Vec<serde_json::Value> {
    serde_json::from_str(&out(&itero(d, &["ws", "ev", "list", "--json"]))).unwrap()
}

#[test]
fn emit_takes_its_data_from_a_file_or_stdin() {
    let d = workspace();
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "order.placed" }, { "$event.şehir": "İzmir" }], "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    file(d.path(), "istek.json", "{\"şehir\": \"İzmir\"}\n");
    let o = itero(
        d.path(),
        &["ws", "emit", "--file", "istek.json", "order.placed"],
    );
    assert!(o.status.success(), "{o:?}");
    // what it starts is read from the file's data too
    assert!(err(&o).contains("starts: sipariş"), "{}", err(&o));
    let o = itero_with_stdin(
        d.path(),
        &["ws", "emit", "order.placed", "--file", "-"],
        r#"{"şehir": "Muğla", "adet": 3}"#,
    );
    assert!(o.status.success(), "{o:?}");
    let events = events_waiting(d.path());
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0]["data"], serde_json::json!({"şehir": "İzmir"}));
    assert_eq!(
        events[1]["data"],
        serde_json::json!({"şehir": "Muğla", "adet": 3})
    );
}

#[test]
fn emit_refuses_a_file_it_cannot_use_and_sends_nothing() {
    let d = workspace();
    file(d.path(), "bozuk.json", "{\"şehir\": ");
    file(d.path(), "liste.json", "[1, 2]");
    file(d.path(), "istek.json", "{}");
    for (args, stdin, said) in [
        (
            &["ws", "emit", "a.b", "--file", "yok.json"][..],
            "",
            &["yok.json"][..],
        ),
        (
            &["ws", "emit", "a.b", "--file", "bozuk.json"],
            "",
            &["bozuk.json", "not JSON", "line 1"],
        ),
        (
            &["ws", "emit", "a.b", "--file", "liste.json"],
            "",
            &["liste.json", "a JSON object"],
        ),
        (
            &["ws", "emit", "a.b", "--file", "-"],
            "ılık",
            &["stdin", "not JSON"],
        ),
        (
            &["ws", "emit", "a.b", "--file", "-"],
            "",
            &["stdin", "not JSON"],
        ),
        (
            &["ws", "emit", "a.b", "{}", "--file", "istek.json"],
            "",
            &["cannot be used with"],
        ),
    ] {
        let o = itero_with_stdin(d.path(), args, stdin);
        assert!(!o.status.success(), "{args:?}: {o:?}");
        for word in said {
            assert!(err(&o).contains(word), "{args:?}: {word}: {}", err(&o));
        }
    }
    assert!(events_waiting(d.path()).is_empty());
}
