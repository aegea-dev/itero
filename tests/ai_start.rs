//! Starting itero-ai, or using the one that answers. Uses the workspace's
//! own itero-ai build, which needs no model to answer /health. Its own
//! test binary: it sets ITERO_AI_BIN and ITERO_AI_INDEX for the process.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::Duration;

fn itero_ai() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/itero-ai");
    assert!(
        p.is_file(),
        "{} is not built: cargo build -p itero-ai (make test builds it)",
        p.display()
    );
    p
}

#[test]
fn it_is_started_once_answers_and_is_used_again() {
    // the socket's path: short whatever the workspace's, and one each
    let long = "/çok/uzun/bir/yol".repeat(20);
    let a = itero::ai::socket_for(Path::new(&long));
    assert!(a.as_os_str().len() < 104, "{}", a.display());
    assert_ne!(a, itero::ai::socket_for(Path::new("/başka")));
    assert_eq!(a, itero::ai::socket_for(Path::new(&long)));

    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("çalışma");
    std::fs::create_dir(&root).unwrap();
    // SAFETY: this binary's only test, before any thread starts
    unsafe {
        std::env::set_var("ITERO_AI_BIN", itero_ai());
        std::env::set_var("ITERO_AI_INDEX", d.path().join("models.json"));
        std::env::remove_var("ITERO_AI_SOCKET");
    }
    let lines = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let l2 = lines.clone();
    let mut s = itero::ai::ensure(&root, Duration::from_secs(60), move |v| {
        l2.lock().unwrap().push(v)
    })
    .unwrap();
    assert!(s.child.is_some(), "none answered: one is started");
    assert_eq!(s.socket, itero::ai::socket_for(&root));
    let again = itero::ai::ensure(&root, Duration::from_secs(60), |_| ()).unwrap();
    assert!(again.child.is_none(), "one that answers is used");
    for _ in 0..50 {
        if !lines.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(lines.lock().unwrap()[0]["event"], "started");
    let health: serde_json::Value = itero::net::unix_client(&s.socket, Duration::from_secs(2))
        .unwrap()
        .get("http://itero-ai/health")
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(health["loaded"], serde_json::Value::Null);
    let dir = s.socket.parent().unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let mut child = s.child.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
}
