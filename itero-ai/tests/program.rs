//! The itero-ai program itself, as itero starts it: what it says and how
//! it exits when it cannot serve. No model is loaded.
#![cfg(unix)]
use std::process::{Command, Output};
use std::time::Duration;

fn itero_ai(env: &[(&str, &std::ffi::OsStr)]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_itero-ai"));
    c.env_remove("ITERO_AI_SOCKET")
        .env_remove("ITERO_AI_IDLE")
        .env_remove("ITERO_AI_INDEX")
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
    for _ in 0..400 {
        if std::os::unix::net::UnixStream::connect(&sock).is_ok() {
            break;
        }
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
