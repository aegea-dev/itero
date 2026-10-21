//! Messages from outside: checked, then saved for the workspace.
use itero::engine::events::{EmitError, emit};
use itero::engine::journal::Journal;
use serde_json::json;

fn ws(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

const NOW: &str = "2026-10-15T14:00:00Z";

#[test]
fn a_message_is_saved_for_the_workspace() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let id = emit(
        &ws,
        "foto.eklendi",
        Some(json!({"yol": "tatil/çarşı.jpg"})),
        NOW,
    )
    .unwrap();
    let j = Journal::open(&ws).unwrap();
    let r = j.events(Some("itero.app")).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].event.id, id);
    assert_eq!(r[0].event.kind, "foto.eklendi");
    assert_eq!(r[0].event.source, "outside");
    assert_eq!(r[0].event.data["yol"], "tatil/çarşı.jpg");
}

#[test]
fn bad_messages_are_refused_and_nothing_is_saved() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let bad = |kind: &str, data: Option<serde_json::Value>| emit(&ws, kind, data, NOW).unwrap_err();
    for kind in ["foto", "Foto Eklendi", "foto.", ".eklendi"] {
        let e = bad(kind, None);
        assert!(matches!(e, EmitError::Bad(_)), "{kind}");
        assert!(e.to_string().contains("two or more words"), "{e}");
    }
    for own in [
        "action.finished",
        "timer.fired",
        "run.started",
        "workflow.paused",
        "source.paused",
    ] {
        let e = bad(own, None);
        assert!(e.to_string().contains("engine's own"), "{e}");
    }
    assert!(matches!(
        bad("foto.eklendi", Some(json!([1, 2]))),
        EmitError::Bad(_)
    ));
    assert!(Journal::open(&ws).unwrap().events(None).unwrap().is_empty());
}
