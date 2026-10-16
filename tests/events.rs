use itero::engine::events::{EmitError, emit};
use itero::engine::journal::Journal;
use serde_json::json;

fn ws(d: &std::path::Path) -> itero::workspace::Workspace {
    std::fs::write(d.join("itero.toml"), "").unwrap();
    itero::workspace::find(d).unwrap()
}

const NOW: &str = "2026-10-15T14:00:00Z";

#[test]
fn an_app_event_is_saved_for_the_workspace() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let id = emit(
        &ws,
        "foto.eklendi",
        Some(json!({"yol": "tatil/çarşı.jpg"})),
        None,
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
fn bad_events_are_refused_and_nothing_is_saved() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let bad = |kind: &str, data: Option<serde_json::Value>, run: Option<&str>| {
        emit(&ws, kind, data, run, NOW).unwrap_err()
    };
    assert!(matches!(bad("Foto Eklendi", None, None), EmitError::Bad(_)));
    for own in [
        "action.finished",
        "timer.fired",
        "run.started",
        "workflow.paused",
        "source.paused",
    ] {
        let e = bad(own, None, None);
        assert!(e.to_string().contains("engine's own"), "{e}");
    }
    assert!(matches!(
        bad("foto.eklendi", Some(json!([1, 2])), None),
        EmitError::Bad(_)
    ));
    assert!(matches!(
        bad("onay.geldi", None, Some("yok")),
        EmitError::NoRun(_)
    ));
    assert!(Journal::open(&ws).unwrap().events(None).unwrap().is_empty());
}

#[test]
fn an_event_for_a_run_goes_to_that_run_until_it_ends() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws(d.path());
    let flow = itero::workflow::parse(
        r#"{ "name": "onay", "states": ["bekle", "bitti"], "start": "bekle", "end": ["bitti"],
      "transitions": [ { "from": "bekle", "on": "onay.geldi", "to": "bitti" } ] }"#,
    )
    .unwrap();
    let mut j = Journal::open(&ws).unwrap();
    let itero::engine::journal::Started::Run(run) =
        j.start_run(&flow, &Default::default(), NOW, None).unwrap()
    else {
        panic!("the run did not start")
    };
    let id = emit(
        &ws,
        "onay.geldi",
        Some(json!({"kişi": "yönetici"})),
        Some(&run),
        NOW,
    )
    .unwrap();
    let r = j.events(Some(&format!("itero.run.{run}"))).unwrap();
    assert!(r.iter().any(|r| r.event.id == id));
    // once ended, refused
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
    assert_eq!(j.run(&run).unwrap().unwrap().runtime.state, "bitti");
    assert!(matches!(
        emit(&ws, "onay.geldi", None, Some(&run), NOW).unwrap_err(),
        EmitError::Ended(_)
    ));
}
