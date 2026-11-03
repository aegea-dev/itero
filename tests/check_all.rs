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

fn ws_check(d: &Path, json: bool) -> std::process::Output {
    let mut args = vec!["ws", "check"];
    if json {
        args.push("--json");
    }
    std::process::Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(args)
        .current_dir(d)
        .env("ITERO_NO_SECRET_STORE", "1")
        .env("XDG_STATE_HOME", d.join("durum"))
        .env("XDG_CACHE_HOME", d.join("önbellek"))
        .output()
        .unwrap()
}

#[test]
fn ws_check_reports_every_problem_then_counts_them() {
    let d = tempfile::tempdir().unwrap();
    file(
        d.path(),
        "itero.config.json",
        r#"{"workflow": {"yok-böyle": {}, "bozuk": {}}}"#,
    );
    file(d.path(), "workflows/bozuk.json", "{ \"steps\": [");
    file(
        d.path(),
        "workflows/posta.json",
        r##"{ "on": [{ "event": "posta.geldi" }], "steps": [
        { "#ai": { "args": { "model": "u/k", "text": "x" } } },
        { "#ai": { "args": { "url": "https://api.openai.com/v1/chat/completions",
                             "key": "sk-proj-4f9aB2cD8eF1gH3iJ5kL", "message": "x" } } },
        "#gönder", "kayıp" ] }"##,
    );
    let ws = itero::workspace::find(d.path()).unwrap();
    let name = itero::engine::journal::Journal::open(&ws)
        .unwrap()
        .workspace()
        .unwrap()
        .name;
    let o = ws_check(d.path(), false);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(o.stdout.is_empty(), "no listing with errors");
    for line in [
        format!("({name}/workflow/yok-böyle)\n"),
        format!("({name}/workflow/bozuk)\n"),
        format!("error: it takes message, not text ({name}/workflow/posta/steps/#ai/1)\n"),
        format!(
            "error: no action gönder in the actions folders ({name}/workflow/posta/steps/#gönder)\n"
        ),
        format!("({name}/workflow/posta/steps/kayıp)\n"),
        format!(
            "warning: key looks like a token: write it as a $$NAME ({name}/workflow/posta/steps/#ai/2)\n"
        ),
        "5 errors, 1 warning\n".to_string(),
    ] {
        assert!(err.contains(&line), "{line}\n{err}");
    }
    // an arg refused is told once, as an error
    assert!(!err.contains("#ai takes"), "{err}");
    // a broken file's entry is not called missing
    assert_eq!(err.matches("bozuk").count(), 1, "{err}");
    let o = ws_check(d.path(), true);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["errors"].as_array().unwrap().len(), 5, "{v}");
    assert_eq!(
        v["warnings"][0]["at"],
        serde_json::json!(format!("{name}/workflow/posta/steps/#ai/2"))
    );
}
