//! The mail-servers and gmail examples against a real mail server: GreenMail, run by
//! `make test-docker` (Docker), whose address is `ITERO_GREENMAIL`
//! (`127.0.0.1`, with SMTP on 13025, POP3 on 13110 and IMAP on 13143).
//! The example's checks are shell around curl, run on this machine. Without
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
    for other in ["imap-alert", "pop3-alert"]
        .iter()
        .filter(|w| **w != workflow)
    {
        std::fs::remove_file(ws.join(format!("workflows/{other}.json"))).unwrap();
    }
    let file = ws.join(format!("workflows/{workflow}.json"));
    let mut f: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let action = format!("#{}", workflow.replace("-alert", "-check"));
    let with = &mut f["steps"][0][action.as_str()];
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
        // GreenMail offers IMAP LOGIN only, which takes an ASCII password;
        // POP3's PASS takes any
        .env("IMAP_PASS", "gizli-sifre")
        .env("POP3_PASS", "gizli-şifre")
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
    let memory =
        std::fs::read_to_string(ws.join(format!(".itero/memory/{workflow}"))).unwrap_or_default();
    let out = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(args)
            .current_dir(ws)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    format!(
        "runs:\n{}errors:\n{}memory: {memory}",
        out(&["runs"]),
        out(&["errors"])
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
        .filter(|l| l.contains(" error "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mail that arrives after the first check is notified once, by `workflow`.
fn notifies_new_mail(workflow: &str, port: u16) {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("deneme-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), workflow, port, &user, &host);
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
    notifies_new_mail("imap-alert", IMAP);
}

#[test]
fn pop3_notifies_mail_that_arrives_after_the_first_check() {
    notifies_new_mail("pop3-alert", POP3);
}

/// The gmail example, copied into `d`: its source pointed at the test
/// server, its daemon on a free port.
fn gmail_example(d: &Path, user: &str, host: &str) -> PathBuf {
    let ws = d.join("ws");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/gmail");
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
    let file = ws.join("itero.config.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    m["port"] = free.local_addr().unwrap().port().into();
    let args = &mut m["source"]["gmail.received"]["args"];
    args["host"] = host.into();
    args["port"] = IMAP.into();
    args["tls"] = false.into();
    args["user"] = user.into();
    std::fs::write(&file, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    ws
}

/// `itero daemon` in `ws`, its notifications written to `log`.
fn daemon(ws: &Path, log: &Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("daemon")
        .current_dir(ws)
        .env("ITERO_NOTIFY_LOG", log)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("GMAIL_PASS", "gizli-sifre")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

/// Waits up to 20s for `done`.
fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !done() {
        assert!(std::time::Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn gmail_tells_each_new_mail_as_it_arrives_and_none_twice() {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("anlik-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    let ws = gmail_example(d.path(), &user, &host);
    send(&host, &user, "Eski posta");
    let mut first = daemon(&ws, &log);
    let seen = ws.join(".itero/gmail.received.seen");
    until("the first start to note the inbox", || seen.exists());
    std::thread::sleep(std::time::Duration::from_secs(1)); // in IDLE
    send(&host, &user, "Sipariş geldi: çay ve şeker");
    until("the notification", || !shown(&log).is_empty());
    first.kill().unwrap();
    first.wait().unwrap();
    assert_eq!(
        shown(&log),
        "Gmail\tgonderen@ornek.test: Sipariş geldi: çay ve şeker\n",
        "mail from before the first start is not new"
    );
    // a mail while nothing runs is told on the next start, once
    send(&host, &user, "Kahve molası");
    let mut again = daemon(&ws, &log);
    until("the mail sent while down", || {
        shown(&log).lines().count() == 2
    });
    std::thread::sleep(std::time::Duration::from_secs(1));
    again.kill().unwrap();
    again.wait().unwrap();
    assert_eq!(
        shown(&log).lines().last().unwrap(),
        "Gmail\tgonderen@ornek.test: Kahve molası"
    );
    assert_eq!(shown(&log).lines().count(), 2, "none twice");
}

#[test]
fn a_server_that_cannot_be_reached_fails_the_run_and_says_why() {
    let Some(_) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("bildirim.log");
    // nothing listens on port 9 (discard) here
    let ws = example(d.path(), "imap-alert", 9, "yok@ornek.test", "127.0.0.1");
    pass(&ws, "2026-10-16T10:01:00Z", &log);
    pass(&ws, "2026-10-16T10:05:10Z", &log);
    assert!(!failed_runs(&ws).is_empty(), "the run failed");
    assert_eq!(shown(&log), "");
}
