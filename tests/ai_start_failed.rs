//! A start of itero-ai that never answers is told as
//! `itero.ai.start.failed`, with how long it ran and why. Its own test
//! binary: it sets ITERO_AI_BIN for the process, to a stand-in that ends
//! before it answers.
#![cfg(unix)]
use itero::ai::keeper::Keeper;
use itero::engine::journal::Journal;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_start_that_ends_before_it_answers_is_told_with_its_seconds_and_why() {
    let d = tempfile::tempdir().unwrap();
    let bin = d.path().join("itero-ai");
    let parent = d.path().join("üst");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\necho \"$ITERO_AI_PARENT\" > '{}'\nsleep 1\n\
             echo 'itero-ai: model klasörü okunamadı' >&2\nexit 3\n",
            parent.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
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

    let e = Keeper::global().socket(&ws).unwrap_err();
    assert!(e.contains("model klasörü okunamadı"), "{e}");

    let told: Vec<_> = Journal::open(&ws)
        .unwrap()
        .events(None)
        .unwrap()
        .into_iter()
        .filter(|r| r.event.kind.starts_with("itero.ai."))
        .collect();
    let kinds: Vec<_> = told.iter().map(|r| r.event.kind.as_str()).collect();
    assert_eq!(kinds, vec!["itero.ai.start.failed"]);
    let data = &told[0].event.data;
    assert_eq!(data["why"].as_str(), Some(e.as_str()));
    let took = data["seconds"].as_f64().unwrap();
    assert!((1.0..30.0).contains(&took), "it ran {took} s");
    // itero names itself, so itero-ai knows when it is gone
    let named = std::fs::read_to_string(&parent).unwrap();
    assert_eq!(named.trim(), std::process::id().to_string());
}
