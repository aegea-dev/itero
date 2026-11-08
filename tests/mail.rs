//! The mail-alert example, live: its connector tells the daemon about each new
//! mail in a local mailbox, and its workflow shows it.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use std::path::{Path, PathBuf};
use stop::Daemon;

/// The example workspace, copied into `d`, with its mailbox at `mbox`.
/// The mail-alert example in `d`, on `mbox`; with `late`, its watch is set
/// two seconds after the connector starts.
fn example(d: &Path, mbox: &Path, late: bool) -> PathBuf {
    let ws = d.join("ws");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mail-alert");
    let status = std::process::Command::new("cp")
        .arg("-R")
        .arg(&src)
        .arg(&ws)
        .status()
        .unwrap();
    assert!(status.success());
    // a journal left by trying the example by hand is not the test's
    let _ = std::fs::remove_dir_all(ws.join(".itero"));
    let src = ws.join("connectors/mail.received");
    let text = std::fs::read_to_string(&src)
        .unwrap()
        .replace("/var/mail/$USER", mbox.to_str().unwrap());
    let text = match late {
        false => text,
        true => {
            let watch = "itero watch \"$mailbox\"";
            assert_eq!(text.matches(watch).count(), 1, "{text}");
            text.lines()
                .map(|l| match l.contains(watch) {
                    true => format!("sleep 2\n{l}\n"),
                    false => format!("{l}\n"),
                })
                .collect()
        }
    };
    std::fs::write(&src, text).unwrap();
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

fn shown(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn wait_for(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

#[test]
fn each_new_mail_is_shown_once_as_it_arrives_and_old_mail_never() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox, false);
    // there before the daemon: where the mailbox is, not news
    mail(&mbox, "eski@örnek.test", "Thu Oct 15 10:00:00 2026", "Eski");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(
        ws.join("itero.config.json"),
        format!("{{\"port\": {port}}}"),
    )
    .unwrap();
    let daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NOTIFY_LOG", &log)
            .env("ITERO_NO_SECRET_STORE", "1")
            .spawn()
            .unwrap(),
    );
    // the connector has set its watch, then noted where the mailbox is
    let seen = ws.join(".itero/mail.received.seen");
    assert!(
        wait_for(20, || seen.exists()),
        "the connector did not start"
    );
    mail(
        &mbox,
        "ayşe@örnek.test",
        "Fri Oct 16 10:00:00 2026",
        "Toplantı",
    );
    assert!(
        wait_for(10, || shown(&log).contains("Toplantı")),
        "{}",
        shown(&log)
    );
    // two more, a moment apart: each shown, once
    mail(&mbox, "can@örnek.test", "Fri Oct 16 10:00:01 2026", "Çay");
    mail(
        &mbox,
        "elif@örnek.test",
        "Fri Oct 16 10:00:02 2026",
        "Kahve",
    );
    assert!(
        wait_for(10, || shown(&log).lines().count() >= 3),
        "{}",
        shown(&log)
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    drop(daemon);
    // each its own run, side by side: in any order, each once
    let all = shown(&log);
    let mut lines: Vec<&str> = all.lines().collect();
    lines.sort();
    assert_eq!(
        lines,
        vec![
            "Mail\tayşe@örnek.test: Toplantı",
            "Mail\tcan@örnek.test: Çay",
            "Mail\telif@örnek.test: Kahve"
        ]
    );
}

/// However late the connector's watch is set, its first read comes after
/// it: a mail as soon as it has noted the mailbox is shown.
#[test]
fn a_mail_is_shown_however_late_the_watch_is_set() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox, true);
    mail(&mbox, "eski@örnek.test", "Thu Oct 15 10:00:00 2026", "Eski");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(
        ws.join("itero.config.json"),
        format!("{{\"port\": {port}}}"),
    )
    .unwrap();
    let _daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NOTIFY_LOG", &log)
            .env("ITERO_NO_SECRET_STORE", "1")
            .spawn()
            .unwrap(),
    );
    let seen = ws.join(".itero/mail.received.seen");
    assert!(
        wait_for(20, || seen.exists()),
        "the connector did not start"
    );
    mail(&mbox, "ayşe@örnek.test", "Fri Oct 16 10:00:00 2026", "Geç");
    assert!(
        wait_for(10, || shown(&log).contains("Geç")),
        "{}",
        shown(&log)
    );
    assert!(!shown(&log).contains("Eski"), "{}", shown(&log));
}

#[test]
fn check_lists_the_workflow_and_the_connector() {
    let d = tempfile::tempdir().unwrap();
    let ws = example(d.path(), &d.path().join("yok"), false);
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "check"])
        .current_dir(&ws)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(
        out.contains("mail-alert\n  id ") && out.contains("  #notify  built in"),
        "{out}"
    );
    assert!(out.contains("connector mail.received  "), "{out}");
}

/// A mail as a mail program wrote it: its `From ` line, then `headers`
/// and a text.
fn mail_as_written(mbox: &Path, from_line: &str, headers: &str, text: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(mbox)
        .unwrap();
    write!(f, "From {from_line}\n{headers}\n\n{text}\n\n").unwrap();
}

#[test]
fn headers_are_shown_as_mail_programs_meant_them() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox, false);
    mail(&mbox, "eski@örnek.test", "Thu Oct 15 10:00:00 2026", "Eski");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(
        ws.join("itero.config.json"),
        format!("{{\"port\": {port}}}"),
    )
    .unwrap();
    let daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NOTIFY_LOG", &log)
            .env("ITERO_NO_SECRET_STORE", "1")
            .spawn()
            .unwrap(),
    );
    let seen = ws.join(".itero/mail.received.seen");
    assert!(
        wait_for(20, || seen.exists()),
        "the connector did not start"
    );
    // an encoded sender, and a long subject in two encoded words on two lines
    mail_as_written(
        &mbox,
        "ayse@ornek.test Fri Oct 16 10:00:00 2026",
        "From: =?UTF-8?B?QXnFn2U=?= <ayse@ornek.test>\n\
         Subject: =?UTF-8?B?SGFmdGFsxLFrIHRvcGxhbnTEsSBub3RsYXLEsTog?=\n \
         =?UTF-8?B?w6dhcsWfYW1iYSBzYWF0IMO8w6d0ZQ==?=",
        "merhaba",
    );
    // no subject: the "Subject:" in its text is not one
    mail_as_written(
        &mbox,
        "can@ornek.test Fri Oct 16 10:00:01 2026",
        "From: can@ornek.test",
        "Subject: bu metnin içinde",
    );
    // another charset, Q encoded, and a tab in it: Ş is 0xDE in ISO-8859-9
    mail_as_written(
        &mbox,
        "elif@ornek.test Fri Oct 16 10:00:02 2026",
        "From: elif@ornek.test\nSubject: =?ISO-8859-9?Q?=DEeker?=\tve \"tuz\"",
        "merhaba",
    );
    assert!(
        wait_for(10, || shown(&log).lines().count() >= 3),
        "{}",
        shown(&log)
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    drop(daemon);
    let all = shown(&log);
    let mut lines: Vec<&str> = all.lines().collect();
    lines.sort();
    assert_eq!(
        lines,
        vec![
            "Mail\tAyşe <ayse@ornek.test>: Haftalık toplantı notları: çarşamba saat üçte",
            "Mail\tcan@ornek.test: (no subject)",
            "Mail\telif@ornek.test: Şeker ve \"tuz\"",
        ]
    );
}
