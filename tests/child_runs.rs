//! A workflow used as a step, end to end: a run of its own, linked to its
//! parent, with its own end lists; the parent's step gets what it returned.
#![cfg(unix)]
mod common;
use itero::engine::daemon::run_once;
use itero::engine::journal::{Journal, Run};
use itero::engine::steps;
use serde_json::{Value, json};
use std::path::Path;

const NOW: &str = "2026-10-21T10:00:00Z";

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

/// A workspace in a temporary folder.
struct Ws {
    d: tempfile::TempDir,
    ws: itero::workspace::Workspace,
}

impl Ws {
    fn new() -> Ws {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.config.json"), "{}").unwrap();
        let ws = itero::workspace::find(d.path()).unwrap();
        Ws { d, ws }
    }

    fn write(&self, rel: &str, text: &str) {
        let f = self.d.path().join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }

    fn flow(&self, name: &str, v: Value) {
        self.write(&format!("workflows/{name}.json"), &v.to_string());
    }

    /// An action: a shell script, run in the workspace.
    fn action(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let rel = format!("actions/{name}");
        self.write(&rel, &format!("#!/bin/sh\n{body}\n"));
        let f = self.d.path().join(rel);
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn emit(&self, kind: &str, data: Value) {
        itero::engine::events::emit(&self.ws, kind, Some(data), NOW).unwrap();
    }

    fn pass(&self) {
        self.pass_at(NOW);
    }

    fn pass_at(&self, now: &str) {
        run_once(&self.ws, now.parse().unwrap(), exe()).unwrap();
    }

    fn runs(&self) -> Vec<Run> {
        Journal::open(&self.ws).unwrap().runs().unwrap()
    }

    /// The run of `workflow`, the only one.
    fn run(&self, workflow: &str) -> Run {
        let mut of: Vec<Run> = self
            .runs()
            .into_iter()
            .filter(|r| r.workflow == workflow)
            .collect();
        assert_eq!(of.len(), 1, "{workflow}: {of:#?}");
        of.remove(0)
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.d.path().join(rel)).unwrap_or_default()
    }

    fn log(&self) -> Vec<Value> {
        common::log_lines(&self.read(".itero/itero.log"))
    }
}

/// `sipariş` uses `stok` as a step, then writes what it returned.
fn order(w: &Ws, stok: Value) {
    w.flow(
        "sipariş",
        json!({ "on": [{ "event": "sipariş.geldi" }], "steps": [
            {"stok": {"args": {"depo": "$event.depo"}}},
            {"#yaz": {"args": {"durum": "$steps.stok.status", "cevap": "$output"}}}
        ] }),
    );
    w.flow("stok", stok);
    w.action("yaz", r#"echo "$durum $cevap" >> sonuç.log"#);
}

#[test]
fn a_workflow_used_as_a_step_is_a_run_of_its_own_and_returns_its_success_list_s_result() {
    let w = Ws::new();
    order(
        &w,
        json!({ "steps": [{"#bak": {"args": {"depo": "$event.depo"}}}],
                "success": ["#bildir"] }),
    );
    w.action("bak", r#"echo "{\"raf\": \"$depo-3\"}""#);
    w.action("bildir", r#"echo 'raf B-1'"#);
    w.emit("sipariş.geldi", json!({ "depo": "İzmir" }));
    w.pass();
    assert_eq!(w.read("sonuç.log"), "done raf B-1\n", "{:#?}", w.log());
    let (parent, child) = (w.run("sipariş"), w.run("stok"));
    assert_eq!(child.parent.as_deref(), Some(parent.id.as_str()));
    assert_eq!(child.parent_path, Some(vec![0, 0]));
    assert_eq!(child.input, json!({ "depo": "İzmir" }), "its $event");
    assert_eq!(steps::status(&child.runtime), "done");
    assert_eq!(steps::status(&parent.runtime), "done");
    assert_eq!(child.runtime.state["steps"]["bak"]["raf"], "İzmir-3");
    assert!(
        parent.runtime.state["steps"].get("bak").is_none(),
        "its insides are its own"
    );
    // the log says which run it is from
    let started = w
        .log()
        .into_iter()
        .find(|l| l["event"] == "run.started" && l["workflow"] == "stok")
        .unwrap();
    assert_eq!(started["parent"], json!(parent.id), "{started}");
    assert!(started.get("message").is_none(), "{started}");
    // each run's end is told, the child's too
    let done: Vec<Value> = w
        .log()
        .into_iter()
        .filter(|l| l["event"] == "message" && l["type"] == "itero.run.done")
        .map(|l| l["data"]["workflow"].clone())
        .collect();
    assert_eq!(done, vec![json!("stok"), json!("sipariş")]);
}

#[test]
fn a_child_that_broke_runs_its_undo_then_its_error_list_and_its_parent_goes_on() {
    let w = Ws::new();
    order(
        &w,
        json!({ "steps": [{"#kur": {"undo": "#sil"}}, "#kır"], "error": ["#rapor"] }),
    );
    w.action("kur", "echo kuruldu");
    w.action("sil", "echo silindi >> sonuç.log");
    w.action("kır", "echo 'çöktü' >&2; exit 3");
    w.action("rapor", "echo bozuk");
    w.emit("sipariş.geldi", json!({}));
    w.pass();
    assert_eq!(
        w.read("sonuç.log"),
        "silindi\nerror bozuk\n",
        "{:#?}",
        w.log()
    );
    assert_eq!(steps::status(&w.run("stok").runtime), "error");
    assert_eq!(steps::status(&w.run("sipariş").runtime), "done");
    // the child's error is told as any run's
    assert!(w.log().iter().any(|l| l["event"] == "message"
        && l["type"] == "itero.run.error"
        && l["data"]["workflow"] == "stok"));
}

#[test]
fn a_child_that_broke_with_no_error_list_returns_why() {
    let w = Ws::new();
    order(&w, json!({ "steps": ["#kır"] }));
    w.action("kır", "echo 'çöktü' >&2; exit 3");
    w.action("yaz", r#"echo "$durum $why" >> sonuç.log"#);
    w.flow(
        "sipariş",
        json!({ "on": [{ "event": "sipariş.geldi" }], "steps": [
            "stok",
            {"#yaz": {"args": {"durum": "$steps.stok.status", "why": "$output.failed_step"}}}
        ] }),
    );
    w.emit("sipariş.geldi", json!({}));
    w.pass();
    assert_eq!(w.read("sonuç.log"), "error #kır\n", "{:#?}", w.log());
    assert_eq!(steps::status(&w.run("sipariş").runtime), "done");
}

#[test]
fn a_child_that_waits_for_data_keeps_its_parent_waiting_on_it() {
    let w = Ws::new();
    order(&w, json!({ "steps": ["#onay"] }));
    w.action(
        "onay",
        r#"case "$event" in *'"onaylandı":true'*) echo 'onaylı'; exit 0 ;; esac
echo '{"sipariş": "ş-1"}'
exit 75"#,
    );
    w.emit("sipariş.geldi", json!({}));
    w.pass();
    assert_eq!(steps::status(&w.run("stok").runtime), "waiting");
    assert_eq!(steps::status(&w.run("sipariş").runtime), "running");
    w.emit("onay.geldi", json!({ "sipariş": "ş-1", "onaylandı": true }));
    w.pass();
    assert_eq!(w.read("sonuç.log"), "done onaylı\n", "{:#?}", w.log());
    assert_eq!(steps::status(&w.run("sipariş").runtime), "done");
}

#[test]
fn a_child_of_a_child_is_a_run_linked_to_its_own_parent() {
    let w = Ws::new();
    order(&w, json!({ "steps": ["raf"] }));
    w.flow("raf", json!({ "steps": ["#say"] }));
    w.action("say", "echo 3");
    w.emit("sipariş.geldi", json!({}));
    w.pass();
    assert_eq!(w.read("sonuç.log"), "done 3\n", "{:#?}", w.log());
    let (top, mid, low) = (w.run("sipariş"), w.run("stok"), w.run("raf"));
    assert_eq!(mid.parent.as_deref(), Some(top.id.as_str()));
    assert_eq!(low.parent.as_deref(), Some(mid.id.as_str()));
    assert_eq!(mid.runtime.state["steps"]["raf"]["output"], 3);
}

#[test]
fn a_child_that_lost_a_race_runs_on_to_its_end_and_its_result_reaches_nothing() {
    let w = Ws::new();
    w.flow(
        "yarış",
        json!({ "on": [{ "event": "yarış.başladı" }],
                "steps": [{ "race": ["#hızlı", "stok"] }, "#sonra"] }),
    );
    w.flow("stok", json!({ "steps": ["raf"] }));
    w.flow("raf", json!({ "steps": ["#yavaş"] }));
    w.action("hızlı", "echo hızlı");
    w.action("yavaş", "sleep 2; echo geç >> sonuç.log; echo geç");
    w.action("sonra", "echo sonra >> sonuç.log");
    w.emit("yarış.başladı", json!({}));
    w.pass();
    // the parent went on without it; it and its own child ran to their end
    assert_eq!(w.read("sonuç.log"), "sonra\ngeç\n", "{:#?}", w.log());
    let top = w.run("yarış");
    assert_eq!(steps::status(&top.runtime), "done");
    assert_eq!(top.runtime.state["steps"].get("stok"), None);
    assert_eq!(steps::status(&w.run("stok").runtime), "done");
    assert_eq!(steps::status(&w.run("raf").runtime), "done");
}

#[test]
fn a_parent_that_breaks_leaves_its_running_child_to_run_to_its_end() {
    let w = Ws::new();
    w.flow(
        "küme",
        json!({ "on": [{ "event": "küme.kur" }],
                "steps": [["#kır", "stok"], "#sonra"],
                "error": [{ "#rapor": { "args": { "why": "$output.why" } } }] }),
    );
    w.flow("stok", json!({ "steps": ["#yavaş"] }));
    w.action("kır", "echo 'olmadı' >&2; exit 4");
    w.action("yavaş", "sleep 2; echo bitti >> sonuç.log");
    w.action("sonra", "echo sonra >> sonuç.log");
    w.action("rapor", r#"echo "hata: $why" >> sonuç.log"#);
    w.emit("küme.kur", json!({}));
    w.pass();
    // the group waits for the child to end, then breaks: the child is done
    let log = w.read("sonuç.log");
    assert!(
        log.starts_with("bitti\nhata: exit 4"),
        "{log} {:#?}",
        w.log()
    );
    assert_eq!(steps::status(&w.run("küme").runtime), "error");
    assert_eq!(steps::status(&w.run("stok").runtime), "done");
}

#[test]
fn a_child_s_own_timeout_ends_its_wait_and_its_parent_s_does_not() {
    let w = Ws::new();
    // the parent's timeout is a minute: it waits for no data itself
    w.flow(
        "sipariş",
        json!({ "on": [{ "event": "sipariş.geldi" }], "timeout": "1m", "steps": [
            "stok",
            {"#yaz": {"args": {"durum": "$steps.stok.status", "cevap": "$output.why"}}}
        ] }),
    );
    w.flow("stok", json!({ "timeout": "1h", "steps": ["#onay"] }));
    w.action("onay", r#"echo '{"sipariş": "ş-1"}'; exit 75"#);
    w.action("yaz", r#"echo "$durum $cevap" >> sonuç.log"#);
    w.emit("sipariş.geldi", json!({}));
    w.pass_at("2026-10-21T10:00:00Z");
    w.pass_at("2026-10-21T10:30:00Z");
    assert_eq!(steps::status(&w.run("stok").runtime), "waiting");
    assert_eq!(steps::status(&w.run("sipariş").runtime), "running");
    // a pass's clock moves on while it runs: the wait began a second or
    // two after 10:00, so its hour is up a little after 11:00
    w.pass_at("2026-10-21T11:00:30Z");
    assert_eq!(steps::status(&w.run("stok").runtime), "error");
    assert_eq!(
        w.read("sonuç.log"),
        "error waited 1h for data\n",
        "{:#?}",
        w.log()
    );
    assert_eq!(steps::status(&w.run("sipariş").runtime), "done");
}
