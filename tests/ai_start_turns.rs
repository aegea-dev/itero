//! itero-ais that use the GPU start one at a time, whoever starts them;
//! those that only read headers do not wait. A stand-in logs when each
//! start begins and ends, then runs the real itero-ai without Metal, so
//! the order says it all and no shader compile can blur it. Its own test
//! binary: it sets the process's environment.
#![cfg(unix)]
use itero::ai::Started;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

fn itero_ai() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai");
    assert!(p.is_file(), "{} is not built", p.display());
    p
}

/// Writes `text` from a child, so no thread of this process keeps the
/// script open for writing when it runs.
fn script(path: &Path, text: &str) {
    let mut c = Command::new("sh")
        .arg("-c")
        .arg(format!("cat > '{}'", path.display()))
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Starts three side by side, and gives the log of how they went.
fn three(run: &Path, log: &Path, name: &str, start: fn(PathBuf) -> Started) -> Vec<String> {
    let _ = std::fs::remove_file(log);
    let starts: Vec<_> = (0..3)
        .map(|i| {
            let socket = run.join(format!("{name}{i}.sock"));
            std::thread::spawn(move || start(socket))
        })
        .collect();
    let started: Vec<Started> = starts.into_iter().map(|t| t.join().unwrap()).collect();
    for mut s in started {
        let mut child = s.child.take().expect("each started its own");
        let _ = child.kill();
        let _ = child.wait();
    }
    std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn only_what_uses_the_gpu_takes_turns() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("günlük");
    let bin = d.path().join("itero-ai-yerine");
    script(
        &bin,
        &format!(
            "#!/bin/sh\necho başladı >> '{log}'\nsleep 1\necho bitti >> '{log}'\n\
             GGML_METAL_DEVICES=0 exec '{real}'\n",
            log = log.display(),
            real = itero_ai().display()
        ),
    );
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("ITERO_AI_BIN", &bin);
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    let run = d.path().join("k");

    let readers = three(&run, &log, "b", |s| {
        itero::ai::header_reader_at(s, Duration::from_secs(30)).unwrap()
    });
    assert_eq!(readers[..3], ["başladı"; 3], "readers do not wait");

    let steps = three(&run, &log, "a", |s| {
        itero::ai::ensure_at(s, Duration::from_secs(30), |_| ()).unwrap()
    });
    assert_eq!(
        steps,
        ["başladı", "bitti", "başladı", "bitti", "başladı", "bitti"],
        "one at a time"
    );
}
