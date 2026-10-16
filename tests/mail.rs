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
    // a journal left by trying the example by hand is not the test's
    let _ = std::fs::remove_dir_all(ws.join(".itero"));
    let flow = ws.join("workflows/mail-check.json");
    let text = std::fs::read_to_string(&flow).unwrap().replace(
        "\"mailbox\": \"\"",
        &format!("\"mailbox\": \"{}\"", mbox.display()),
    );
    std::fs::write(&flow, text).unwrap();
    // the source watches the same mailbox
    let src = ws.join("sources/mailbox.json");
    let text = std::fs::read_to_string(&src)
        .unwrap()
        .replace("/var/mail/${env.USER}", mbox.to_str().unwrap());
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
    pass(&ws, "2026-10-15T14:01:00Z", &log); // the hourly schedule is seen
    assert_eq!(shown(&log), "");
    pass(&ws, "2026-10-15T15:00:10Z", &log); // first run: one mail, not seen before
    assert_eq!(shown(&log), "Mail\t1 new: Toplantı notları\n");
    pass(&ws, "2026-10-15T16:00:10Z", &log); // nothing new
    assert_eq!(shown(&log).lines().count(), 1);
    mail(
        &mbox,
        "can@örnek.test",
        "Thu Oct 15 14:12:00 2026",
        "Çay saati",
    );
    pass(&ws, "2026-10-15T17:00:10Z", &log); // one new
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
    pass(&ws, "2026-10-15T15:00:10Z", &log);
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
    assert!(out.contains("source mailbox  watch@1"), "{out}");
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
fn mail_is_notified_as_it_arrives_and_never_twice() {
    let d = tempfile::tempdir().unwrap();
    let mbox = d.path().join("gelen kutusu");
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), &mbox);
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
    // the source is up (a busy machine can take a while), then a moment
    // for it to set its watch
    let running = || {
        reqwest::blocking::get(format!("http://127.0.0.1:{port}/health"))
            .and_then(|r| r.json::<serde_json::Value>())
            .is_ok_and(|h| h["sources"]["mailbox"]["state"] == "running")
    };
    assert!(wait_for(20, running), "the source did not start");
    std::thread::sleep(std::time::Duration::from_millis(500));
    mail(
        &mbox,
        "ayse@örnek.test",
        "Fri Oct 16 10:00:00 2026",
        "Toplantı",
    );
    assert!(
        wait_for(5, || shown(&log).contains("1 new: Toplantı")),
        "{}",
        shown(&log)
    );
    // two more, a moment apart: never a message counted twice
    mail(&mbox, "can@örnek.test", "Fri Oct 16 10:00:01 2026", "Çay");
    mail(
        &mbox,
        "elif@örnek.test",
        "Fri Oct 16 10:00:02 2026",
        "Kahve",
    );
    let memory = || {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["memory", "mail-check"])
            .current_dir(&ws)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    assert!(
        wait_for(5, || memory().contains("elif@örnek.test")),
        "{}",
        memory()
    );
    std::thread::sleep(std::time::Duration::from_secs(1));
    drop(daemon);
    let all = shown(&log);
    let counted: u32 = all
        .lines()
        .map(|l| {
            let text = l.split('\t').nth(1).unwrap();
            text.split(' ').next().unwrap().parse::<u32>().unwrap()
        })
        .sum();
    assert_eq!(counted, 3, "three mails, each counted once: {all}");
}
