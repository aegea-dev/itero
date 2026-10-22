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
    if rel.starts_with("actions/") || rel.starts_with("sources/") {
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
    file(
        d.path(),
        "workflows/bakım.json",
        r##"{ "meta": { "internal": true }, "on": [{ "event": "a.b" }], "steps": ["#tamam"] }"##,
    );
    itero(d.path(), &["emit", "a.b", "{}"]);
    itero(d.path(), &["daemon", "--once"]);
    assert!(!out(&itero(d.path(), &["runs"])).contains("bakım"));
    assert!(out(&itero(d.path(), &["runs", "--all"])).contains("bakım"));
    let errors = out(&itero(d.path(), &["errors"]));
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
        "#!/bin/sh\n[ -n \"$tamam\" ] || { echo '{\"ödeme\": \"bekliyor\"}'; exit 75; }\n",
    );
    file(
        d.path(),
        "actions/gönder",
        "#!/bin/sh\necho '{\"kargo\": \"TR-ü1\"}'\n",
    );
    itero(d.path(), &["emit", "sipariş.geldi", r#"{"sipariş": 1}"#]);
    itero(d.path(), &["daemon", "--once"]);
    let list = out(&itero(d.path(), &["runs"]));
    let name = list.split_whitespace().nth(1).unwrap().to_string();
    let waiting = out(&itero(d.path(), &["runs", &name]));
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
    let story = out(&itero(d.path(), &["runs", id]));
    for says in ["done", "ödeme.geldi", "#gönder", "TR-ü1", "state"] {
        assert!(story.contains(says), "{says}: {story}");
    }
    let o = itero(d.path(), &["runs", "yok-böyle"]);
    assert!(
        !o.status.success() && err(&o).contains("no run yok-böyle"),
        "{o:?}"
    );
}

#[test]
fn info_shows_the_workspace_and_check_each_workflows_identity() {
    let d = workspace();
    file(
        d.path(),
        "itero.toml",
        "port = 8797\n\n[meta]\nsahibi = \"ayşe\"\n\n[limits]\nruns = 7\n",
    );
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "meta": { "description": "Dükkân", "ekip": "ödeme" }, "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    let info = out(&itero(d.path(), &["info"]));
    for says in [
        "id ",
        "name ",
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
    file(d.path(), "itero.toml", "port = 8799\n");
    file(
        d.path(),
        "workflows/sipariş.json",
        r##"{ "on": [{ "event": "order.placed" }], "steps": ["#gönder"] }"##,
    );
    file(
        d.path(),
        "workflows/özet.json",
        r##"{ "meta": { "off": true }, "on": [{ "schedule": "0 9 * * *" }, { "path": "gelen" }], "steps": ["#gönder"] }"##,
    );
    file(
        d.path(),
        "workflows/adım.json",
        r##"{ "steps": ["#gönder"] }"##,
    );
    file(d.path(), "actions/gönder", "#!/bin/sh\n");
    itero(d.path(), &["emit", "order.placed", r#"{"sipariş": 1}"#]);
    let before = out(&itero(d.path(), &["workflows"]));
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
        before.lines().last().unwrap() == "daemon: not running, 1 message waiting",
        "{before}"
    );
    itero(d.path(), &["daemon", "--once"]);
    let after = out(&itero(d.path(), &["workflows"]));
    let s = line(&after, "sipariş");
    assert!(s.contains("runs 1") && s.contains("last: done"), "{after}");
    assert_eq!(
        after.lines().last().unwrap(),
        "daemon: not running, no message waiting"
    );
}

#[test]
fn emit_says_what_a_message_starts() {
    let d = workspace();
    file(d.path(), "itero.toml", "port = 8798\n");
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
    file(d.path(), "itero.toml", "port = 8802\n");
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
    let o = itero(
        d.path(),
        &["emit", "--reply", "ftp://örnek.test", "kişi.geldi", "{}"],
    );
    assert!(!o.status.success() && err(&o).contains("http"), "{o:?}");
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
    assert_eq!(run["state"]["selam"], "merhaba ayşe");
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
    file(d.path(), "sources/posta/posta.geldi", "#!/bin/sh\n");
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
        "source    sources/posta/posta.geldi\n"
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
    let list = out(&itero(d.path(), &["workflows"]));
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
