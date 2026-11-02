//! The itero-ai program itself, as itero starts it: what it says and how
//! it exits when it cannot serve. No model is loaded.
#![cfg(unix)]
use std::io::{BufRead, BufReader};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn itero_ai(env: &[(&str, &std::ffi::OsStr)]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_itero-ai"));
    c.env_remove("ITERO_AI_SOCKET")
        .env_remove("ITERO_AI_IDLE")
        .env_remove("ITERO_AI_INDEX")
        .env_remove("ITERO_AI_PARENT")
        .envs(env.iter().copied());
    c
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn without_its_environment_it_says_what_is_missing_and_exits_2() {
    let o = itero_ai(&[]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", said(&o));
    assert!(said(&o).starts_with("itero-ai: "), "{}", said(&o));
    assert!(said(&o).contains("ITERO_AI_"), "{}", said(&o));
}

#[test]
fn with_a_parent_that_is_not_a_process_id_it_exits_2() {
    let d = tempfile::tempdir().unwrap();
    let (sock, index) = (d.path().join("ç.sock"), d.path().join("models.json"));
    let o = itero_ai(&[
        ("ITERO_AI_SOCKET", sock.as_os_str()),
        ("ITERO_AI_IDLE", std::ffi::OsStr::new("30")),
        ("ITERO_AI_INDEX", index.as_os_str()),
        ("ITERO_AI_PARENT", std::ffi::OsStr::new("üst")),
    ])
    .output()
    .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", said(&o));
    assert!(said(&o).contains("ITERO_AI_PARENT"), "{}", said(&o));
}

#[test]
fn on_a_socket_another_answers_on_it_exits_1() {
    let d = tempfile::tempdir().unwrap();
    let sock = d.path().join("ç.sock");
    let index = d.path().join("models.json");
    let env = [
        ("ITERO_AI_SOCKET", sock.as_os_str()),
        ("ITERO_AI_IDLE", std::ffi::OsStr::new("30")),
        ("ITERO_AI_INDEX", index.as_os_str()),
    ];
    let mut first = itero_ai(&env)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // on a Mac a start may first compile its Metal shaders, 15 seconds and
    // more; a second started sooner would take the socket itself
    let end = Instant::now() + Duration::from_secs(90);
    while std::os::unix::net::UnixStream::connect(&sock).is_err() {
        assert!(Instant::now() < end, "the first did not start");
        std::thread::sleep(Duration::from_millis(25));
    }
    let second = itero_ai(&env).output().unwrap();
    // SAFETY: kill only sends a signal, to the itero-ai this test started
    unsafe {
        libc::kill(first.id() as libc::pid_t, libc::SIGTERM);
    }
    let ended = first.wait().unwrap();
    assert_eq!(second.status.code(), Some(1), "{}", said(&second));
    assert!(said(&second).contains("answers there"), "{}", said(&second));
    assert!(ended.success(), "the first ends cleanly: {ended:?}");
}

/// An itero-ai held in its start, as compiling Metal's shaders does, by a
/// stand-in: a real compile cannot be asked for. Back once it says so.
fn starting(env: &[(&str, &std::ffi::OsStr)]) -> std::process::Child {
    let mut c = itero_ai(env)
        .env("ITERO_TEST_START_SECS", "60")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(c.stderr.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("starting"), "{line}");
    c
}

#[test]
fn told_to_stop_while_it_starts_it_exits_at_once() {
    let d = tempfile::tempdir().unwrap();
    let (sock, index) = (d.path().join("ç.sock"), d.path().join("models.json"));
    let mut c = starting(&[
        ("ITERO_AI_SOCKET", sock.as_os_str()),
        ("ITERO_AI_IDLE", std::ffi::OsStr::new("30")),
        ("ITERO_AI_INDEX", index.as_os_str()),
    ]);
    // SAFETY: kill only sends a signal, to the itero-ai this test started
    unsafe {
        libc::kill(c.id() as libc::pid_t, libc::SIGTERM);
    }
    let end = Instant::now() + Duration::from_secs(10);
    let ended = loop {
        if let Some(status) = c.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= end {
            let _ = c.kill();
            let _ = c.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(ended.is_some_and(|e| e.success()), "{ended:?}");
}

#[test]
fn with_its_parent_gone_while_it_starts_it_exits_at_once() {
    let d = tempfile::tempdir().unwrap();
    // the shell lives long enough for itero-ai to see it as its parent
    let pid = started_by_a_shell(
        d.path(),
        r#""$0" >/dev/null 2>&1 </dev/null & echo $!; sleep 1"#,
    );
    assert_gone(&pid);
}

#[test]
fn with_its_parent_gone_before_it_starts_it_exits_at_once() {
    let d = tempfile::tempdir().unwrap();
    // the shell has exited before itero-ai runs: the subshell waits, then
    // becomes itero-ai, whose parent is gone from its first instruction.
    // It is named, as itero names itself.
    let pid = started_by_a_shell(
        d.path(),
        r#"(sleep 0.5; ITERO_AI_PARENT=$$ exec "$0" >/dev/null 2>&1 </dev/null) & echo $!"#,
    );
    assert_gone(&pid);
}

/// The pid of an itero-ai held in its start, started in `d` by
/// `sh -c script` with the program as `$0`.
fn started_by_a_shell(d: &std::path::Path, script: &str) -> String {
    let (sock, index) = (d.join("ç.sock"), d.join("models.json"));
    let o = Command::new("sh")
        .args(["-c", script])
        .arg(env!("CARGO_BIN_EXE_itero-ai"))
        .env("ITERO_TEST_START_SECS", "60")
        .env("ITERO_AI_SOCKET", &sock)
        .env("ITERO_AI_IDLE", "30")
        .env("ITERO_AI_INDEX", &index)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// `pid` ends within 10 seconds; else it is killed and the test fails.
fn assert_gone(pid: &str) {
    let gone = || {
        let ps = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        // a zombie has ended too: its new parent has yet to reap it
        let stat = String::from_utf8_lossy(&ps.stdout);
        stat.trim().is_empty() || stat.contains('Z')
    };
    let end = Instant::now() + Duration::from_secs(10);
    while !gone() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let left = gone();
    if !left {
        // SAFETY: kill only sends a signal, to the itero-ai this test started
        unsafe {
            libc::kill(pid.parse().unwrap(), libc::SIGKILL);
        }
    }
    assert!(left, "itero-ai {pid} outlived its parent");
}
