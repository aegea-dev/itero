//! itero-ai's program: where itero finds it, why one did not start, and
//! the folder its socket goes in. Its own test binary with one test: it
//! sets PATH and ITERO_AI_BIN for the process.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

fn program(at: &Path, body: &str) {
    std::fs::write(at, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn itero_ai_is_found_started_or_said_why_not() {
    let d = tempfile::tempdir().unwrap();
    let bin = d.path().join("yol");
    std::fs::create_dir(&bin).unwrap();
    program(&bin.join("itero-ai"), "exit 0");

    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("ITERO_AI_BIN", "/bir/yer/itero-ai");
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    assert_eq!(
        itero::ai::binary().unwrap(),
        Path::new("/bir/yer/itero-ai"),
        "ITERO_AI_BIN wins"
    );
    // SAFETY: as above
    unsafe {
        std::env::remove_var("ITERO_AI_BIN");
        std::env::set_var("PATH", &bin);
    }
    assert_eq!(
        itero::ai::binary().unwrap(),
        bin.join("itero-ai"),
        "on PATH"
    );
    // SAFETY: as above
    unsafe { std::env::set_var("PATH", d.path().join("boş")) };
    let e = itero::ai::binary().unwrap_err().to_string();
    assert!(e.contains("brew install aegea-dev/tap/itero-ai"), "{e}");

    // one that ends before it answers: what it said last is why
    let bozuk = d.path().join("bozuk");
    program(&bozuk, "echo 'ilk satır' >&2\necho 'model yok' >&2\nexit 3");
    // SAFETY: as above
    unsafe { std::env::set_var("ITERO_AI_BIN", &bozuk) };
    // the socket's folder is made this user's own, 0700
    let run = d.path().join("soket");
    std::fs::create_dir(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = itero::ai::ensure_at(run.join("a.sock"), Duration::from_secs(5), |_| ())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("before it answered: model yok"), "{e}");
    assert_eq!(
        std::fs::metadata(&run).unwrap().permissions().mode() & 0o777,
        0o700
    );

    // a socket's folder that is no folder of its own is refused
    let link = d.path().join("bağ");
    std::os::unix::fs::symlink(&run, &link).unwrap();
    let e = itero::ai::ensure_at(link.join("a.sock"), Duration::from_secs(5), |_| ())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("is not this user's own folder"), "{e}");
    let file = d.path().join("dosya");
    std::fs::write(&file, "").unwrap();
    assert!(
        itero::ai::ensure_at(file.join("a.sock"), Duration::from_secs(5), |_| ()).is_err(),
        "a file where the folder goes"
    );
}
