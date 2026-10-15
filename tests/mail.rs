#![cfg(unix)]
use std::path::{Path, PathBuf};

/// The example workspace, copied into `d`, with its mailbox at `mbox`.
fn example(d: &Path, mbox: &Path) -> PathBuf {
    let ws = d.join("ws");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mail-check");
    let status = std::process::Command::new("cp")
        .arg("-R")
        .arg(&src)
        .arg(&ws)
        .status()
        .unwrap();
    assert!(status.success());
    let flow = ws.join("workflows/mail-check.json");
    let text = std::fs::read_to_string(&flow).unwrap().replace(
        "\"mailbox\": \"\"",
        &format!("\"mailbox\": \"{}\"", mbox.display()),
    );
    std::fs::write(&flow, text).unwrap();
    ws
}

fn mail(mbox: &Path, from: &str, date: &str, subject: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(mbox)
        .unwrap();
    write!(
        f,
        "From {from} {date}\nFrom: {from}\nSubject: {subject}\n\nmerhaba\n\n"
    )
    .unwrap();
}

fn itero(ws: &Path, args: &[&str], log: &Path) -> std::process::Output {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(ws)
        .env("ITERO_NOTIFY_LOG", log)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    o
}

fn pass(ws: &Path, now: &str, log: &Path) {
    itero(ws, &["daemon", "--once", "--now", now], log);
}

fn shown(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

#[test]
fn new_mail_gives_one_notification_and_old_mail_none() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("posta kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
    mail(
        &mbox,
        "ayse@örnek.test",
        "Thu Oct 15 13:50:00 2026",
        "Toplantı notları",
    );
    pass(&ws, "2026-10-15T14:01:00Z", &log); // the schedule is seen
    assert_eq!(shown(&log), "");
    pass(&ws, "2026-10-15T14:05:10Z", &log); // first run: one mail, not seen before
    assert_eq!(shown(&log), "Mail\t1 new: Toplantı notları\n");
    pass(&ws, "2026-10-15T14:10:10Z", &log); // nothing new
    assert_eq!(shown(&log).lines().count(), 1);
    mail(
        &mbox,
        "can@örnek.test",
        "Thu Oct 15 14:12:00 2026",
        "Çay saati",
    );
    pass(&ws, "2026-10-15T14:15:10Z", &log); // one new
    let all = shown(&log);
    assert_eq!(all.lines().count(), 2, "{all}");
    assert_eq!(all.lines().last().unwrap(), "Mail\t1 new: Çay saati");
    // what the runs and the memory say
    let runs = String::from_utf8(itero(&ws, &["runs"], &log).stdout).unwrap();
    assert_eq!(runs.lines().count(), 3, "{runs}");
    assert!(
        runs.lines()
            .all(|l| l.contains("mail-check") && l.contains("done")),
        "{runs}"
    );
    let memory = String::from_utf8(itero(&ws, &["memory", "mail-check"], &log).stdout).unwrap();
    let m: serde_json::Value = serde_json::from_str(&memory).unwrap();
    assert_eq!(m["seen"], "From can@örnek.test Thu Oct 15 14:12:00 2026");
}

#[test]
fn an_empty_or_missing_mailbox_gives_no_notification() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("yok");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
    pass(&ws, "2026-10-15T14:01:00Z", &log);
    pass(&ws, "2026-10-15T14:05:10Z", &log);
    assert_eq!(shown(&log), "");
    let runs = String::from_utf8(itero(&ws, &["runs"], &log).stdout).unwrap();
    assert!(runs.contains("done"), "{runs}");
}

#[test]
fn check_lists_the_workflows_and_their_versions() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &d.path().join("yok"));
    let out = String::from_utf8(itero(&ws, &["check"], &log).stdout).unwrap();
    let line = out.lines().find(|l| l.starts_with("mail-check ")).unwrap();
    assert_eq!(line.split_whitespace().nth(1).unwrap().len(), 12, "{out}");
    assert!(out.contains("mail.check@1"), "{out}");
}
