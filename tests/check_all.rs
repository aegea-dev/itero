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
    let read = ws.read_flows().unwrap();
    let (flows, ps) = (read.flows, read.problems);
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
            "workflows/bozuk",
            "workflows/eski",
            "workflows/yanlış",
            "workflows/yanlış/timeout"
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
        format!("({name}/workflows/yok-böyle)\n"),
        format!("({name}/workflows/bozuk)\n"),
        format!("error: it needs $message ({name}/workflows/posta/steps/#ai/1)\n"),
        format!(
            "error: no action gönder in the actions folders ({name}/workflows/posta/steps/#gönder)\n"
        ),
        format!("({name}/workflows/posta/steps/kayıp)\n"),
        format!(
            "warning: key looks like a token: write it as a $$NAME ({name}/workflows/posta/steps/#ai/2)\n"
        ),
        "5 errors, 2 warnings\n".to_string(),
    ] {
        assert!(err.contains(&line), "{line}\n{err}");
    }
    // a broken file's entry is not called missing
    assert_eq!(err.matches("bozuk").count(), 1, "{err}");
    let o = ws_check(d.path(), true);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["errors"].as_array().unwrap().len(), 5, "{v}");
    let at = format!("{name}/workflows/posta/steps/#ai/2");
    assert!(
        v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["at"] == at.as_str()),
        "{v}"
    );
}

#[test]
fn the_steps_of_a_workflow_with_mistakes_are_checked_too() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), "actions/depo", "#!/bin/sh\n");
    file(
        d.path(),
        "workflows/posta.json",
        r##"{ "on": [{ "event": "posta.geldi" }], "renk": 1, "steps": [
        { "#ai": { "args": { "model": "u/k" } } },
        ["#depo", "#yok", { "#depo": { "argz": 1 } }],
        "#kargola",
        { "#depo": { "undo": "#iptal" } } ], "error": [7] }"##,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = d.path().join("actions/depo");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let o = ws_check(d.path(), false);
    let err = String::from_utf8_lossy(&o.stderr);
    for at in [
        "/workflows/posta)",
        "/workflows/posta/steps/#ai)",
        "/workflows/posta/steps/all/#yok)",
        "/workflows/posta/steps/all/#depo/2)",
        "/workflows/posta/steps/#kargola)",
        "/workflows/posta/steps/#depo/undo)",
        "/workflows/posta/error)",
    ] {
        assert!(err.contains(at), "{at}\n{err}");
    }
    assert!(err.contains("it needs $message ("), "{err}");
    assert!(err.contains("7 errors"), "{err}");
}

#[test]
fn ws_check_says_every_problem_of_every_browser_step() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/tarayıcı.json",
        r##"{ "on": [{ "event": "sayfa.istendi" }], "steps": [
        { "#browser": { "args": { "open": "ftp://örnek", "click": "Gönder" } } },
        { "#browser": { "args": { "type": "ışık", "timeout": 0 } } },
        { "#browser": { "args": { "open": "$event.url", "bekle": 3 } } } ] }"##,
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
    for line in [
        format!(
            "error: #browser: one action per step, not open and click; open is an http or https address, not \"ftp://örnek\" ({name}/workflows/tarayıcı/steps/#browser/1)\n"
        ),
        format!(
            "error: #browser: type needs into: the field to type in; timeout is seconds, more than 0 and at most 3600, not 0 ({name}/workflows/tarayıcı/steps/#browser/2)\n"
        ),
        "warning: #browser takes ".to_string(),
        "2 errors, 1 warning\n".to_string(),
    ] {
        assert!(err.contains(&line), "{line}\n{err}");
    }
    assert!(err.contains("not bekle"), "{err}");
}
