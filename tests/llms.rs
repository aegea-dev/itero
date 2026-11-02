//! The example in docs.itero.sh/llms.txt (`docs/llms/intro.md`): its
//! workflow and action read as a workspace, and the action does what the
//! text says.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// The fenced blocks of `intro.md` that name a file after their language
/// (```` ```json workflows/report.json ````), as (path, text).
fn named_blocks() -> Vec<(String, String)> {
    let intro = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/llms/intro.md");
    let text = std::fs::read_to_string(&intro).unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(info) = line.strip_prefix("```") else {
            continue;
        };
        let body: Vec<&str> = lines
            .by_ref()
            .take_while(|l| !l.starts_with("```"))
            .collect();
        if let Some(path) = info.split_whitespace().nth(1) {
            out.push((path.to_string(), body.join("\n") + "\n"));
        }
    }
    out
}

#[test]
fn the_llms_txt_example_checks_and_its_action_does_what_it_says() {
    let blocks = named_blocks();
    let paths: Vec<&str> = blocks.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["workflows/report.json", "actions/count-lines"]);

    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("çalışma");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("itero.config.json"), "{}\n").unwrap();
    for (path, text) in &blocks {
        let file = ws.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }
    let action = ws.join("actions/count-lines");
    std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();

    let check = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "check"])
        .current_dir(&ws)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("XDG_STATE_HOME", d.path().join("durum"))
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");

    std::fs::write(ws.join("rapor.txt"), "bir\niki\nüç\n").unwrap();
    let counted = Command::new(&action)
        .current_dir(&ws)
        .env("file", "rapor.txt")
        .output()
        .unwrap();
    assert!(counted.status.success(), "{counted:?}");
    assert_eq!(String::from_utf8_lossy(&counted.stdout).trim(), "3");

    // a file it cannot read fails the action, as the text says
    let missing = Command::new(&action)
        .current_dir(&ws)
        .env("file", "yok.txt")
        .output()
        .unwrap();
    assert!(!missing.status.success(), "{missing:?}");
}
