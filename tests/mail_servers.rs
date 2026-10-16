//! The mail-servers example against a real mail server: GreenMail, run by
//! `make test-docker` (Docker), whose address is `ITERO_GREENMAIL`
//! (`127.0.0.1`, with SMTP on 13025, POP3 on 13110 and IMAP on 13143).
//! The example's checks are container actions, so they run in Docker too,
//! and reach the server as `host.docker.internal`. Without
//! `ITERO_GREENMAIL` these tests say so and pass: `make test` needs no
//! Docker.
#![cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

const SMTP: u16 = 13025;
const POP3: u16 = 13110;
const IMAP: u16 = 13143;

fn server() -> Option<String> {
    let s = std::env::var("ITERO_GREENMAIL").ok();
    if s.is_none() {
        eprintln!("ITERO_GREENMAIL is not set: skipped (make test-docker runs it)");
    }
    s
}

/// A text as an RFC 2047 encoded word, so a subject with Turkish letters
/// travels as plain ASCII.
fn encoded_word(text: &str) -> String {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("=?UTF-8?B?{out}?=")
}

/// Sends one mail over SMTP.
fn send(host: &str, to: &str, subject: &str) {
    let s = TcpStream::connect((host, SMTP)).unwrap();
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut w = s;
    let mut line = String::new();
    let mut expect = |r: &mut BufReader<TcpStream>, code: &str| loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.starts_with(code), "SMTP: wanted {code}, got {line}");
        if line.as_bytes().get(3) != Some(&b'-') {
            break;
        }
    };
    expect(&mut r, "220");
    for (cmd, code) in [
        ("HELO ornek.test".to_string(), "250"),
        ("MAIL FROM:<gonderen@ornek.test>".to_string(), "250"),
        (format!("RCPT TO:<{to}>"), "250"),
        ("DATA".to_string(), "354"),
    ] {
        write!(w, "{cmd}\r\n").unwrap();
        expect(&mut r, code);
    }
    write!(
        w,
        "From: gonderen@ornek.test\r\nTo: {to}\r\nSubject: {}\r\n\r\nmerhaba\r\n.\r\n",
        encoded_word(subject)
    )
    .unwrap();
    expect(&mut r, "250");
    write!(w, "QUIT\r\n").unwrap();
}

/// The example, copied into `d`, its `workflow` pointed at the test server
/// and a new account (GreenMail makes it on first use).
fn example(d: &Path, workflow: &str, port: u16, user: &str, host: &str) -> PathBuf {
    let ws = d.join("ws");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/mail-servers");
    assert!(
        std::process::Command::new("cp")
            .arg("-R")
            .arg(&src)
            .arg(&ws)
            .status()
            .unwrap()
            .success()
    );
    let _ = std::fs::remove_dir_all(ws.join(".itero"));
    // only the workflow under test: the other still names example.com
    for other in ["imap-check", "pop3-check"]
        .iter()
        .filter(|w| **w != workflow)
    {
        std::fs::remove_file(ws.join(format!("workflows/{other}.json"))).unwrap();
    }
    let file = ws.join(format!("workflows/{workflow}.json"));
    let mut f: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let with = &mut f["transitions"][0]["do"][0]["with"];
    with["host"] = host.into();
    with["port"] = port.into();
    with["tls"] = false.into();
    with["user"] = user.into();
    std::fs::write(&file, serde_json::to_string_pretty(&f).unwrap()).unwrap();
    ws
}

fn pass(ws: &Path, now: &str, log: &Path) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["daemon", "--once", "--now", now])
        .current_dir(ws)
        .env("ITERO_NOTIFY_LOG", log)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("IMAP_PASSWORD", "gizli-şifre")
        .env("POP3_PASSWORD", "gizli-şifre")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
}

fn shown(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// The runs and the workflow's memory, for a failing assertion's message.
fn state(ws: &Path, workflow: &str) -> String {
    let out = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(args)
            .current_dir(ws)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    format!(
        "runs:\n{}memory: {}",
        out(&["runs"]),
        out(&["memory", workflow])
    )
}

fn failed_runs(ws: &Path) -> String {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("runs")
        .current_dir(ws)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| l.contains("failed"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mail that arrives after the first check is notified once, by `workflow`.
fn notifies_new_mail(workflow: &str, port: u16) {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("deneme-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    // the check runs in a container: this machine is host.docker.internal
    let ws = example(d.path(), workflow, port, &user, "host.docker.internal");
    send(&host, &user, "Eski posta");
    pass(&ws, "2026-10-16T10:01:00Z", &log); // the schedule is seen
    pass(&ws, "2026-10-16T10:05:10Z", &log); // first check: where the mailbox is
    assert_eq!(
        shown(&log),
        "",
        "mail from before the first check is not new"
    );
    send(&host, &user, "Toplantı notları");
    pass(&ws, "2026-10-16T10:10:10Z", &log);
    assert_eq!(
        shown(&log),
        "Mail\t1 new: Toplantı notları\n",
        "{}",
        state(&ws, workflow)
    );
    pass(&ws, "2026-10-16T10:15:10Z", &log); // nothing new
    assert_eq!(shown(&log).lines().count(), 1);
    send(&host, &user, "Çay saati");
    send(&host, &user, "Kahve molası");
    pass(&ws, "2026-10-16T10:20:10Z", &log);
    assert_eq!(
        shown(&log).lines().last().unwrap(),
        "Mail\t2 new: Kahve molası"
    );
    assert_eq!(failed_runs(&ws), "", "no run failed");
}

#[test]
fn imap_notifies_mail_that_arrives_after_the_first_check() {
    notifies_new_mail("imap-check", IMAP);
}

#[test]
fn pop3_notifies_mail_that_arrives_after_the_first_check() {
    notifies_new_mail("pop3-check", POP3);
}

#[test]
fn a_server_that_cannot_be_reached_fails_the_run_and_says_why() {
    let Some(_) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    // nothing listens on port 9 (discard) here
    let ws = example(d.path(), "imap-check", 9, "yok@ornek.test", "127.0.0.1");
    pass(&ws, "2026-10-16T10:01:00Z", &log);
    pass(&ws, "2026-10-16T10:05:10Z", &log);
    assert!(!failed_runs(&ws).is_empty(), "the run failed");
    assert_eq!(shown(&log), "");
}

fn wait_for(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

/// A daemon in `ws`, killed when dropped.
struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn leftover_sources() -> String {
    let o = std::process::Command::new("docker")
        .args(["ps", "-aq", "--filter", "label=dev.aegea.itero.source"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn imap_idle_notifies_mail_as_it_arrives() {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("idle-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), "imap-check", IMAP, &user, "host.docker.internal");
    // the source, in its container, pointed at the test server too
    let src = ws.join("sources/imap-idle.json");
    let mut s: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&src).unwrap()).unwrap();
    s["with"]["host"] = "host.docker.internal".into();
    s["with"]["port"] = IMAP.into();
    s["with"]["tls"] = false.into();
    s["with"]["user"] = user.as_str().into();
    std::fs::write(&src, serde_json::to_string(&s).unwrap()).unwrap();
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
            .env("IMAP_PASSWORD", "gizli-şifre")
            .spawn()
            .unwrap(),
    );
    // the source's first event runs a check, which notes where the mailbox is
    let done = || {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .arg("runs")
            .current_dir(&ws)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.contains(" done "))
            .count()
    };
    assert!(wait_for(60, || done() >= 1), "{}", state(&ws, "imap-check"));
    send(&host, &user, "Toplantı notları");
    // the 5-minute schedule has not come yet: the source told the daemon
    assert!(
        wait_for(20, || shown(&log).contains("1 new: Toplantı notları")),
        "{}{}",
        shown(&log),
        state(&ws, "imap-check")
    );
    assert_eq!(failed_runs(&ws), "", "no run failed");
    drop(daemon);
    // the daemon gone, its source's container goes too
    assert!(
        wait_for(15, || leftover_sources().is_empty()),
        "left: {}",
        leftover_sources()
    );
}
