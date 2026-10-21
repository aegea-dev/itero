//! The mail-alert example, live: its source tells the daemon about each new
//! mail in a local mailbox, and its workflow shows it.
#![cfg(unix)]
use std::path::{Path, PathBuf};

/// The example workspace, copied into `d`, with its mailbox at `mbox`.
fn example(d: &Path, mbox: &Path) -> PathBuf {
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
    let src = ws.join("sources/mail.received");
    let text = std::fs::read_to_string(&src)
        .unwrap()
        .replace("/var/mail/$USER", mbox.to_str().unwrap());
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

/// A daemon, killed when dropped.
struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn each_new_mail_is_shown_once_as_it_arrives_and_old_mail_never() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
    // there before the daemon: where the mailbox is, not news
    mail(&mbox, "eski@örnek.test", "Thu Oct 15 10:00:00 2026", "Eski");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(ws.join("itero.toml"), format!("port = {port}\n")).unwrap();
    let daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("daemon")
            .current_dir(&ws)
            .env("ITERO_NOTIFY_LOG", &log)
            .env("ITERO_NO_SECRET_STORE", "1")
            .spawn()
            .unwrap(),
    );
    // the source has noted where the mailbox is and set its watch
    let seen = ws.join(".itero/mail.received.seen");
    assert!(wait_for(20, || seen.exists()), "the source did not start");
    std::thread::sleep(std::time::Duration::from_millis(500));
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
    assert_eq!(
        shown(&log),
        "Mail\tayşe@örnek.test: Toplantı\nMail\tcan@örnek.test: Çay\nMail\telif@örnek.test: Kahve\n"
    );
}

#[test]
fn check_lists_the_workflow_and_the_source() {
    let d = tempfile::tempdir().unwrap();
    let ws = example(d.path(), &d.path().join("yok"));
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("check")
        .current_dir(&ws)
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(out.contains("mail-alert\n  #notify  built in"), "{out}");
    assert!(out.contains("source mail.received  "), "{out}");
}
