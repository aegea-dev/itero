//! Clicks on notifications: `itero.notification.clicked`, saved by the
//! hidden `itero notification-clicked` that Itero.app runs, and `#notify`
//! waiting for one.
#![cfg(unix)]
use itero::engine::journal::Journal;
use std::path::Path;
use std::process::Output;

fn itero(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(dir)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("ITERO_NOTIFY_LOG", dir.join("bildirim.log"))
        .output()
        .unwrap()
}

fn file(d: &Path, rel: &str, text: &str) {
    let f = d.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, text).unwrap();
    if rel.starts_with("actions/") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// One pass of the daemon, now.
fn pass(d: &Path) {
    pass_at(d, chrono::Utc::now());
}

fn pass_at(d: &Path, at: chrono::DateTime<chrono::Utc>) {
    let now = itero::engine::daemon::stamp(at);
    let o = itero(d, &["daemon", "--once", "--now", &now]);
    assert!(o.status.success(), "{o:?}");
}

/// The notifications shown, one line each.
fn shown(d: &Path) -> Vec<String> {
    std::fs::read_to_string(d.join("bildirim.log"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// A workspace whose `dağıt` asks before it deploys.
fn asking(d: &Path) {
    file(d, "itero.config.json", "{}");
    file(
        d,
        "workflows/dağıt.json",
        r##"{ "on": [{ "event": "dağıtım.istendi" }],
             "steps": [
               { "#notify": { "args": { "title": "Dağıtılsın mı?", "text": "3 değişiklik",
                                        "buttons": ["Dağıt", "Vazgeç"] } } },
               { "#uygula": { "on": [{ "$output.answer": "Dağıt" }] } } ] }"##,
    );
    file(
        d,
        "actions/uygula",
        "#!/bin/sh\necho dağıtıldı > uygulandı.txt\n",
    );
}

/// The notification's id, from its line.
fn id_of(line: &str) -> String {
    line.split('\t')
        .find_map(|f| f.strip_prefix("notification="))
        .unwrap_or_else(|| panic!("no id in {line}"))
        .to_string()
}

fn click(d: &Path, id: &str, answer: &str) {
    let c = serde_json::json!({"notification": id, "answer": answer, "workflow": "dağıt"});
    let o = itero(d, &["notification-clicked", &c.to_string()]);
    assert!(o.status.success(), "{o:?}");
}

#[test]
fn a_notification_with_buttons_waits_for_its_answer() {
    let d = tempfile::tempdir().unwrap();
    asking(d.path());
    assert!(
        itero(d.path(), &["emit", "dağıtım.istendi"])
            .status
            .success()
    );
    pass(d.path());
    let lines = shown(d.path());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with(
            "Dağıtılsın mı?\t3 değişiklik\tbuttons=[\"Dağıt\",\"Vazgeç\"]\twait=true\tnotification="
        ),
        "{}",
        lines[0]
    );
    let id = id_of(&lines[0]);
    // waiting, run again in an hour: nothing shown again
    pass_at(d.path(), chrono::Utc::now() + chrono::Duration::hours(2));
    assert_eq!(shown(d.path()).len(), 1);
    assert!(!d.path().join("uygulandı.txt").exists());
    // another notification's click reaches nothing
    click(d.path(), "başka-bildirim", "Dağıt");
    pass(d.path());
    assert!(!d.path().join("uygulandı.txt").exists());
    // its own does: the click is #notify's output, and the run goes on
    click(d.path(), &id, "Dağıt");
    pass(d.path());
    assert_eq!(
        std::fs::read_to_string(d.path().join("uygulandı.txt")).unwrap(),
        "dağıtıldı\n"
    );
    assert_eq!(shown(d.path()).len(), 1);
}

#[test]
fn another_answer_or_a_dismissal_goes_on_without_the_step() {
    for answer in ["Vazgeç", "dismissed"] {
        let d = tempfile::tempdir().unwrap();
        asking(d.path());
        assert!(
            itero(d.path(), &["emit", "dağıtım.istendi"])
                .status
                .success()
        );
        pass(d.path());
        let id = id_of(&shown(d.path())[0]);
        click(d.path(), &id, answer);
        pass(d.path());
        assert!(!d.path().join("uygulandı.txt").exists(), "{answer}");
        let runs = String::from_utf8(itero(d.path(), &["wf", "runs"]).stdout).unwrap();
        assert!(runs.contains("done"), "{answer}: {runs}");
    }
}

#[test]
fn notify_s_buttons_are_a_short_list_of_labels() {
    for bad in [
        r#""Dağıt""#,
        "[]",
        r#"["a", "b", "c", "d", "e"]"#,
        r#"["", "b"]"#,
    ] {
        let d = tempfile::tempdir().unwrap();
        file(d.path(), "itero.config.json", "{}");
        file(
            d.path(),
            "workflows/sor.json",
            &format!(
                r##"{{ "on": [{{ "event": "soru.geldi" }}],
                     "steps": [{{ "#notify": {{ "args": {{ "text": "?", "buttons": {bad} }} }} }}] }}"##
            ),
        );
        assert!(itero(d.path(), &["emit", "soru.geldi"]).status.success());
        pass(d.path());
        let errors =
            String::from_utf8(itero(d.path(), &["wf", "runs", "--errors"]).stdout).unwrap();
        assert!(errors.contains("buttons"), "{bad}: {errors}");
        assert!(shown(d.path()).is_empty(), "{bad}");
    }
}

#[test]
fn a_click_is_itero_s_own_message_and_starts_workflows() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/okundu.json",
        r##"{ "on": [{ "event": "itero.notification.clicked" }, { "$event.workflow": "posta" }],
             "steps": [{ "#işaretle": { "args": { "answer": "$event.answer", "data": "$event.data" } } }] }"##,
    );
    file(
        d.path(),
        "actions/işaretle",
        "#!/bin/sh\nprintf '%s %s' \"$answer\" \"$data\" > işaretlendi.txt\n",
    );
    let click = r#"{"notification": "bildirim-1", "answer": "open", "workflow": "posta", "data": {"uid": "42"}}"#;
    let o = itero(d.path(), &["notification-clicked", click]);
    assert!(o.status.success(), "{o:?}");
    // another workflow's click starts nothing here
    let other = r#"{"notification": "bildirim-2", "answer": "open", "workflow": "başka"}"#;
    assert!(
        itero(d.path(), &["notification-clicked", other])
            .status
            .success()
    );
    let j = Journal::open(&itero::workspace::find(d.path()).unwrap()).unwrap();
    let own = j.events(Some("itero.app")).unwrap();
    assert_eq!(own[0].event.kind, "itero.notification.clicked");
    assert_eq!(own[0].event.source, "daemon");
    pass(d.path());
    assert_eq!(
        std::fs::read_to_string(d.path().join("işaretlendi.txt")).unwrap(),
        r#"open {"uid":"42"}"#
    );

    // from outside it is refused, and it needs its notification's id
    let o = itero(d.path(), &["emit", "itero.notification.clicked", click]);
    assert!(!o.status.success(), "{o:?}");
    for bad in [r#"{"answer": "open"}"#, "[1]", "{"] {
        let o = itero(d.path(), &["notification-clicked", bad]);
        assert!(!o.status.success(), "{bad}: {o:?}");
    }
}
