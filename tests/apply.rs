//! A server takes a workspace: `PUT /workspaces/{name}` with its files
//! makes it the first time and mirrors it after, on the machine's daemon.
//! Every command runs with HOME, XDG_CONFIG_HOME and XDG_STATE_HOME in a
//! temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use base64::Engine as _;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

struct Server {
    d: tempfile::TempDir,
    url: String,
    _daemon: Daemon,
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn http() -> reqwest::blocking::Client {
    itero::net::client(Duration::from_secs(30)).unwrap()
}

impl Server {
    /// A machine's daemon with an empty list, once its API answers.
    fn new() -> Server {
        let d = tempfile::tempdir().unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_itero"))
            .current_dir(d.path())
            .env("HOME", d.path().join("ev"))
            .env("XDG_CONFIG_HOME", d.path().join("ayar"))
            .env("XDG_STATE_HOME", d.path().join("durum"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let kept = d.path().join("durum/itero/port");
        let end = Instant::now() + Duration::from_secs(20);
        let url = loop {
            assert!(Instant::now() < end, "waited 20s for the daemon's API");
            if let Some(port) = std::fs::read_to_string(&kept)
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok())
            {
                let u = format!("http://127.0.0.1:{port}");
                if http()
                    .get(format!("{u}/health"))
                    .send()
                    .is_ok_and(|r| r.status() == 200)
                {
                    break u;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Server {
            d,
            url,
            _daemon: Daemon(child),
        }
    }

    /// The workspace's folder on the server.
    fn folder(&self, name: &str) -> PathBuf {
        self.d.path().join("durum/itero/workspaces").join(name)
    }

    /// `PUT /workspaces/{name}` with `files`: its status and answer.
    fn apply(&self, name: &str, files: &[(&str, &str, bool)], dry_run: bool) -> (u16, Value) {
        let body = json!({ "files": files.iter().map(|(path, text, executable)| json!({
            "path": path,
            "executable": executable,
            "content": base64::engine::general_purpose::STANDARD.encode(text),
        })).collect::<Vec<_>>() });
        let q = if dry_run { "?dry_run=true" } else { "" };
        let r = http()
            .put(format!("{}/workspaces/{name}{q}", self.url))
            .json(&body)
            .send()
            .unwrap();
        (r.status().as_u16(), r.json().unwrap_or(Value::Null))
    }

    fn get(&self, path: &str) -> Value {
        http()
            .get(format!("{}{path}", self.url))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }

    fn emit(&self, name: &str, kind: &str) -> Value {
        http()
            .post(format!("{}/ws/{name}/events", self.url))
            .json(&json!({ "type": kind, "data": {} }))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
}

/// A workspace whose `selam` workflow, on `selam.geldi`, runs `#yaz`,
/// which writes `out` (a path outside the workspace).
fn workspace(out: &Path) -> Vec<(String, String, bool)> {
    vec![
        ("itero.config.json".into(), "{}".into(), false),
        (
            "workflows/selam.json".into(),
            json!({ "on": [{ "event": "selam.geldi" }], "steps": ["#yaz"] }).to_string(),
            false,
        ),
        (
            "actions/yaz".into(),
            format!("#!/bin/sh\necho çay >> '{}'\n", out.display()),
            true,
        ),
    ]
}

fn borrowed(files: &[(String, String, bool)]) -> Vec<(&str, &str, bool)> {
    files
        .iter()
        .map(|(p, t, x)| (p.as_str(), t.as_str(), *x))
        .collect()
}

fn state_of(s: &Server, name: &str) -> Option<String> {
    s.get("/workspaces")
        .as_array()?
        .iter()
        .find_map(|w| (w["name"] == name).then(|| w["state"].as_str().unwrap_or("").to_string()))
}

#[test]
fn the_first_apply_makes_the_workspace_and_it_runs() {
    let s = Server::new();
    let out = s.d.path().join("çıktı");
    let files = workspace(&out);
    let (code, v) = s.apply("dükkan", &borrowed(&files), false);
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["created"], true, "{v}");
    let mut added: Vec<&str> = v["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    added.sort();
    assert_eq!(
        added,
        ["actions/yaz", "itero.config.json", "workflows/selam.json"]
    );
    // on the list, run by the daemon, under the name given
    until("dükkan running", || {
        state_of(&s, "dükkan").as_deref() == Some("running")
    });
    let folder = s.folder("dükkan");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(folder.join("actions/yaz"))
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "executable: {mode:o}");
    // its workflow runs
    let sent = s.emit("dükkan", "selam.geldi");
    assert_eq!(sent["starts"], json!(["selam"]), "{sent}");
    until("the action to write", || out.exists());
}

#[test]
fn an_event_right_after_an_apply_reaches_what_was_applied() {
    let s = Server::new();
    let out = s.d.path().join("çıktı");
    let files = workspace(&out);
    // the first apply: the workspace runs when its answer comes, so an
    // event sent at once reaches it
    assert_eq!(s.apply("dükkan", &borrowed(&files), false).0, 200);
    assert_eq!(state_of(&s, "dükkan").as_deref(), Some("running"));
    let sent = s.emit("dükkan", "selam.geldi");
    assert_eq!(sent["starts"], json!(["selam"]), "{sent}");
    until("selam's action to write", || out.exists());
    // a later apply adds a workflow: an event for it sent at once runs it
    let yeni = s.d.path().join("yeni");
    let mut more = files.clone();
    more.push((
        "workflows/yeni.json".into(),
        json!({ "on": [{ "event": "yeni.geldi" }], "steps": ["#yeni"] }).to_string(),
        false,
    ));
    more.push((
        "actions/yeni".into(),
        format!("#!/bin/sh\necho ışık > '{}'\n", yeni.display()),
        true,
    ));
    assert_eq!(s.apply("dükkan", &borrowed(&more), false).0, 200);
    let sent = s.emit("dükkan", "yeni.geldi");
    assert_eq!(sent["starts"], json!(["yeni"]), "{sent}");
    until("yeni's action to write", || yeni.exists());
}

#[test]
fn a_later_apply_mirrors_and_a_dry_run_writes_nothing() {
    let s = Server::new();
    let out = s.d.path().join("çıktı");
    let mut files = workspace(&out);
    files.push((
        "workflows/eski.json".into(),
        json!({ "on": [{ "event": "eski.geldi" }], "steps": ["#yaz"] }).to_string(),
        false,
    ));
    assert_eq!(s.apply("dükkan", &borrowed(&files), false).0, 200);
    until("dükkan running", || {
        state_of(&s, "dükkan").as_deref() == Some("running")
    });
    // the server's own .env and journal are its, never touched
    let folder = s.folder("dükkan");
    std::fs::write(folder.join(".env"), "ANAHTAR=sunucu\n").unwrap();
    // eski goes, selam changes, a new action comes
    files.retain(|(p, _, _)| p != "workflows/eski.json");
    files[1].1 =
        json!({ "on": [{ "event": "selam.geldi" }], "steps": ["#yaz", "#ek"] }).to_string();
    files.push(("actions/ek".into(), "#!/bin/sh\nexit 0\n".into(), true));
    let (code, dry) = s.apply("dükkan", &borrowed(&files), true);
    assert_eq!(code, 200, "{dry}");
    assert!(
        folder.join("workflows/eski.json").exists(),
        "a dry run writes nothing"
    );
    assert!(!folder.join("actions/ek").exists());
    let (code, v) = s.apply("dükkan", &borrowed(&files), false);
    assert_eq!(code, 200, "{v}");
    assert_eq!(v, dry, "a dry run says what the apply does");
    assert_eq!(v["created"], false, "{v}");
    assert_eq!(v["added"], json!(["actions/ek"]), "{v}");
    assert_eq!(v["changed"], json!(["workflows/selam.json"]), "{v}");
    assert_eq!(v["removed"], json!(["workflows/eski.json"]), "{v}");
    assert!(!folder.join("workflows/eski.json").exists());
    assert_eq!(
        std::fs::read_to_string(folder.join(".env")).unwrap(),
        "ANAHTAR=sunucu\n"
    );
    assert!(folder.join(".itero").is_dir());
    // the workflow gone starts nothing now
    until("eski gone from the daemon", || {
        s.emit("dükkan", "eski.geldi")["starts"] == json!([])
    });
    // the same files again: nothing to do
    let (_, v) = s.apply("dükkan", &borrowed(&files), false);
    assert_eq!(
        v,
        json!({ "created": false, "added": [], "changed": [], "removed": [] })
    );
}

#[test]
fn what_cannot_be_a_workspace_is_refused_every_problem_at_once() {
    let s = Server::new();
    let (code, v) = s.apply(
        "dükkan",
        &[
            ("../dışarı", "x", false),
            ("/mutlak", "x", false),
            (".itero/itero.db", "x", false),
            (".env", "ANAHTAR=x", false),
            ("workflows/selam.json", "{}", false),
        ],
        false,
    );
    assert_eq!(code, 400, "{v}");
    let m = v["error"].as_str().unwrap();
    for problem in [
        "../dışarı: a path inside the workspace",
        "/mutlak: a path inside the workspace",
        ".itero/itero.db: the server's own",
        ".env: the server's own",
        "itero.config.json: not among the files",
    ] {
        assert!(m.contains(problem), "{problem}: {m}");
    }
    assert!(!s.folder("dükkan").exists(), "nothing made");
    // a name `itero ws <name> ...` could not tell from a command
    let (code, v) = s.apply("list", &[("itero.config.json", "{}", false)], false);
    assert_eq!(code, 400, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("list: a ws command's name"),
        "{v}"
    );
}

impl Server {
    /// `itero <args>` in `dir`, as this machine's user: its output.
    fn itero(&self, dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_itero"))
            .current_dir(dir)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_CONFIG_HOME", self.d.path().join("ayar"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_TOKEN", "")
            .stdin(Stdio::null())
            .args(args)
            .output()
            .unwrap()
    }

    /// A workspace being made in this machine's `projeler/<name>`, its
    /// files `files`, and a context `sunucu` to this server for it.
    fn local(&self, name: &str, files: &[(String, String, bool)]) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let w = self.d.path().join("projeler").join(name);
        for (p, text, executable) in files {
            let f = w.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, text).unwrap();
            if *executable {
                std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let o = self.itero(&w, &["context", "add", "sunucu", &self.url, "--ws", name]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        w
    }
}

fn said(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn ws_apply_sends_the_folder_and_mirrors_it() {
    let s = Server::new();
    let out = s.d.path().join("çıktı");
    let w = s.local("dükkan", &workspace(&out));
    // this machine's own, never sent
    std::fs::write(w.join(".env"), "ANAHTAR=yerel\n").unwrap();
    let o = s.itero(&w, &["ws", "apply", "--context", "sunucu"]);
    let text = said(&o);
    assert!(o.status.success(), "{text}");
    assert!(text.contains("created dükkan on sunucu"), "{text}");
    assert!(text.contains("+ actions/yaz"), "{text}");
    let folder = s.folder("dükkan");
    assert!(folder.join("workflows/selam.json").is_file());
    assert!(!folder.join(".env").exists(), "the .env stayed here");
    until("dükkan running", || {
        state_of(&s, "dükkan").as_deref() == Some("running")
    });
    // an edit, a removal: a dry run says, the apply does
    std::fs::remove_file(w.join("actions/yaz")).unwrap();
    std::fs::write(
        w.join("workflows/selam.json"),
        json!({ "on": [{ "event": "selam.geldi" }], "steps": ["#notify"] }).to_string(),
    )
    .unwrap();
    let o = s.itero(&w, &["ws", "apply", "--dry-run", "--context", "sunucu"]);
    let text = said(&o);
    assert!(
        text.contains("- actions/yaz") && text.contains("~ workflows/selam.json"),
        "{text}"
    );
    assert!(text.contains("dry run: nothing written"), "{text}");
    assert!(folder.join("actions/yaz").exists());
    let o = s.itero(&w, &["ws", "apply", "--context", "sunucu", "--json"]);
    assert!(o.status.success(), "{}", said(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["removed"], json!(["actions/yaz"]), "{v}");
    assert_eq!(v["changed"], json!(["workflows/selam.json"]), "{v}");
    assert!(!folder.join("actions/yaz").exists());
    let o = s.itero(&w, &["ws", "apply", "--context", "sunucu"]);
    assert!(
        said(&o).contains("dükkan on sunucu is up to date"),
        "{}",
        said(&o)
    );
}

#[test]
fn ws_apply_needs_a_context_and_refuses_what_cannot_travel() {
    let s = Server::new();
    let out = s.d.path().join("çıktı");
    let w = s.local("dükkan", &workspace(&out));
    // with no context: the folder is the workspace already
    let o = s.itero(&w, &["ws", "apply"]);
    assert!(!o.status.success());
    assert!(said(&o).contains("--context"), "{}", said(&o));
    // a symlink points at this machine: refused, with every other problem
    std::os::unix::fs::symlink("/etc/hosts", w.join("bağ")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", w.join("actions/öteki")).unwrap();
    let o = s.itero(&w, &["ws", "apply", "--context", "sunucu"]);
    let text = said(&o);
    assert!(!o.status.success(), "{text}");
    assert!(
        text.contains("bağ: a symlink") && text.contains("actions/öteki: a symlink"),
        "{text}"
    );
    assert!(!s.folder("dükkan").exists(), "nothing sent");
}
