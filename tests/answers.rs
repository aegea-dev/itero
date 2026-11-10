//! Each workspace command's answer is computed once: the CLI's `--json`
//! and the API's route give the same JSON, and the same message for a
//! mistake, on one workspace with a real daemon.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

const LONG: Duration = Duration::from_secs(20);

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < end, "waited {LONG:?} for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn http() -> reqwest::blocking::Client {
    itero::net::client(Duration::from_secs(30)).unwrap()
}

/// A workspace whose daemon has a run of each kind: `sipariş` done,
/// `kargo` broken at `#boz`, `bekle` waiting at `#bekle`.
struct Ws {
    d: tempfile::TempDir,
    root: PathBuf,
    port: u16,
    _daemon: Option<Daemon>,
}

impl Ws {
    fn new() -> Ws {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("çalışma");
        for dir in ["workflows", "actions", "connectors"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root = root.canonicalize().unwrap();
        // the daemon picks its port and keeps it: no port picked and let go
        std::fs::write(root.join("itero.config.json"), "{}").unwrap();
        for (name, kind, step) in [
            ("sipariş", "sipariş.verildi", "#yaz"),
            ("kargo", "kargo.çıktı", "#boz"),
            ("bekle", "bekle.başla", "#bekle"),
        ] {
            std::fs::write(
                root.join(format!("workflows/{name}.json")),
                json!({ "on": [{ "event": kind }], "steps": [step] }).to_string(),
            )
            .unwrap();
        }
        for (name, body) in [
            ("yaz", "echo '{\"tamam\": true}'"),
            ("boz", "echo 'olmadı' >&2; exit 1"),
            ("bekle", "exit 75"),
        ] {
            let p = root.join("actions").join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // a connector given args, which tells nothing, and ends when its
        // stdin closes (the daemon has gone), as a connector should
        let kapı = root.join("connectors/kapı.açıldı");
        std::fs::write(&kapı, "#!/bin/sh\nexec cat >/dev/null\n").unwrap();
        std::fs::set_permissions(&kapı, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            root.join("workflows/kapı.json"),
            json!({ "on": [{ "event": "kapı.açıldı", "args": { "kat": "üst" } }], "steps": ["#yaz"] })
                .to_string(),
        )
        .unwrap();
        let mut w = Ws {
            d,
            root,
            port: 0,
            _daemon: None,
        };
        let child = w
            .command()
            .arg("daemon")
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        w._daemon = Some(Daemon(child));
        let ws = itero::workspace::find(&w.root).unwrap();
        until("the daemon's port", || ws.saved_port().is_some());
        w.port = ws.saved_port().unwrap();
        until("the API", || {
            http()
                .get(w.url("/health"))
                .send()
                .is_ok_and(|r| r.status() == 200)
        });
        for kind in ["sipariş.verildi", "kargo.çıktı", "bekle.başla"] {
            let r = http()
                .post(w.url("/events"))
                .json(&json!({ "type": kind, "data": { "ürün": "çay" } }))
                .send()
                .unwrap();
            assert_eq!(r.status(), 202);
        }
        until("the three runs", || {
            let (_, v) = w.get("/runs");
            v.as_array().is_some_and(|r| {
                let st: Vec<&str> = r.iter().filter_map(|r| r["status"].as_str()).collect();
                st.len() == 3 && ["done", "error", "waiting"].iter().all(|s| st.contains(s))
            })
        });
        w
    }

    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(&self.root)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .stdin(Stdio::null());
        c
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// A route's status and JSON.
    fn get(&self, path: &str) -> (u16, Value) {
        let r = http().get(self.url(path)).send().unwrap();
        (r.status().as_u16(), r.json().unwrap_or(Value::Null))
    }

    /// `itero <args> --json`: its JSON, which must succeed.
    fn cli(&self, args: &[&str]) -> Value {
        let o = self.command().args(args).arg("--json").output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }

    /// `itero <args>`, which must fail: what it says.
    fn cli_fails(&self, args: &[&str]) -> String {
        let o = self.command().args(args).output().unwrap();
        assert!(!o.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }

    /// A request with a JSON body: its status and JSON.
    fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = http().request(method, self.url(path));
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().unwrap();
        (r.status().as_u16(), r.json().unwrap_or(Value::Null))
    }

    fn run_id(&self, status: &str) -> String {
        let (_, v) = self.get("/runs");
        v.as_array()
            .unwrap()
            .iter()
            .find(|r| r["status"] == status)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

/// The CLI's `--json` and the route's body are the same.
fn same(w: &Ws, args: &[&str], path: &str) {
    let (code, api) = w.get(path);
    assert_eq!(code, 200, "{path}: {api}");
    assert_eq!(w.cli(args), api, "{args:?} against {path}");
}

/// The CLI and the route refuse alike: the route's status, and the
/// route's message in what the CLI says.
fn refused(w: &Ws, args: &[&str], path: &str, status: u16) -> String {
    let (code, api) = w.get(path);
    assert_eq!(code, status, "{path}: {api}");
    let message = api["error"].as_str().unwrap().to_string();
    let said = w.cli_fails(args);
    assert!(said.contains(&message), "{said:?} lacks {message:?}");
    message
}

#[test]
fn runs_are_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    // every run is listed (the API's own default is 50: more than here)
    same(&w, &["ws", "wf", "runs"], "/runs");
    same(&w, &["ws", "wf", "runs", "--all"], "/runs?all=true");
    same(
        &w,
        &["ws", "wf", "runs", "--workflow", "sipariş"],
        "/runs?workflow=sipari%C5%9F",
    );
    same(&w, &["ws", "wf", "runs", "--errors"], "/runs?status=error");
    same(
        &w,
        &["ws", "wf", "runs", "--action", "boz"],
        "/runs?action=boz",
    );
    same(&w, &["ws", "wf", "runs", "--limit", "1"], "/runs?limit=1");
    // a broken run says where and why, in both
    let (_, v) = w.get("/runs?status=error");
    assert_eq!(v[0]["failed_step"], "#boz", "{v}");
    let one = w.run_id("error");
    same(&w, &["ws", "wf", "runs", &one], &format!("/runs/{one}"));
    let (_, v) = w.get(&format!("/runs/{one}"));
    for field in ["steps", "events", "input", "state"] {
        assert!(!v[field].is_null(), "{field} in {v}");
    }
}

#[test]
fn workflows_are_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    same(&w, &["ws", "wf", "list"], "/workflows");
    let (_, v) = w.get("/workflows");
    let sipariş = v["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "sipariş")
        .unwrap();
    assert_eq!(sipariş["path"], "workflows/sipariş.json");
    assert_eq!(sipariş["internal"], false);
    assert_eq!(sipariş["runs"], 1);
    assert_eq!(sipariş["on"]["events"][0]["event"], "sipariş.verildi");
    assert!(v["daemon"]["running"].is_boolean(), "{v}");
}

#[test]
fn a_mistake_is_said_alike_by_the_cli_and_the_api() {
    let w = Ws::new();
    let m = refused(
        &w,
        &["ws", "wf", "runs", "--workflow", "yok"],
        "/runs?workflow=yok",
        404,
    );
    assert!(m.contains("no workflow yok"), "{m}");
    let m = refused(
        &w,
        &["ws", "wf", "runs", "yok-böyle"],
        "/runs/yok-b%C3%B6yle",
        404,
    );
    assert!(m.contains("no run yok-böyle"), "{m}");
    // a run that has ended cannot be stopped: 409 and why
    let done = w.run_id("done");
    let r = http()
        .post(w.url(&format!("/runs/{done}/stop")))
        .send()
        .unwrap();
    assert_eq!(r.status(), 409);
    let api: Value = r.json().unwrap();
    let message = api["error"].as_str().unwrap();
    let said = w.cli_fails(&["ws", "wf", "stop", &done]);
    assert!(said.contains(message), "{said:?} lacks {message:?}");
}

#[test]
fn a_stop_is_answered_alike() {
    let w = Ws::new();
    let waiting = w.run_id("waiting");
    let api: Value = {
        // the CLI first: the route would stop it before the CLI asked
        let cli = w.cli(&["ws", "wf", "stop", &waiting]);
        assert_eq!(cli["daemon"], true, "{cli}");
        cli
    };
    assert!(api["stopping"].is_string(), "{api}");
    // the route's answer to the same question has the same fields
    let w2 = Ws::new();
    let waiting = w2.run_id("waiting");
    let r = http()
        .post(w2.url(&format!("/runs/{waiting}/stop")))
        .send()
        .unwrap();
    assert_eq!(r.status(), 202);
    let v: Value = r.json().unwrap();
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&v), keys(&api));
}

/// `v` without the fields that differ between two events sent alike.
fn but_ids(mut v: Value) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                for k in [
                    "id", "message", "run", "name", "started", "ended", "time", "at",
                ] {
                    m.remove(k);
                }
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut v);
    v
}

#[test]
fn events_are_listed_and_sent_alike() {
    let w = Ws::new();
    same(&w, &["ws", "ev", "list"], "/events");
    let data = json!({ "ürün": "şeker" });
    let cli = w.cli(&["ws", "emit", "sipariş.verildi", &data.to_string()]);
    let (code, api) = w.send(
        reqwest::Method::POST,
        "/events",
        Some(json!({ "type": "sipariş.verildi", "data": data })),
    );
    assert_eq!(code, 202, "{api}");
    assert_eq!(cli["id"].as_str().unwrap().len(), 36, "{cli}");
    assert_eq!(api["id"].as_str().unwrap().len(), 36, "{api}");
    assert_eq!(
        but_ids(cli.clone()),
        but_ids(api),
        "the same answer, its id apart"
    );
    assert_eq!(cli["starts"], json!(["sipariş"]), "{cli}");
    assert_eq!(cli["daemon"]["running"], true, "{cli}");
    // waiting for the runs it starts: the same runs, ended alike
    let o = w
        .command()
        .args([
            "ws",
            "emit",
            "--wait",
            "--timeout",
            "20s",
            "kargo.çıktı",
            "{}",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "a run in error exits 1");
    let cli: Value = serde_json::from_slice(&o.stdout).unwrap();
    let (code, api) = w.send(
        reqwest::Method::POST,
        "/events?wait=20",
        Some(json!({ "type": "kargo.çıktı", "data": {} })),
    );
    assert_eq!(code, 200, "{api}");
    assert_eq!(but_ids(cli.clone()), but_ids(api), "the same runs");
    assert_eq!(cli["waited"], "failed", "{cli}");
    assert_eq!(cli["runs"][0]["status"], "error", "{cli}");
}

#[test]
fn a_bad_event_and_a_cancel_are_refused_alike() {
    let w = Ws::new();
    // a type that is not one: 400, the same words
    let (code, api) = w.send(
        reqwest::Method::POST,
        "/events",
        Some(json!({ "type": "tek", "data": {} })),
    );
    assert_eq!(code, 400, "{api}");
    let said = w.cli_fails(&["ws", "emit", "tek", "{}"]);
    let message = api["error"].as_str().unwrap();
    assert!(said.contains(message), "{said:?} lacks {message:?}");
    // an event a daemon has handled: 409, what it started, in both
    let sent = w.cli(&["ws", "emit", "sipariş.verildi", "{}"]);
    let id = sent["id"].as_str().unwrap().to_string();
    until("the daemon to take it", || {
        let (_, q) = w.get("/events");
        q.as_array()
            .is_some_and(|q| q.iter().all(|e| e["id"] != id.as_str()))
    });
    let (code, api) = w.send(reqwest::Method::DELETE, &format!("/events/{id}"), None);
    assert_eq!(code, 409, "{api}");
    // the runs it started, by id
    let started = api["started"].as_array().unwrap();
    assert_eq!(started.len(), 1, "{api}");
    let message = api["error"].as_str().unwrap();
    assert!(
        message.contains(&format!("it started {}", started[0].as_str().unwrap())),
        "{message}"
    );
    let said = w.cli_fails(&["ws", "ev", "cancel", &id]);
    assert!(said.contains(message), "{said:?} lacks {message:?}");
    // one there is not: 404, the same words
    let (code, api) = w.send(reqwest::Method::DELETE, "/events/yok-b%C3%B6yle", None);
    assert_eq!(code, 404, "{api}");
    let message = api["error"].as_str().unwrap();
    let said = w.cli_fails(&["ws", "ev", "cancel", "yok-böyle"]);
    assert!(said.contains(message), "{said:?} lacks {message:?}");
}

#[test]
fn actions_connectors_and_breaks_are_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    same(&w, &["ws", "ac", "list"], "/actions");
    let (_, v) = w.get("/actions");
    let boz = v
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "boz")
        .unwrap();
    assert_eq!(boz["files"], json!(["actions/boz"]), "{v}");
    assert!(
        v.as_array().unwrap().iter().any(|a| a["built_in"] == true),
        "{v}"
    );
    same(&w, &["ws", "cn", "list"], "/connectors");
    let (_, v) = w.get("/connectors");
    assert_eq!(v[0]["type"], "kapı.açıldı", "{v}");
    assert_eq!(v[0]["args"], json!({ "kat": "üst" }), "{v}");
    same(&w, &["ws", "ac", "errors"], "/actions/errors");
    let (_, v) = w.get("/actions/errors");
    assert_eq!(v[0]["action"], "boz", "{v}");
    assert_eq!(v[0]["errors"], 1, "{v}");
    same(
        &w,
        &["ws", "ac", "errors", "boz"],
        "/actions/errors?action=boz",
    );
    same(
        &w,
        &["ws", "ac", "errors", "#boz", "--limit", "1"],
        "/actions/errors?action=%23boz&limit=1",
    );
    let (_, v) = w.get("/actions/errors?action=boz");
    assert_eq!(v[0]["why"], "exit 1: olmadı", "{v}");
    // an action that never broke a run: none, not a mistake
    same(
        &w,
        &["ws", "ac", "errors", "yaz"],
        "/actions/errors?action=yaz",
    );
}

#[test]
fn a_misspelt_parameter_is_refused_not_ignored() {
    let w = Ws::new();
    for path in ["/actions/errors?acton=boz", "/runs?statsu=error"] {
        let (code, v) = w.get(path);
        assert_eq!(code, 400, "{path}: {v}");
    }
}

/// `itero <args> --json`, failing or not: whether it succeeded, and its
/// JSON.
fn cli_json(w: &Ws, args: &[&str]) -> (bool, Value) {
    let o = w.command().args(args).arg("--json").output().unwrap();
    let v = serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{args:?}: {e}: {}", String::from_utf8_lossy(&o.stderr)));
    (o.status.success(), v)
}

#[test]
fn a_check_is_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    same(&w, &["ws", "check"], "/check");
    let (_, v) = w.get("/check");
    assert_eq!(v["errors"], json!([]), "{v}");
    let sipariş = v["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "sipariş")
        .unwrap();
    assert_eq!(sipariş["runs"], 1, "{v}");
    assert_eq!(sipariş["uses"][0]["action"], "yaz", "{v}");
    // two errors and a warning: all three told, by both
    std::fs::write(
        w.root.join("workflows/eksik.json"),
        json!({ "on": [{ "event": "eksik.oldu" }], "steps": ["#yok", "hiç"] }).to_string(),
    )
    .unwrap();
    std::fs::write(
        w.root.join("workflows/uyarı.json"),
        json!({ "on": [{ "event": "uyarı.geldi" }],
                "steps": [{ "#notify": { "args": { "başlık": "çay" } } }] })
        .to_string(),
    )
    .unwrap();
    let (code, api) = w.get("/check");
    assert_eq!(code, 200, "a report, not a failed request: {api}");
    let (ok, cli) = cli_json(&w, &["ws", "check"]);
    assert!(!ok, "the CLI fails when there are errors: {cli}");
    assert_eq!(cli, api);
    assert_eq!(api["errors"].as_array().unwrap().len(), 2, "{api}");
    assert_eq!(api["warnings"].as_array().unwrap().len(), 1, "{api}");
}

#[test]
fn info_is_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    same(&w, &["ws", "info"], "/info");
    let (_, v) = w.get("/info");
    let d = &v["daemon"];
    assert_eq!(d["running"], true, "{v}");
    assert_eq!(d["run_by"], "workspace", "{v}");
    assert_eq!(d["port"], w.port, "{v}");
    assert_eq!(d["api"], w.url("/").as_str(), "{v}");
    assert_eq!(v["workspace"]["path"], w.root.display().to_string(), "{v}");
}

#[test]
fn which_is_the_same_from_the_cli_and_the_api() {
    let w = Ws::new();
    // a workflow and an action both named bekle
    same(&w, &["ws", "which", "bekle"], "/which/bekle");
    let (_, v) = w.get("/which/bekle");
    assert_eq!(
        v,
        json!([
            { "kind": "workflow", "path": "workflows/bekle.json" },
            { "kind": "action", "path": "actions/bekle" },
        ])
    );
    // # looks among the actions only
    same(&w, &["ws", "which", "#bekle"], "/which/%23bekle");
    same(
        &w,
        &["ws", "which", "kapı.açıldı"],
        "/which/kap%C4%B1.a%C3%A7%C4%B1ld%C4%B1",
    );
    let m = refused(&w, &["ws", "which", "yok"], "/which/yok", 404);
    assert!(m.contains("nothing named yok"), "{m}");
}
