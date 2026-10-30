//! The daemon's keeper of itero-ai, with the workspace's own itero-ai
//! build (no model needed). Its own test binary: it sets ITERO_AI_BIN and
//! XDG_CACHE_HOME for the process.
#![cfg(unix)]
use itero::ai::keeper::Keeper;
use itero::engine::journal::Journal;
use std::path::Path;
use std::time::Duration;

fn kinds(ws: &itero::workspace::Workspace) -> Vec<String> {
    Journal::open(ws)
        .unwrap()
        .events(None)
        .unwrap()
        .into_iter()
        .map(|r| r.event.kind)
        .filter(|k| k.starts_with("itero.ai."))
        .collect()
}

fn wait_for(ws: &itero::workspace::Workspace, kind: &str) {
    for _ in 0..100 {
        if kinds(ws).iter().any(|k| k == kind) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no {kind}: {:?}", kinds(ws));
}

#[test]
fn started_once_used_again_and_after_a_bad_end_started_after_a_pause() {
    // built beside itero by `cargo test --workspace`, wherever the target is
    let bin = Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai");
    assert!(
        bin.is_file(),
        "{} is not built: cargo build -p itero-ai",
        bin.display()
    );
    let d = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("ITERO_AI_BIN", &bin);
        std::env::set_var("XDG_CACHE_HOME", d.path().join("önbellek"));
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    let root = d.path().join("çalışma");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    let ws = itero::workspace::find(&root).unwrap();
    let k = Keeper::global();

    let sock = k.socket(&ws).unwrap();
    assert_eq!(sock, itero::ai::socket_for(&root));
    assert_eq!(k.socket(&ws).unwrap(), sock, "the one running is used");
    assert_eq!(kinds(&ws), vec!["itero.ai.started"]);
    assert_eq!(k.health().unwrap().state, "running");

    // ended badly (killed): it is not started again before its pause. The
    // keeper's own state turns first, and is asked at once: waiting for the
    // event in the journal could outlast the one-second pause on a slow
    // machine. (`stop` sends SIGTERM, which itero-ai ends on cleanly.)
    // SAFETY: kill only sends a signal, to the itero-ai the keeper started
    unsafe {
        libc::kill(k.pid().unwrap() as libc::pid_t, libc::SIGKILL);
    }
    for _ in 0..1000 {
        if k.health().unwrap().state == "failed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(k.health().unwrap().state, "failed");
    let e = k.socket(&ws).unwrap_err();
    assert!(e.contains("starts again in"), "{e}");
    wait_for(&ws, "itero.ai.failed");
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(k.socket(&ws).unwrap(), sock);
    assert_eq!(k.health().unwrap().state, "running");
    k.stop();
}
