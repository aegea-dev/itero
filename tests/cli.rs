//! The command line: the commands by kind, their args and settings, secrets.
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

/// The workspace's manifest, read.
fn manifest(d: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(d.join("itero.config.json")).unwrap()).unwrap()
}

#[test]
fn args_are_written_for_the_action() {
    let d = workspace();
    file(
        d.path(),
        "itero.config.json",
        r#"{"name": "ev", "port": 8787, "action": {"kargola": {"description": "Kargoya verir"}}}"#,
    );
    file(d.path(), "actions/kargola", "#!/bin/sh\n");
    file(d.path(), "actions/posta-bak", "#!/bin/sh\n");
    let o = itero(
        d.path(),
        &[
            "action",
            "args",
            "set",
            "kargola",
            r#"{"firma": "PTT Kargo"}"#,
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let m = manifest(d.path());
    assert_eq!(m["action"]["kargola"]["args"]["firma"], "PTT Kargo");
    assert_eq!(m["action"]["kargola"]["description"], "Kargoya verir");
    // the rest is kept as it was, in its order
    let keys: Vec<&String> = m.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["name", "port", "action"]);
    // from a file, a list of objects: one set of values, read in order
    let list = d.path().join("hesap.json");
    std::fs::write(
        &list,
        r#"[{"host": "imap.örnek.test"}, {"user": "can@örnek.test"}]"#,
    )
    .unwrap();
    let o = itero(
        d.path(),
        &[
            "action",
            "args",
            "set",
            "posta-bak",
            "--file",
            list.to_str().unwrap(),
        ],
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        manifest(d.path())["action"]["posta-bak"]["args"][1]["user"],
        "can@örnek.test"
    );
    // a name given twice is refused: once per item is repeat's
    std::fs::write(
        &list,
        r#"[{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]"#,
    )
    .unwrap();
    let o = itero(
        d.path(),
        &[
            "action",
            "args",
            "set",
            "posta-bak",
            "--file",
            list.to_str().unwrap(),
        ],
    );
    assert!(
        !o.status.success() && err(&o).contains("user is given twice"),
        "{o:?}"
    );
    // shown, and as JSON
    let shown = out(&itero(d.path(), &["action", "args", "get", "kargola"]));
    assert!(shown.contains("PTT Kargo"), "{shown}");
    let v: serde_json::Value = serde_json::from_str(&out(&itero(
        d.path(),
        &["action", "args", "get", "kargola", "--json"],
    )))
    .unwrap();
    assert_eq!(v["firma"], "PTT Kargo");
    // an action that is not there has no args to set
    let o = itero(d.path(), &["action", "args", "set", "yok", "{}"]);
    assert!(
        !o.status.success() && err(&o).contains("no action yok"),
        "{o:?}"
    );
}

#[test]
fn args_that_are_not_an_object_or_a_list_of_objects_are_refused() {
    let d = workspace();
    file(d.path(), "actions/kargola", "#!/bin/sh\n");
    for bad in ["[1, 2]", r#""çay""#, "{", "42"] {
        let o = itero(d.path(), &["action", "args", "set", "kargola", bad]);
        assert!(!o.status.success(), "{bad}");
        assert!(
            err(&o).contains("an object, or a list of objects"),
            "{bad}: {}",
            err(&o)
        );
    }
    assert!(!d.path().join("args/kargola.json").exists());
    let o = itero(d.path(), &["action", "args", "set", "../dışarı", "{}"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("../dışarı"), "{}", err(&o));
}

#[test]
#[cfg(unix)]
fn args_are_written_for_the_connector_and_checked() {
    let d = workspace();
    file(d.path(), "connectors/posta.geldi", "#!/bin/sh\n");
    let o = itero(
        d.path(),
        &[
            "connector",
            "args",
            "set",
            "posta.geldi",
            r#"{"user": "ayşe@örnek.test"}"#,
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let m = manifest(d.path());
    assert_eq!(
        m["connector"]["posta.geldi"]["args"]["user"],
        "ayşe@örnek.test"
    );
    assert!(m.get("action").is_none(), "{m}");
    assert!(itero(d.path(), &["check"]).status.success());
    // a connector runs once: no list
    let o = itero(
        d.path(),
        &[
            "connector",
            "args",
            "set",
            "posta.geldi",
            r#"[{"user": "can@örnek.test"}]"#,
        ],
    );
    assert!(!o.status.success());
    assert!(
        err(&o).contains("a connector's args are an object"),
        "{}",
        err(&o)
    );
    file(
        d.path(),
        "itero.config.json",
        r#"{"connector": {"posta.geldi": {"args": [{"user": "can@örnek.test"}]}}}"#,
    );
    let o = itero(d.path(), &["check"]);
    assert!(
        !o.status.success() && err(&o).contains("connector.posta.geldi.args"),
        "{o:?}"
    );
    // listed, as JSON too
    let v: serde_json::Value =
        serde_json::from_str(&out(&itero(d.path(), &["connector", "list", "--json"]))).unwrap();
    assert_eq!(v[0]["type"], "posta.geldi");
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
fn config_is_written_shown_and_checked_for_the_workflow() {
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
    let o = itero(
        d.path(),
        &[
            "wf",
            "config",
            "set",
            "gözcü",
            r#"{"siteler": ["https://örnek.test"]}"#,
        ],
    );
    assert!(o.status.success(), "{o:?}");
    let m = manifest(d.path());
    assert_eq!(
        m["workflow"]["gözcü"]["settings"]["siteler"][0],
        "https://örnek.test"
    );
    assert_eq!(m["workflow"]["gözcü"]["description"], "Siteler");
    let shown = out(&itero(d.path(), &["workflow", "config", "get", "gözcü"]));
    assert!(shown.contains("örnek.test"), "{shown}");
    for (name, data, says) in [
        ("../dışarı", "{}", "not a workflow's name"),
        ("gözcü", "[1]", "an object"),
    ] {
        let o = itero(d.path(), &["wf", "config", "set", name, data]);
        assert!(
            !o.status.success() && err(&o).contains(says),
            "{data}: {o:?}"
        );
    }
    assert!(itero(d.path(), &["check"]).status.success());
    // check: an entry naming nothing, and folders from before, are refused
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"gözcü": {}, "yok-böyle": {}}, "action": {"hayalet": {}}}"#,
    );
    let o = itero(d.path(), &["check"]);
    assert!(
        !o.status.success()
            && err(&o).contains("workflow.yok-böyle")
            && err(&o).contains("action.hayalet"),
        "{o:?}"
    );
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), "args/bak.json", "{}");
    let o = itero(d.path(), &["check"]);
    assert!(
        !o.status.success() && err(&o).contains("args") && err(&o).contains("itero.config.json"),
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
    let o = itero(d.path(), &["check"]);
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
    let o = itero(d.path(), &["check"]);
    assert!(!o.status.success());
    let e = err(&o);
    assert!(
        e.contains("action.depo.limits: runs: an action's limits are quiet and most"),
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
        itero(d.path(), &["emit", "a.b", r#"{"kim": "ayşe"}"#])
            .status
            .success()
    );
    let o = itero(d.path(), &["daemon", "--once"]);
    assert!(o.status.success(), "{o:?}");
    let runs = out(&itero(d.path(), &["wf", "runs"]));
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
    itero(d.path(), &["emit", "a.b", "{}"]);
    itero(d.path(), &["daemon", "--once"]);
    assert!(!out(&itero(d.path(), &["wf", "runs"])).contains("bakım"));
    assert!(out(&itero(d.path(), &["wf", "runs", "--all"])).contains("bakım"));
    let errors = out(&itero(d.path(), &["wf", "runs", "--errors"]));
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
        "#!/bin/sh\ncase \"$output\" in *'\"tamam\":true'*) exit 0 ;; esac\necho '{\"ödeme\": \"bekliyor\", \"sipariş\": 1}'; exit 75\n",
    );
    file(
        d.path(),
        "actions/gönder",
        "#!/bin/sh\necho '{\"kargo\": \"TR-ü1\"}'\n",
    );
    itero(d.path(), &["emit", "sipariş.geldi", r#"{"sipariş": 1}"#]);
    itero(d.path(), &["daemon", "--once"]);
    let list = out(&itero(d.path(), &["wf", "runs"]));
    let name = list.split_whitespace().nth(1).unwrap().to_string();
    let waiting = out(&itero(d.path(), &["wf", "runs", &name]));
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
        &["emit", "ödeme.geldi", r#"{"sipariş": 1, "tamam": true}"#],
    );
    itero(d.path(), &["daemon", "--once"]);
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["wf", "runs", id]));
    for says in ["done", "ödeme.geldi", "#gönder", "TR-ü1", "state"] {
        assert!(story.contains(says), "{says}: {story}");
    }
    let o = itero(d.path(), &["wf", "runs", "yok-böyle"]);
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
    itero(d.path(), &["emit", "yarış.başladı", "{}"]);
    itero(d.path(), &["daemon", "--once"]);
    let list = out(&itero(d.path(), &["wf", "runs"]));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["wf", "runs", id]));
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
    file(d.path(), "actions/su", "#!/bin/sh\nsleep 30\n");
    file(
        d.path(),
        "actions/hızlı",
        "#!/bin/sh\nsleep 1; echo hızlı\n",
    );
    itero(d.path(), &["emit", "yarış.başladı", "{}"]);
    itero(d.path(), &["daemon", "--once"]);
    let list = out(&itero(d.path(), &["wf", "runs"]));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["wf", "runs", id]));
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
fn a_step_that_breaks_undoes_those_before_it_and_the_story_says_so() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kurulum.json",
        r##"{ "on": [{ "event": "küme.istendi" }],
              "steps": [{ "#kur": { "undo": "#sil" } }, "#dene"],
              "error": ["#bildir"] }"##,
    );
    file(d.path(), "actions/kur", "#!/bin/sh\necho kur >> sıra.txt\n");
    file(
        d.path(),
        "actions/dene",
        "#!/bin/sh\necho 'yanıt yok' >&2; exit 1\n",
    );
    file(
        d.path(),
        "actions/sil",
        "#!/bin/sh\necho \"sil $failed_step\" >> sıra.txt\n",
    );
    file(
        d.path(),
        "actions/bildir",
        "#!/bin/sh\necho bildir >> sıra.txt\n",
    );
    itero(d.path(), &["emit", "küme.istendi", r#"{"ad": "deneme"}"#]);
    itero(d.path(), &["daemon", "--once"]);
    let order = std::fs::read_to_string(d.path().join("sıra.txt")).unwrap();
    assert_eq!(order, "kur\nsil #dene\nbildir\n");
    let list = out(&itero(d.path(), &["wf", "runs"]));
    let id = list.split_whitespace().next().unwrap();
    let story = out(&itero(d.path(), &["wf", "runs", id]));
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
    let info = out(&itero(d.path(), &["info"]));
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
    let check = out(&itero(d.path(), &["check"]));
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
    itero(d.path(), &["emit", "order.placed", r#"{"sipariş": 1}"#]);
    let before = out(&itero(d.path(), &["wf", "list"]));
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
    itero(d.path(), &["daemon", "--once"]);
    let after = out(&itero(d.path(), &["wf", "list"]));
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
    let o = itero(d.path(), &["emit", "order.placed", r#"{"sipariş": 1}"#]);
    assert!(o.status.success(), "{o:?}");
    assert_eq!(out(&o).trim().len(), 36, "the message's id on stdout");
    let said = err(&o);
    assert!(said.contains("starts: kayıt, sipariş"), "{said}");
    assert!(
        said.contains("no daemon is running: it waits for itero daemon"),
        "{said}"
    );
    let o = itero(d.path(), &["emit", "order.places", "{}"]);
    assert!(o.status.success(), "{o:?}");
    let said = err(&o);
    assert!(
        said.contains("nothing starts on order.places: did you mean order.placed?"),
        "{said}"
    );
    let o = itero(d.path(), &["emit", "çok.başka.bir.şey", "{}"]);
    assert!(!err(&o).contains("did you mean"), "{}", err(&o));
    // every one is saved all the same
    let ws = itero::workspace::find(d.path()).unwrap();
    let j = itero::engine::journal::Journal::open(&ws).unwrap();
    assert_eq!(j.events(Some("itero.app")).unwrap().len(), 3);
}

/// A daemon serving the workspace in `d`, killed however the test ends.
struct Daemon(std::process::Child);

impl Daemon {
    fn start(d: &Path, port: u16) -> Daemon {
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(d)
            .env("ITERO_NO_SECRET_STORE", "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // ready once it answers on its port
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(std::time::Instant::now() < end, "the daemon never answered");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Daemon(child)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn emit_wait_answers_with_the_runs_end() {
    let d = workspace();
    file(d.path(), "itero.config.json", r#"{"port": 8802}"#);
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }"##,
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
        &["emit", "--reply", "http://örnek.test", "kişi.geldi", "{}"],
    );
    assert!(!o.status.success() && err(&o).contains("--reply"), "{o:?}");
    // no daemon: nothing would answer, so it says so at once
    let o = itero(d.path(), &["emit", "--wait", "kişi.geldi", "{}"]);
    assert!(
        !o.status.success() && err(&o).contains("no daemon is running"),
        "{o:?}"
    );

    let _daemon = Daemon::start(d.path(), 8802);
    let o = itero(
        d.path(),
        &["emit", "--wait", "kişi.geldi", r#"{"kim": "ayşe"}"#],
    );
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["message"].as_str().unwrap().len(), 36, "{answer}");
    let run = &answer["runs"][0];
    assert_eq!(run["workflow"], "selam");
    assert_eq!(run["status"], "done");
    assert_eq!(run["state"]["output"]["selam"], "merhaba ayşe");
    assert!(run["name"].as_str().unwrap().contains('-'));

    let o = itero(d.path(), &["emit", "--wait", "kişi.bozuk", "{}"]);
    assert_eq!(o.status.code(), Some(1), "an error is exit 1: {o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"][0]["status"], "error");

    let o = itero(
        d.path(),
        &["emit", "--wait", "--timeout", "2s", "kişi.bekler", "{}"],
    );
    assert_eq!(
        o.status.code(),
        Some(2),
        "past the timeout is exit 2: {o:?}"
    );
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"][0]["status"], "waiting");

    let o = itero(d.path(), &["emit", "--wait", "kimse.istemez", "{}"]);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let answer: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(answer["runs"], serde_json::json!([]));
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
    let found = out(&itero(d.path(), &["which", "gönder"]));
    assert_eq!(
        found,
        "workflow  workflows/dükkân/gönder.json\naction    actions/kargo/gönder\naction    actions/kargo/eski/gönder@0.1\n",
    );
    assert_eq!(
        out(&itero(d.path(), &["which", "#gönder"])),
        "action    actions/kargo/gönder\naction    actions/kargo/eski/gönder@0.1\n",
        "#name: the action only"
    );
    assert_eq!(
        out(&itero(d.path(), &["which", "posta.geldi"])),
        "connector connectors/posta/posta.geldi\n"
    );
    assert_eq!(
        out(&itero(d.path(), &["which", "notify"])),
        "action    built in\n"
    );
    assert_eq!(
        out(&itero(d.path(), &["which", "keep"])),
        "workflow  built in\n"
    );
    let o = itero(d.path(), &["which", "yok"]);
    assert!(
        !o.status.success() && err(&o).contains("nothing named yok"),
        "{o:?}"
    );
    let list = out(&itero(d.path(), &["wf", "list"]));
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
    let o = itero(d.path(), &["check"]);
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
        assert!(itero(d.path(), &["emit", e, "{}"]).status.success());
    }
    assert!(itero(d.path(), &["daemon", "--once"]).status.success());
    // each action that broke a run, how often, and why last
    let all = out(&itero(d.path(), &["action", "errors"]));
    assert_eq!(all.lines().count(), 1, "{all}");
    assert!(
        all.contains("#kargola") && all.contains("2 errors") && all.contains("kargo kapalı"),
        "{all}"
    );
    // one action's failures, each with its run
    let v: serde_json::Value = serde_json::from_str(&out(&itero(
        d.path(),
        &["action", "errors", "kargola", "--json"],
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
    let none = out(&itero(d.path(), &["action", "errors", "öde"]));
    assert!(none.contains("no run ended in error"), "{none}");
    // the same runs, from the workflow side
    let runs = out(&itero(d.path(), &["wf", "runs", "--action", "kargola"]));
    assert_eq!(runs.lines().count(), 2, "{runs}");
    let one = out(&itero(
        d.path(),
        &["wf", "runs", "--action", "kargola", "--workflow", "iade"],
    ));
    assert_eq!(one.lines().count(), 1, "{one}");
}

#[test]
fn commands_that_print_also_print_json() {
    let d = workspace();
    file(
        d.path(),
        "workflows/selam.json",
        r##"{ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }"##,
    );
    file(d.path(), "actions/selamla", "#!/bin/sh\n");
    itero(d.path(), &["emit", "kişi.geldi", "{}"]);
    itero(d.path(), &["daemon", "--once"]);
    let json = |args: &[&str]| -> serde_json::Value {
        let o = itero(d.path(), args);
        assert!(o.status.success(), "{args:?}: {o:?}");
        serde_json::from_str(&out(&o)).unwrap_or_else(|e| panic!("{args:?}: {e}"))
    };
    let list = json(&["wf", "list", "--json"]);
    assert!(
        list["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["name"] == "selam" && w["runs"] == 1),
        "{list}"
    );
    let runs = json(&["wf", "runs", "--json"]);
    assert_eq!(runs[0]["workflow"], "selam");
    assert_eq!(runs[0]["status"], "done");
    let story = json(&["wf", "runs", runs[0]["name"].as_str().unwrap(), "--json"]);
    assert_eq!(story["status"], "done");
    assert!(story["steps"].as_array().unwrap().len() >= 2, "{story}");
    let actions = json(&["action", "list", "--json"]);
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "selamla" && a["built_in"] == false),
        "{actions}"
    );
    assert!(json(&["info", "--json"])["workspace"]["id"].is_string());
    assert_eq!(json(&["which", "selam", "--json"])[0]["kind"], "workflow");
    assert_eq!(json(&["check", "--json"])["workflows"][0]["name"], "selam");
    assert!(json(&["wf", "config", "get", "selam", "--json"]).is_null());
    assert!(json(&["emit", "kişi.geldi", "{}", "--json"])["id"].is_string());
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

#[test]
fn check_says_a_steps_values_go_under_args() {
    let d = workspace();
    file(
        d.path(),
        "workflows/kargo.json",
        r##"{ "on": [{ "event": "kargo.hazır" }], "steps": [{ "#gönder": { "firma": "PTT" } }] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let o = itero(d.path(), &["check"]);
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
        "itero.config.json",
        r#"{"action": {"gönder": {"args": {"başlık": "Bearer $$POSTA_ANAHTARI"}}}}"#,
    );
    file(
        d.path(),
        "workflows/kargo.json",
        r##"{ "on": [{ "event": "kargo.hazır" }], "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let o = itero(d.path(), &["check"]);
    assert!(!o.status.success(), "{o:?}");
    let said = format!("{}{}", out(&o), err(&o));
    assert!(
        said.contains("action.gönder.args") && said.contains("a secret is a whole value only"),
        "{said}"
    );
}

#[test]
fn check_warns_of_an_arg_notify_does_not_take() {
    let d = workspace();
    file(
        d.path(),
        "workflows/fiyat.json",
        r##"{"on": [{"event": "fiyat.soruldu"}], "steps": [{"#notify": {"args": [{"title": "Fiyat", "text": "$output"}]}}]}"##,
    );
    let o = itero(d.path(), &["check"]);
    assert!(o.status.success(), "{}", err(&o));
    let e = err(&o);
    assert!(
        e.contains("warning: fiyat: #notify takes title, text, subtitle, sound, url, buttons, wait and data, not Fiyat"),
        "{e}"
    );
    // the args it takes give no warning
    file(
        d.path(),
        "workflows/fiyat.json",
        r##"{"on": [{"event": "fiyat.soruldu"}], "steps": [{"#notify": {"args": {"title": "Fiyat", "text": "$output"}}}]}"##,
    );
    assert!(!err(&itero(d.path(), &["check"])).contains("#notify takes"));
}

#[test]
fn repeat_is_set_and_shown_for_an_action() {
    let d = workspace();
    file(d.path(), "actions/imap-check", "#!/bin/sh\n");
    let o = itero(
        d.path(),
        &[
            "action",
            "repeat",
            "set",
            "imap-check",
            r#"[{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]"#,
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        manifest(d.path())["action"]["imap-check"]["repeat"],
        serde_json::json!([{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}])
    );
    let o = itero(d.path(), &["action", "repeat", "get", "imap-check"]);
    assert!(out(&o).contains("can@örnek.test"), "{}", out(&o));
    for bad in [r#"{"user": "a"}"#, "[]", "[1]"] {
        let o = itero(d.path(), &["action", "repeat", "set", "imap-check", bad]);
        assert!(!o.status.success(), "{bad}");
        assert!(
            err(&o).contains("repeat is a list of one or more objects"),
            "{bad}: {}",
            err(&o)
        );
    }
}
