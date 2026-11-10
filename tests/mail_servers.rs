//! The mail-servers and gmail examples against a real mail server: GreenMail, run by
//! `make test-docker` (Docker), whose address is `ITERO_GREENMAIL`
//! (`127.0.0.1`, with SMTP on 13025, POP3 on 13110 and IMAP on 13143).
//! The example's checks are shell around curl, run on this machine. Without
//! `ITERO_GREENMAIL` these tests say so and pass: `make test` needs no
//! Docker.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
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

/// Sends one mail over SMTP, its subject as an encoded word.
fn send(host: &str, to: &str, subject: &str) {
    send_header(host, to, &format!("Subject: {}", encoded_word(subject)));
}

/// Sends one mail over SMTP with `header` (its subject, as a mail program
/// wrote it).
fn send_header(host: &str, to: &str, header: &str) {
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
        "From: gonderen@ornek.test\r\nTo: {to}\r\n{header}\r\n\r\nmerhaba\r\n.\r\n"
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
    let args = &mut f["steps"][0][action.as_str()]["args"];
    args["host"] = host.into();
    args["port"] = port.into();
    args["tls"] = false.into();
    args["user"] = user.into();
    std::fs::write(&file, serde_json::to_string_pretty(&f).unwrap()).unwrap();
    ws
}

fn pass(ws: &Path, now: &str, log: &Path) {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "daemon", "--once", "--now", now])
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
        out(&["ws", "wf", "runs"]),
        out(&["ws", "wf", "runs", "--errors"])
    )
}

fn failed_runs(ws: &Path) -> String {
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "wf", "runs"])
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

/// Subjects as mail programs write them, each the newest of its check:
/// the header as sent, and the subject the notification shows.
const SUBJECTS: &[(&str, &str)] = &[
    // a long one: split into two encoded words, on two lines
    (
        "Subject: =?UTF-8?B?SGFmdGFsxLFrIHRvcGxhbnTEsSBub3RsYXLEsTog?=\r\n =?UTF-8?B?w6dhcsWfYW1iYSBzYWF0IMO8w6d0ZQ==?=",
        "Haftalık toplantı notları: çarşamba saat üçte",
    ),
    // Q encoding, in lower case; the space between two words is not text
    (
        "Subject: =?utf-8?q?=C3=87ay_saati?= =?utf-8?q?_ba=C5=9Flad=C4=B1?=",
        "Çay saati başladı",
    ),
    // plain text around an encoded word
    (
        "Subject: Re: =?UTF-8?B?U2lwYXJpxZ8=?= (2)",
        "Re: Sipariş (2)",
    ),
    // another charset: Ş is 0xDE in ISO-8859-9
    ("Subject: =?ISO-8859-9?Q?=DEeker?=", "Şeker"),
    // the header's name in capitals, and a tab in the text
    (
        "SUBJECT: Fiyat \"50\" \\ yüzde\tson",
        "Fiyat \"50\" \\ yüzde son",
    ),
    // none at all
    ("X-Note: konu yok", "(no subject)"),
];

/// Each subject in `SUBJECTS` is shown whole, by `workflow`.
fn reads_subjects_as_written(workflow: &str, port: u16) {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("konu-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    let ws = example(d.path(), workflow, port, &user, &host);
    send(&host, &user, "Eski posta"); // GreenMail makes the account
    pass(&ws, "2026-10-16T10:01:00Z", &log);
    pass(&ws, "2026-10-16T10:05:10Z", &log); // first check: where the mailbox is
    let mut wrong = Vec::new();
    for (i, (header, subject)) in SUBJECTS.iter().enumerate() {
        send_header(&host, &user, header);
        pass(&ws, &format!("2026-10-16T11:{:02}:10Z", i * 5), &log);
        let last = shown(&log).lines().last().unwrap_or_default().to_string();
        let want = format!("Mail\t1 new: {subject}");
        if last != want {
            wrong.push(format!("{header:?}\n  wanted {want:?}\n  shown  {last:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{}\n{}",
        wrong.join("\n"),
        state(&ws, workflow)
    );
    assert_eq!(failed_runs(&ws), "", "no run failed");
}

#[test]
fn imap_reads_subjects_as_mail_programs_write_them() {
    reads_subjects_as_written("imap-alert", IMAP);
}

#[test]
fn pop3_reads_subjects_as_mail_programs_write_them() {
    reads_subjects_as_written("pop3-alert", POP3);
}

/// RFC 2047 encoded words in `text`, decoded (UTF-8, B or Q, as
/// send-mail writes them); the whitespace between two of them dropped.
fn decode_words(text: &str) -> String {
    let mut out = Vec::new();
    let mut rest = text;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let Some(len) = rest[start + 2..]
            .match_indices("?=")
            .map(|(i, _)| i)
            .find(|i| rest[start + 2..start + 2 + i].matches('?').count() == 2)
        else {
            break;
        };
        let (pre, word) = (&rest[..start], &rest[start + 2..start + 2 + len]);
        if !(after_word && pre.trim().is_empty()) {
            out.extend_from_slice(pre.as_bytes());
        }
        let parts: Vec<&str> = word.splitn(3, '?').collect();
        assert!(parts[0].eq_ignore_ascii_case("utf-8"), "{word}");
        if parts[1].eq_ignore_ascii_case("b") {
            out.extend(from_base64(parts[2]));
        } else {
            out.extend(from_quoted(&parts[2].replace('_', " ")));
        }
        after_word = true;
        rest = &rest[start + 2 + len + 2..];
    }
    out.extend_from_slice(rest.as_bytes());
    String::from_utf8(out).unwrap()
}

fn from_base64(text: &str) -> Vec<u8> {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let (mut bits, mut n, mut out) = (0u32, 0, Vec::new());
    for c in text.bytes() {
        let Some(v) = B64.iter().position(|b| *b == c) else {
            continue;
        };
        bits = (bits << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    out
}

/// Quoted-printable: `=XX` is a byte, `=` at a line's end joins it to the
/// next.
fn from_quoted(text: &str) -> Vec<u8> {
    let text = text.replace("=\r\n", "");
    let b = text.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        if b[i] == b'='
            && let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The one mail in `user`'s inbox, as the server keeps it.
fn fetched(host: &str, user: &str) -> String {
    let o = std::process::Command::new("curl")
        .args(["-sS", "--fail", "-u", &format!("{user}:x")])
        .arg(format!("imap://{host}:{IMAP}/INBOX;UID=1"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    String::from_utf8(o.stdout).unwrap()
}

/// A header's value, its folded lines joined.
fn header_of(mail: &str, name: &str) -> Option<String> {
    let head = mail.split("\r\n\r\n").next().unwrap();
    let mut value: Option<String> = None;
    let mut on = false;
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if on {
                value.as_mut().unwrap().push_str(line);
            }
            continue;
        }
        on = false;
        if value.is_none()
            && line.len() > name.len()
            && line[..name.len() + 1].eq_ignore_ascii_case(&format!("{name}:"))
        {
            value = Some(line[name.len() + 1..].trim_start().to_string());
            on = true;
        }
    }
    value
}

/// `send-mail`, as an example has it, sends what any mail program reads:
/// a subject with Turkish letters too long for one encoded word, lines of
/// text, a line with only a dot, and a password with a quote and a
/// backslash, all at once.
fn send_mail_sends_what_mail_programs_read(example: &str) {
    let Some(host) = server() else { return };
    let to = format!("alici-{}@ornek.test", uuid::Uuid::now_v7());
    let subject = "Haftalık rapor: çarşamba günü saat üçte, ışıklı salonda, öğleden önce";
    let text = "Satır bir; \"tırnak\"\nsatır iki\n.\nson satır";
    let action = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(example)
        .join("actions/send-mail");
    let d = tempfile::tempdir().unwrap();
    let o = std::process::Command::new(&action)
        .current_dir(d.path())
        .env("host", &host)
        .env("port", SMTP.to_string())
        .env("tls", "false")
        .env("user", "gonderen@ornek.test")
        .env("password", "gizli \"şifre\" \\ son")
        .env("to", &to)
        .env("subject", subject)
        .env("text", text)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{example}: {o:?}");
    let mail = fetched(&host, &to);
    let mut wrong = Vec::new();
    match header_of(&mail, "Subject") {
        Some(s) if decode_words(&s) == subject => {
            for word in s.split_whitespace() {
                if word.len() > 75 {
                    wrong.push(format!("an encoded word over 75 characters: {word}"));
                }
            }
        }
        other => wrong.push(format!("subject: {other:?}")),
    }
    for (name, want) in [("From", "gonderen@ornek.test"), ("To", to.as_str())] {
        if header_of(&mail, name).as_deref() != Some(want) {
            wrong.push(format!("{name}: {:?}", header_of(&mail, name)));
        }
    }
    if header_of(&mail, "Date").is_none() {
        wrong.push("no Date".into());
    }
    // the text's part: its headers, then the text up to the boundary
    let lower = mail.to_ascii_lowercase();
    if !lower.contains("content-type: text/plain; charset=utf-8") {
        wrong.push("no text/plain in UTF-8".into());
    }
    let mark = "content-transfer-encoding: quoted-printable\r\n\r\n";
    let body = match lower.find(mark) {
        Some(at) => mail[at + mark.len()..].split("\r\n--").next().unwrap(),
        None => {
            wrong.push("not quoted-printable".into());
            ""
        }
    };
    let body = String::from_utf8(from_quoted(body)).unwrap_or_default();
    if body.trim_end() != text.replace('\n', "\r\n") {
        wrong.push(format!("body: {body:?}"));
    }
    assert!(wrong.is_empty(), "{example}: {}\n{mail}", wrong.join("\n"));
}

#[test]
fn the_messages_send_mail_sends_what_mail_programs_read() {
    send_mail_sends_what_mail_programs_read("messages");
}

#[test]
fn the_page_digest_send_mail_sends_what_mail_programs_read() {
    send_mail_sends_what_mail_programs_read("page-digest");
}

/// The gmail example, copied into `d`: its connector pointed at the test
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
    std::fs::write(&file, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    // the connector's args, in the workflow's on entry
    let file = ws.join("workflows/gmail.json");
    let mut f: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let args = &mut f["on"][0]["args"];
    args["host"] = host.into();
    args["port"] = IMAP.into();
    args["tls"] = false.into();
    args["user"] = user.into();
    std::fs::write(&file, serde_json::to_string_pretty(&f).unwrap()).unwrap();
    ws
}

/// `itero daemon` in `ws`, its notifications written to `log`; stopped
/// when dropped.
fn daemon(ws: &Path, log: &Path) -> stop::Daemon {
    stop::Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(["ws", "daemon"])
            .current_dir(ws)
            .env("ITERO_NOTIFY_LOG", log)
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("GMAIL_PASS", "gizli-sifre")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    )
}

/// Waits up to 20s for `done`.
fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !done() {
        assert!(std::time::Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// How many runs of the gmail workflow have ended, each saved by then. A
/// daemon killed sooner has told of a mail without having saved that it
/// did, and its step runs again: a step is repeated only if it was cut off.
fn gmail_runs_ended(ws: &Path) -> usize {
    std::fs::read_to_string(ws.join(".itero/itero.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(r#""event":"run.ended""#) && l.contains(r#""workflow":"gmail""#))
        .count()
}

#[test]
fn gmail_tells_each_new_mail_as_it_arrives_and_none_twice() {
    let Some(host) = server() else { return };
    let d = tempfile::tempdir().unwrap();
    let user = format!("anlik-{}@ornek.test", uuid::Uuid::now_v7());
    let log = d.path().join("bildirim.log");
    let ws = gmail_example(d.path(), &user, &host);
    send(&host, &user, "Eski posta");
    let first = daemon(&ws, &log);
    let seen = ws.join(".itero/gmail.received.seen");
    until("the first start to note the inbox", || seen.exists());
    std::thread::sleep(std::time::Duration::from_secs(1)); // in IDLE
    send(&host, &user, "Sipariş geldi: çay ve şeker");
    until("the notification", || !shown(&log).is_empty());
    until("its run to end", || gmail_runs_ended(&ws) == 1);
    drop(first);
    assert_eq!(
        shown(&log),
        "Gmail\tSipariş geldi: çay ve şeker\tsubtitle=gonderen@ornek.test\tsound=default\n",
        "mail from before the first start is not new"
    );
    // a mail while nothing runs is told on the next start, once
    send(&host, &user, "Kahve molası");
    let again = daemon(&ws, &log);
    until("the mail sent while down", || {
        shown(&log).lines().count() == 2
    });
    std::thread::sleep(std::time::Duration::from_secs(1));
    drop(again);
    assert_eq!(
        shown(&log).lines().last().unwrap(),
        "Gmail\tKahve molası\tsubtitle=gonderen@ornek.test\tsound=default"
    );
    assert_eq!(
        shown(&log).lines().count(),
        2,
        "none twice: {}",
        shown(&log)
    );
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
