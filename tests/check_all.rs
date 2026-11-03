//! `itero ws check` reports every problem, each with its address.
use std::path::Path;

fn file(d: &Path, rel: &str, text: &str) {
    let p = d.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn every_workflow_that_can_be_read_is_and_the_rest_are_told() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/iyi.json",
        r##"{ "on": [{ "event": "a.b" }], "steps": ["#notify"] }"##,
    );
    file(d.path(), "workflows/bozuk.json", "{ \"steps\": [");
    file(
        d.path(),
        "workflows/eski.json",
        r##"{ "meta": {}, "steps": ["#notify"] }"##,
    );
    file(
        d.path(),
        "workflows/yanlış.json",
        r##"{ "steps": ["#notify"], "renk": 1, "timeout": "uzun" }"##,
    );
    let ws = itero::workspace::find(d.path()).unwrap();
    let (flows, ps) = ws.read_flows().unwrap();
    assert!(flows.iter().any(|(n, _)| n == "iyi"));
    assert!(
        !flows
            .iter()
            .any(|(n, _)| n == "bozuk" || n == "eski" || n == "yanlış")
    );
    let at: Vec<&str> = ps.iter().map(|p| p.at.as_str()).collect();
    assert_eq!(
        at,
        [
            "workflow/bozuk",
            "workflow/eski",
            "workflow/yanlış",
            "workflow/yanlış/timeout"
        ],
        "{ps:?}"
    );
    assert!(ps[0].message.contains("line 1"), "{}", ps[0].message);
    let e = format!("{:#}", ws.flows().unwrap_err());
    assert!(
        e.contains("renk") && e.contains("timeout") && e.contains("line 1"),
        "{e}"
    );
}
