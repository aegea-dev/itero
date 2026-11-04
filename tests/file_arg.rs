//! A file passed as a step's arg: `{"file": "prompts/sort.md"}`.
#![cfg(unix)]
use itero::engine::journal::Journal;
use itero::workspace::Workspace;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

const NOW: &str = "2026-11-05T10:00:00Z";
/// What must arrive untouched: everything the fill would otherwise read.
const TEXT: &str = "Sırala: $event.x $$GİZLİ \\$ $5 $output \\ şüphe";

fn file(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn workspace() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    file(d.path(), "itero.config.json", "{}");
    file(d.path(), "prompts/sırala.md", &format!("{TEXT}\n"));
    for (name, body) in [("söyle", "printf '%s' \"$text\""), ("boş", "true")] {
        let a = d.path().join("actions").join(name);
        file(
            d.path(),
            &format!("actions/{name}"),
            &format!("#!/bin/sh\n{body}\n"),
        );
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    d
}

fn run(root: &Path, kind: &str, data: Value) -> Workspace {
    let ws = itero::workspace::find(root).unwrap();
    itero::engine::events::emit(&ws, kind, Some(data), NOW).unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_itero"));
    // a workflow used as a step takes a pass of its own
    for _ in 0..4 {
        itero::engine::daemon::run_once(&ws, NOW.parse().unwrap(), exe).unwrap();
    }
    ws
}

fn output(ws: &Workspace, workflow: &str) -> Value {
    let runs = Journal::open(ws).unwrap().runs().unwrap();
    let r = runs
        .iter()
        .find(|r| r.workflow == workflow)
        .expect(workflow);
    assert_eq!(
        itero::engine::steps::status(&r.runtime),
        "done",
        "{workflow}: {:?}",
        r.runtime.why
    );
    r.runtime.state["output"].clone()
}

#[test]
fn a_file_reaches_the_action_as_written_on_any_step() {
    let d = workspace();
    let f = json!({"file": "prompts/sırala.md"});
    let say = json!({"#söyle": {"args": {"text": f}}});
    for (name, body) in [
        ("tek", json!({"steps": [say]})),
        (
            "liste",
            json!({"steps": [{"#söyle": {"args": [{"text": f}]}}]}),
        ),
        (
            "yan",
            json!({"steps": [[say, "#boş"],
                                 {"#söyle": {"args": {"text": "$steps.söyle"}}}]}),
        ),
        (
            "grup",
            json!({"steps": [{"settled": [say, "#boş"]},
                                  {"#söyle": {"args": {"text": "$steps.söyle"}}}]}),
        ),
        ("son", json!({"steps": ["#boş"], "success": [say]})),
        ("üst", json!({"steps": [{"alt": {"args": {"text": f}}}]})),
    ] {
        let mut body = body;
        body["on"] = json!([{"event": format!("deneme.{name}")}]);
        file(
            d.path(),
            &format!("workflows/{name}.json"),
            &body.to_string(),
        );
    }
    file(
        d.path(),
        "workflows/alt.json",
        &json!({"steps": [{"#söyle": {"args": {"text": "$event.text"}}}]}).to_string(),
    );
    for name in ["tek", "liste", "yan", "grup", "son", "üst"] {
        let ws = run(d.path(), &format!("deneme.{name}"), json!({"x": "dolmaz"}));
        assert_eq!(output(&ws, name), TEXT, "{name}");
    }
}

#[test]
fn a_file_in_an_undo_is_read_too() {
    let d = workspace();
    file(
        d.path(),
        "workflows/geri.json",
        &json!({"steps": [{"#boş": {"undo": {"#söyle": {"args": {"text":
            {"file": "prompts/sırala.md"}}}}}}]})
        .to_string(),
    );
    let ws = itero::workspace::find(d.path()).unwrap();
    let values = ws.flow_values().unwrap();
    let text = values["geri"]
        .pointer("/steps/0/#boş/undo/#söyle/args/text")
        .and_then(Value::as_str)
        .unwrap();
    assert!(text.contains("şüphe"), "{text}");
}

#[test]
fn only_an_object_of_the_one_key_file_is_a_file() {
    let d = workspace();
    let data = json!({
        "file": "yok.png",
        "iki": {"file": "yok.png", "tür": "png"},
        "derin": {"kat": {"file": "yok.png"}},
        "liste": [{"file": "yok.png"}],
        "sayı": {"file": 3},
    });
    let body = json!({
        "on": [{"event": "posta.geldi", "args": {"yer": {"file": "yok.png"}}}],
        "steps": [{"#boş": {"args": data, "on": [{"$event.file": "yok.png"}]}}],
    });
    file(d.path(), "workflows/veri.json", &body.to_string());
    let ws = itero::workspace::find(d.path()).unwrap();
    let values = ws.flow_values().unwrap();
    assert_eq!(values["veri"].pointer("/steps/0/#boş/args"), Some(&data));
}

#[test]
fn check_says_every_file_it_cannot_use_and_nothing_runs() {
    let d = workspace();
    file(d.path(), ".env", "GİZLİ=değer\n");
    file(d.path(), ".itero/not.txt", "iç\n");
    let outside = tempfile::tempdir().unwrap();
    file(outside.path(), "dış.md", "dış\n");
    std::os::unix::fs::symlink(outside.path().join("dış.md"), d.path().join("prompts/dış"))
        .unwrap();
    std::os::unix::fs::symlink(d.path().join(".env"), d.path().join("prompts/gizli")).unwrap();
    let bad = [
        "prompts/yok.md",
        "../dış.md",
        "/etc/hosts",
        "~/not.md",
        ".env",
        ".ENV",
        ".itero/not.txt",
        "prompts/dış",
        "prompts/gizli",
        "$event.yol",
    ];
    let steps: Vec<Value> = bad
        .iter()
        .map(|p| json!({"#boş": {"args": {"text": {"file": p}}}}))
        .collect();
    file(
        d.path(),
        "workflows/kötü.json",
        &json!({"steps": steps}).to_string(),
    );
    let o = Command::new(env!("CARGO_BIN_EXE_itero"))
        .args(["ws", "check"])
        .current_dir(d.path())
        .env("ITERO_NO_SECRET_STORE", "1")
        .output()
        .unwrap();
    assert!(!o.status.success(), "{o:?}");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    for p in bad {
        assert!(said.contains(p), "{p}: {said}");
    }
    assert!(said.contains("10 errors"), "{said}");
    assert!(!said.contains("GİZLİ=değer"), "{said}");
    // as for any workflow that cannot be read
    let ws = itero::workspace::find(d.path()).unwrap();
    assert!(ws.flow_values().is_err());
}

#[test]
fn a_run_keeps_the_text_it_started_with_and_a_new_run_gets_the_edit() {
    use std::os::unix::fs::PermissionsExt;
    let d = workspace();
    file(d.path(), "prompts/not.md", "ilk\n");
    let wait = d.path().join("actions/bekle");
    file(
        d.path(),
        "actions/bekle",
        "#!/bin/sh\nif [ -n \"${event:-}\" ]; then exit 0; fi\necho '{\"no\": 1}'\nexit 75\n",
    );
    std::fs::set_permissions(&wait, std::fs::Permissions::from_mode(0o755)).unwrap();
    file(
        d.path(),
        "workflows/not.json",
        &json!({"on": [{"event": "not.yaz"}],
                "steps": ["#bekle", {"#söyle": {"args": {"text": {"file": "prompts/not.md"}}}}]})
        .to_string(),
    );
    let ws = run(d.path(), "not.yaz", json!({}));
    file(d.path(), "prompts/not.md", "ikinci\n");
    let _ = run(d.path(), "not.geldi", json!({"no": 1}));
    assert_eq!(output(&ws, "not"), "ilk");
    let _ = run(d.path(), "not.yaz", json!({}));
    let _ = run(d.path(), "not.geldi", json!({"no": 1}));
    let runs = Journal::open(&ws).unwrap().runs().unwrap();
    let outs: Vec<&Value> = runs
        .iter()
        .filter(|r| r.workflow == "not")
        .map(|r| &r.runtime.state["output"])
        .collect();
    assert_eq!(outs.len(), 2);
    assert!(outs.iter().any(|o| **o == "ikinci"), "{outs:?}");
    assert!(outs.iter().any(|o| **o == "ilk"), "{outs:?}");
}
