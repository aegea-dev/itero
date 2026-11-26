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
    let ws = itero::workspace::find_as_is(d.path()).unwrap();
    let name = itero::engine::journal::Journal::open(&ws)
        .unwrap()
        .workspace()
        .unwrap()
        .name;
    let o = ws_check(d.path(), false);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    assert!(o.stdout.is_empty(), "no listing with errors");
    // each file once, then its problems at their places
    for line in [
        "itero.config.json\n  error: workflow.yok-böyle: no workflow yok-böyle\n",
        "workflows/bozuk.json\n  error: not JSON: ",
        "workflows/posta.json\n",
        "  error: steps.#ai.1: it needs $message\n",
        "  error: steps.#gönder: no action gönder in the actions folders\n",
        "  error: steps.kayıp: ",
        "  warning: steps.#ai.2: key looks like a token: write it as a $$NAME\n",
        "5 errors, 2 warnings\n",
    ] {
        assert!(err.contains(line), "{line}\n{err}");
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
    assert!(err.starts_with("workflows/posta.json\n"), "{err}");
    assert!(err.contains("  error: unknown key \"renk\""), "{err}");
    for place in [
        "steps.#ai",
        "steps.all.#yok",
        "steps.all.#depo.2",
        "steps.#kargola",
        "steps.#depo.undo",
        "error",
    ] {
        let line = format!("  error: {place}: ");
        assert!(err.contains(&line), "{line}\n{err}");
    }
    assert!(
        err.contains("  error: steps.#ai: it needs $message"),
        "{err}"
    );
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
    let _ = name;
    for line in [
        "workflows/tarayıcı.json\n",
        "  error: steps.#browser.1: #browser: one action per step, not open and click; open is an http or https address, not \"ftp://örnek\"\n",
        "  error: steps.#browser.2: #browser: type needs into: the field to type in; timeout is seconds, more than 0 and at most 3600, not 0\n",
        "  warning: steps.#browser.3: #browser takes ",
        "2 errors, 1 warning\n",
    ] {
        assert!(err.contains(line), "{line}\n{err}");
    }
    assert!(err.contains("not bekle"), "{err}");
}

#[test]
fn the_daemon_refuses_the_browser_and_ai_steps_ws_check_refuses() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/tarayıcı.json",
        r##"{ "on": [{ "event": "sayfa.istendi" }], "steps": [
        { "#browser": { "args": { "open": "ftp://örnek", "click": "Gönder" } } },
        { "#ai": { "args": { "model": "u/var" } } } ] }"##,
    );
    let o = ws_check(d.path(), false);
    let check = String::from_utf8_lossy(&o.stderr).to_string();
    assert_eq!(o.status.code(), Some(1), "{check}");
    let ws = itero::workspace::find(d.path()).unwrap();
    let refused = itero::engine::daemon::refused(&ws).unwrap();
    assert_eq!(refused.len(), 2, "{refused:?}");
    // the same problems, told once each by ws check
    for p in &refused {
        assert_eq!(
            check.matches(p.message.as_str()).count(),
            1,
            "{}\n{check}",
            p.message
        );
    }
    assert!(check.contains("2 errors"), "{check}");
}

#[test]
fn ws_check_and_the_daemon_refuse_every_wrong_part_of_a_load_step() {
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(
        d.path(),
        "workflows/ölçüm.json",
        r##"{ "on": [{ "event": "ölç.istendi" }], "steps": [
        { "#load": { "args": { "measure": ["memroy", 3] } } },
        { "#load": { "args": { "ölç": true } } } ] }"##,
    );
    let o = ws_check(d.path(), false);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    for line in [
        "workflows/ölçüm.json\n",
        "#load: measure: \"memroy\" is not a part; did you mean \"memory\"?\n",
        "#load: measure: a part is text, not 3\n",
        "#load takes measure, not ölç\n",
        "2 errors, 1 warning\n",
    ] {
        assert!(err.contains(line), "{line}\n{err}");
    }
    let ws = itero::workspace::find(d.path()).unwrap();
    let refused = itero::engine::daemon::refused(&ws).unwrap();
    let said: Vec<&str> = refused.iter().map(|p| p.message.as_str()).collect();
    assert!(
        said.iter().any(|m| m.contains("\"memroy\" is not a part")),
        "{said:?}"
    );
    assert!(
        said.iter().any(|m| m.contains("a part is text, not 3")),
        "{said:?}"
    );
}

#[test]
fn ws_check_shows_warm_steps_and_says_each_that_cannot_be() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    file(
        d.path(),
        "itero.config.json",
        r#"{ "name": "ilik", "description": "warm" }"#,
    );
    for (name, text) in [
        ("şeftali", "#!/usr/bin/env python3\nprint('ılık')\n"),
        ("üzüm", "#!/bin/sh\necho üzüm\n"),
        ("ağaç", "#!/usr/bin/env node\nconsole.log('ağaç')\n"),
        ("çam", "#!/usr/bin/env bun\nconsole.log('çam')\n"),
    ] {
        file(d.path(), &format!("actions/{name}"), text);
        let f = d.path().join("actions").join(name);
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    file(
        d.path(),
        "workflows/sepet.json",
        r##"{ "on": [{ "event": "sepet.doldu" }], "steps": [
            { "#şeftali": { "warm": true } },
            { "#üzüm": { "warm": true } },
            { "#ağaç": { "warm": true } },
            { "#çam": { "warm": true } },
            { "#notify": { "args": { "title": "çay" }, "warm": true } }
        ] }"##,
    );
    let out = ws_check(d.path(), false);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "{text}");
    for says in [
        "workflows/sepet.json\n",
        "  error: steps.#üzüm: warm: only Python and Node actions can be warm: its first line runs sh\n",
        ": warm: only Python and Node actions can be warm: its first line runs bun\n",
        "  error: steps.#notify: warm: only Python and Node actions can be warm: #notify is built in\n",
        "3 errors",
    ] {
        assert!(text.contains(says), "{says}: {text}");
    }
    // the one that can: shown warm in the listing
    let out = ws_check(d.path(), true);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let uses = &v["workflows"][0]["uses"];
    let seftali = uses
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["action"] == "şeftali")
        .unwrap();
    assert!(
        seftali["notes"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("warm")),
        "{v:#}"
    );
    // Node can be warm now: shown so, not an error
    let agac = uses
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["action"] == "ağaç")
        .unwrap();
    assert!(
        agac["notes"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("warm")),
        "{v:#}"
    );
}

#[test]
fn ws_check_says_an_actions_entry_limits_are_gone_beside_another_problem() {
    let d = tempfile::tempdir().unwrap();
    file(
        d.path(),
        "itero.config.json",
        r#"{ "action": { "depo": { "limits": { "quiet": "1h" } } } }"#,
    );
    file(d.path(), "actions/depo", "#!/bin/sh\necho depo\n");
    {
        use std::os::unix::fs::PermissionsExt;
        let f = d.path().join("actions/depo");
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    file(
        d.path(),
        "workflows/sepet.json",
        r##"{ "on": [{ "event": "sepet.doldu" }], "steps": ["#depo", "#yok"] }"##,
    );
    let out = ws_check(d.path(), false);
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("itero.config.json\n  error: action.depo: limits are gone: write them on the steps that run it:"),
        "{text}"
    );
    assert!(
        text.contains("workflows/sepet.json\n  error: steps.#yok: no action yok"),
        "{text}"
    );
    assert!(text.contains("2 errors"), "{text}");
}
