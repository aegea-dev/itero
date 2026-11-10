//! One itero per machine: the machine's list of workspaces, and the one
//! daemon that runs them. Every command runs with HOME and
//! XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use itero::engine::journal::Journal;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

fn until(what: &str, f: impl Fn() -> bool) {
    assert!(within(20, f), "waited 20s for {what}");
}

/// Whether `f` comes true within `secs` seconds.
fn within(secs: u64, f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// Seconds for what starts a real itero-ai. On a Mac a start may first
/// compile its Metal shaders, 15 seconds and more, and itero gives it 60 to
/// answer.
const ITERO_AI_STARTS: u64 = 90;

struct Machine {
    d: tempfile::TempDir,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            d: tempfile::tempdir().unwrap(),
        }
    }

    /// A workspace, `{}` its manifest, in a folder of this machine.
    fn workspace(&self, name: &str) -> PathBuf {
        let w = self.d.path().join("projeler").join(name);
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("itero.config.json"), "{}").unwrap();
        // as the commands see it (macOS's /var is /private/var)
        w.canonicalize().unwrap()
    }

    /// A workspace whose workflow, on `deneme.geldi`, writes the file
    /// `çıktı` in it.
    fn busy_workspace(&self, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let w = self.workspace(name);
        std::fs::write(
            w.join("itero.config.json"),
            r#"{"workflow": {"keep": {"off": true}, "models": {"off": true}}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(w.join("workflows")).unwrap();
        std::fs::create_dir_all(w.join("actions")).unwrap();
        std::fs::write(
            w.join("workflows/yaz.json"),
            r##"{"on": [{"event": "deneme.geldi"}], "steps": ["#yaz"]}"##,
        )
        .unwrap();
        let action = w.join("actions/yaz");
        std::fs::write(
            &action,
            format!("#!/bin/sh\necho tamam > '{}'\n", w.join("çıktı").display()),
        )
        .unwrap();
        std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).unwrap();
        w
    }

    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(dir)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .env(
                "ITERO_AI_BIN",
                Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai"),
            )
            .env_remove("ITERO_AI_SOCKET");
        c
    }

    /// A workspace whose workflow, on `soru.geldi`, asks a model this
    /// machine does not have.
    fn ai_workspace(&self, name: &str) -> PathBuf {
        let w = self.busy_workspace(name);
        std::fs::write(
            w.join("workflows/sor.json"),
            r##"{"on": [{"event": "soru.geldi"}],
                 "steps": [{"#ai": {"args": {"model": "yok/Yok-GGUF", "message": "merhaba"}}}]}"##,
        )
        .unwrap();
        w
    }

    /// The `itero.ai.*` events in a workspace's journal, with their data.
    fn ai_events(&self, dir: &Path) -> Vec<(String, Value)> {
        self.journal(dir)
            .events(None)
            .unwrap()
            .into_iter()
            .filter(|r| r.event.kind.starts_with("itero.ai."))
            .map(|r| (r.event.kind, r.event.data))
            .collect()
    }

    /// `itero daemon --all`, from this machine's own folder.
    fn daemon_all(&self) -> Daemon {
        let child = self
            .command(self.d.path())
            .args(["daemon"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Daemon(child)
    }

    /// A `server.json` of this machine's own saying `port`, for `--config`.
    fn server_file(&self, port: u16) -> String {
        let p = self.d.path().join(format!("sunucu-{port}.json"));
        std::fs::write(&p, format!(r#"{{ "port": {port} }}"#)).unwrap();
        p.display().to_string()
    }

    /// `itero daemon --all` on `port` (its `server.json`).
    fn daemon_all_on(&self, port: u16) -> Daemon {
        let child = self
            .command(self.d.path())
            .args(["daemon", "--config", &self.server_file(port)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Daemon(child)
    }

    /// `itero daemon --all` on `port`, its stderr kept in a file.
    fn daemon_all_said(&self, port: u16, said: &Path) -> Daemon {
        let child = self
            .command(self.d.path())
            .args(["daemon", "--config", &self.server_file(port)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(said).unwrap())
            .spawn()
            .unwrap();
        Daemon(child)
    }

    /// The machine daemon's port, once it serves.
    fn port(&self) -> u16 {
        let f = self.d.path().join("durum/itero/port");
        until("the machine's port", || {
            std::fs::read_to_string(&f).is_ok_and(|p| p.trim().parse::<u16>().is_ok())
        });
        std::fs::read_to_string(&f).unwrap().trim().parse().unwrap()
    }

    fn ws(&self, dir: &Path) -> itero::workspace::Workspace {
        itero::workspace::find(dir).unwrap()
    }

    fn run(&self, dir: &Path, args: &[&str]) -> (bool, String) {
        let out = self.command(dir).args(args).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let (ok, out) = self.run(dir, args);
        assert!(ok, "itero {args:?}: {out}");
        out
    }

    fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let out = self.ok(dir, args);
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
    }

    fn journal(&self, dir: &Path) -> Journal {
        Journal::open(&itero::workspace::find(dir).unwrap()).unwrap()
    }

    fn journal_name(&self, dir: &Path) -> String {
        self.journal(dir).workspace().unwrap().name
    }
}

#[test]
fn add_lists_a_workspace_by_its_journal_name_and_remove_takes_it_off() {
    let m = Machine::new();
    let ev = m.workspace("ev");
    let out = m.ok(&ev, &["workspace", "add"]);
    let name = m.journal_name(&ev);
    assert!(out.contains(&name), "{out}");
    assert!(out.contains("itero service install"), "{out}");
    let v = m.json(&ev, &["workspace", "list", "--json"]);
    assert_eq!(v[0]["name"], name.as_str());
    assert_eq!(v[0]["folder"], ev.display().to_string());
    assert_eq!(v[0]["state"], "stopped");
    // `ws` is short for `workspace`, as `wf` is for `workflow`
    assert_eq!(m.json(&ev, &["ws", "list", "--json"]), v);
    let table = m.ok(&ev, &["workspace", "list"]);
    assert!(
        table.contains(&name) && table.contains("stopped"),
        "{table}"
    );
    m.ok(&ev, &["workspace", "remove"]);
    assert_eq!(
        m.json(&ev, &["workspace", "list", "--json"]),
        Value::Array(vec![])
    );
    let (ok, out) = m.run(&ev, &["workspace", "remove"]);
    assert!(!ok && out.contains("not on this machine's list"), "{out}");
}

#[test]
fn a_name_taken_on_the_machine_is_replaced_in_the_newer_journal() {
    let m = Machine::new();
    let (a, b) = (m.workspace("bir"), m.workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    let taken = m.journal_name(&a);
    m.journal(&b).rename_workspace(&taken).unwrap();
    let out = m.ok(&b, &["workspace", "add"]);
    assert_ne!(m.journal_name(&b), taken);
    assert!(out.contains("taken"), "{out}");
    let v = m.json(&b, &["workspace", "list", "--json"]);
    assert_eq!(v.as_array().unwrap().len(), 2);
    // removed by name, from any folder
    m.ok(&a, &["workspace", "remove", &m.journal_name(&b)]);
    let v = m.json(&a, &["workspace", "list", "--json"]);
    assert_eq!(v[0]["name"], taken.as_str());
}

#[test]
fn a_moved_folder_keeps_its_entry_and_a_gone_one_is_missing() {
    let m = Machine::new();
    let ev = m.workspace("ev");
    m.ok(&ev, &["workspace", "add"]);
    let moved = ev.with_file_name("taşındı");
    std::fs::rename(&ev, &moved).unwrap();
    let v = m.json(&moved, &["workspace", "list", "--json"]);
    assert_eq!(v[0]["state"], "missing");
    m.ok(&moved, &["workspace", "add"]);
    let v = m.json(&moved, &["workspace", "list", "--json"]);
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["folder"], moved.display().to_string());
    assert_eq!(v[0]["state"], "stopped");
}

#[test]
fn all_runs_each_listed_workspace_and_follows_the_list() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    let _d = m.daemon_all();
    until("a to be run", || {
        itero::engine::lock::take(&m.ws(&a), "deneme").is_err()
    });
    m.ok(&a, &["ws", "emit", "deneme.geldi"]);
    until("a's workflow", || a.join("çıktı").is_file());
    // added while it runs: started
    m.ok(&b, &["workspace", "add"]);
    until("b to be run", || {
        itero::engine::lock::take(&m.ws(&b), "deneme").is_err()
    });
    m.ok(&b, &["ws", "emit", "deneme.geldi"]);
    until("b's workflow", || b.join("çıktı").is_file());
    // removed while it runs: stopped, its lock free
    m.ok(&a, &["workspace", "remove"]);
    until("a to be stopped", || {
        itero::engine::lock::take(&m.ws(&a), "deneme").is_ok()
    });
    // and b runs on
    std::fs::remove_file(b.join("çıktı")).unwrap();
    m.ok(&b, &["ws", "emit", "deneme.geldi"]);
    until("b's workflow again", || b.join("çıktı").is_file());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http() -> reqwest::blocking::Client {
    itero::net::client(Duration::from_secs(5)).unwrap()
}

/// A GET's status and body; status 0 while nothing answers yet.
fn get(port: u16, path: &str) -> (u16, Value) {
    match http().get(format!("http://127.0.0.1:{port}{path}")).send() {
        Ok(r) => (r.status().as_u16(), r.json().unwrap_or(Value::Null)),
        Err(_) => (0, Value::Null),
    }
}

#[test]
fn one_api_for_the_machine_each_workspace_under_its_name() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    m.ok(&b, &["workspace", "add"]);
    let (na, nb) = (m.journal_name(&a), m.journal_name(&b));
    let port = free_port();
    let _d = m.daemon_all_on(port);
    assert_eq!(m.port(), port);
    until("both running", || {
        let (_, v) = get(port, "/workspaces");
        v.as_array()
            .is_some_and(|l| l.len() == 2 && l.iter().all(|w| w["state"] == "running"))
    });
    // an event posted to one workspace runs there only
    let r = http()
        .post(format!("http://127.0.0.1:{port}/ws/{na}/events"))
        .json(&serde_json::json!({ "type": "deneme.geldi" }))
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 202);
    until("a's workflow", || a.join("çıktı").is_file());
    assert!(!b.join("çıktı").exists());
    let (code, runs) = get(port, &format!("/ws/{na}/runs"));
    assert_eq!(code, 200, "{runs}");
    assert_eq!(runs.as_array().map(Vec::len), Some(1), "{runs}");
    let (code, h) = get(port, &format!("/ws/{nb}/health"));
    assert_eq!(
        (code, &h["workspace"]["name"]),
        (200, &Value::from(nb.as_str()))
    );
    let (code, h) = get(port, "/health");
    assert_eq!(code, 200);
    assert_eq!(
        h["workspaces"][&na]["passes"].as_u64().map(|p| p >= 1),
        Some(true),
        "{h}"
    );
    let (code, e) = get(port, "/ws/yok/runs");
    assert_eq!(code, 404);
    assert!(
        e["error"]
            .as_str()
            .unwrap()
            .contains("itero workspace list"),
        "{e}"
    );
    // only this machine's own address
    let r = http()
        .get(format!("http://127.0.0.1:{port}/workspaces"))
        .header("Host", "kotu.example")
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);
    // the list asks the daemon
    let v = m.json(&a, &["workspace", "list", "--json"]);
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .all(|w| w["state"] == "running"),
        "{v}"
    );
}

#[test]
fn a_workspace_run_in_the_foreground_is_left_to_it() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    m.ok(&b, &["workspace", "add"]);
    let mut own = m.command(&a);
    own.args(["ws", "daemon"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let fg = Daemon(own.spawn().unwrap());
    until("a's own daemon", || a.join(".itero/port").is_file());
    let port = free_port();
    let _d = m.daemon_all_on(port);
    let na = m.journal_name(&a);
    until("a stopped, b running", || {
        let (_, v) = get(port, "/workspaces");
        let state = |n: &str| {
            v.as_array()
                .and_then(|l| l.iter().find(|w| w["name"] == n).cloned())
                .map(|w| w["state"].clone())
        };
        state(&na) == Some("stopped".into()) && state(&m.journal_name(&b)) == Some("running".into())
    });
    let (_, v) = get(port, "/workspaces");
    let wa = v
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == na.as_str())
        .unwrap();
    assert!(
        wa["why"]
            .as_str()
            .unwrap()
            .contains(&format!("pid {}", fg.0.id())),
        "{wa}"
    );
}

#[test]
fn a_stuck_workspace_is_told_and_the_others_run_on() {
    use std::os::unix::fs::PermissionsExt;
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    // a's step takes four seconds: past ITERO_STUCK_AFTER
    std::fs::write(
        a.join("actions/yaz"),
        format!(
            "#!/bin/sh\nsleep 4\necho tamam > '{}'\n",
            a.join("çıktı").display()
        ),
    )
    .unwrap();
    // b hears of it, with the stuck one's name
    std::fs::write(
        b.join("workflows/duyar.json"),
        r##"{"on": [{"event": "itero.workspace.stuck"}],
             "steps": [{"#not": {"args": {"workspace": "$event.workspace"}}}]}"##,
    )
    .unwrap();
    let not = b.join("actions/not");
    std::fs::write(
        &not,
        format!(
            "#!/bin/sh\necho \"$workspace\" >> '{}'\n",
            b.join("takıldı").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&not, std::fs::Permissions::from_mode(0o755)).unwrap();
    m.ok(&a, &["workspace", "add"]);
    m.ok(&b, &["workspace", "add"]);
    let na = m.journal_name(&a);
    let port = free_port();
    let child = m
        .command(m.d.path())
        .args(["daemon", "--config", &m.server_file(port)])
        .env("ITERO_STUCK_AFTER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _d = Daemon(child);
    let state = |n: &str| {
        let (_, v) = get(port, "/workspaces");
        v.as_array()
            .and_then(|l| l.iter().find(|w| w["name"] == n).cloned())
            .map(|w| w["state"].clone())
    };
    until("both running", || {
        state(&na) == Some("running".into()) && state(&m.journal_name(&b)) == Some("running".into())
    });
    m.ok(&a, &["ws", "emit", "deneme.geldi"]);
    until("a stuck", || state(&na) == Some("stuck".into()));
    // on a busy machine a pass of b's can pass a second too, and b hears of
    // itself as well
    until("b to hear of a", || {
        std::fs::read_to_string(b.join("takıldı"))
            .unwrap_or_default()
            .lines()
            .any(|l| l == na)
    });
    // b runs on meanwhile
    m.ok(&b, &["ws", "emit", "deneme.geldi"]);
    until("b's workflow", || b.join("çıktı").is_file());
    // a's step ends: running again
    until("a's workflow", || a.join("çıktı").is_file());
    until("a running again", || state(&na) == Some("running".into()));
}

#[test]
fn info_in_a_listed_workspace_names_the_machines_daemon() {
    let m = Machine::new();
    let a = m.busy_workspace("bir");
    m.ok(&a, &["workspace", "add"]);
    let na = m.journal_name(&a);
    let port = free_port();
    let _d = m.daemon_all_on(port);
    until("a running", || {
        get(port, &format!("/ws/{na}/health")).0 == 200
    });
    let v = m.json(&a, &["ws", "info", "--json"]);
    let d = &v["daemon"];
    assert_eq!(d["running"], true, "{v}");
    assert_eq!(d["run_by"], "machine", "{v}");
    let api = format!("http://127.0.0.1:{port}/ws/{na}/");
    assert_eq!(d["api"], api.as_str(), "{v}");
    // the route under the machine's API says the same
    let (code, route) = get(port, &format!("/ws/{na}/info"));
    assert_eq!(code, 200, "{route}");
    assert_eq!(route, v);
    let text = m.ok(&a, &["ws", "info"]);
    assert!(
        text.contains(&api) && text.contains("the machine's daemon"),
        "{text}"
    );
    // `itero ws emit --wait` waits on the same daemon
    let out = m.ok(&a, &["ws", "emit", "deneme.geldi", "--wait"]);
    assert!(a.join("çıktı").is_file(), "{out}");
}

#[test]
fn the_machines_api_passes_an_encoded_path_on_as_it_came() {
    use std::os::unix::fs::PermissionsExt;
    let m = Machine::new();
    let a = m.busy_workspace("bir");
    let gönder = a.join("actions/gönder");
    std::fs::write(&gönder, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&gönder, std::fs::Permissions::from_mode(0o755)).unwrap();
    m.ok(&a, &["workspace", "add"]);
    let na = m.journal_name(&a);
    let port = free_port();
    let _d = m.daemon_all_on(port);
    until("a running", || {
        get(port, &format!("/ws/{na}/health")).0 == 200
    });
    // `#` and a letter past ASCII, each percent-encoded in the path, reach
    // the workspace as they were sent
    for (name, path) in [("#yaz", "%23yaz"), ("gönder", "g%C3%B6nder")] {
        let (code, route) = get(port, &format!("/ws/{na}/which/{path}"));
        assert_eq!(code, 200, "{name}: {route}");
        assert_eq!(
            route,
            m.json(&a, &["ws", "which", name, "--json"]),
            "{name}"
        );
    }
    // a route with a parameter of its own sees only that one
    let (code, v) = get(port, &format!("/ws/{na}/runs/yok"));
    assert_eq!(code, 404, "{v}");
    assert!(v["error"].as_str().unwrap().contains("no run yok"), "{v}");
}

#[test]
fn one_itero_ai_for_every_workspace_of_the_daemon() {
    let bin = Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai");
    assert!(
        bin.is_file(),
        "{} is not built: cargo build -p itero-ai",
        bin.display()
    );
    let m = Machine::new();
    let (a, b) = (m.ai_workspace("bir"), m.ai_workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    m.ok(&b, &["workspace", "add"]);
    let port = free_port();
    let _d = m.daemon_all_on(port);
    let na = m.journal_name(&a);
    until("both running", || {
        let (_, v) = get(port, "/workspaces");
        v.as_array()
            .is_some_and(|l| l.len() == 2 && l.iter().all(|w| w["state"] == "running"))
    });
    let kinds = |dir: &Path, kind: &str| m.ai_events(dir).iter().filter(|(k, _)| k == kind).count();
    m.ok(&a, &["ws", "emit", "soru.geldi"]);
    assert!(
        within(ITERO_AI_STARTS, || {
            kinds(&a, "itero.ai.model.missing") == 1
        }),
        "a's step did not ask: {:?}",
        m.ai_events(&a)
    );
    // itero-ai started once, told to both; the missing model only to a
    assert_eq!(kinds(&a, "itero.ai.started"), 1);
    until("b to hear", || kinds(&b, "itero.ai.started") == 1);
    assert_eq!(kinds(&b, "itero.ai.model.missing"), 0);
    let (_, started) = m
        .ai_events(&a)
        .into_iter()
        .find(|(k, _)| k == "itero.ai.started")
        .unwrap();
    let socket = started["socket"].as_str().unwrap().to_string();
    // b's step asks the same itero-ai: started no second time
    m.ok(&b, &["ws", "emit", "soru.geldi"]);
    until("b's step to ask", || {
        kinds(&b, "itero.ai.model.missing") == 1
    });
    assert_eq!(kinds(&a, "itero.ai.started"), 1);
    assert_eq!(kinds(&b, "itero.ai.started"), 1);
    assert_eq!(kinds(&a, "itero.ai.model.missing"), 1);
    // one socket for the machine, not one per workspace, and this
    // machine's state folder's own: not another daemon's
    assert!(!socket.contains(&na), "{socket}");
    let state = m.d.path().join("durum/itero");
    assert_eq!(Path::new(&socket), itero::ai::socket_for(&state));
}

#[test]
fn a_listed_workspace_has_no_models_workflow_of_its_own() {
    let m = Machine::new();
    let (a, b) = (m.workspace("bir"), m.workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    let names = |dir: &Path| {
        m.json(dir, &["ws", "wf", "list", "--json"])["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert!(
        !names(&a).contains(&"models".to_string()),
        "{:?}",
        names(&a)
    );
    assert!(names(&b).contains(&"models".to_string()), "{:?}", names(&b));
}

#[test]
fn the_daemon_scans_for_models_once_and_when_a_step_asks() {
    let bin = Path::new(env!("CARGO_BIN_EXE_itero")).with_file_name("itero-ai");
    assert!(
        bin.is_file(),
        "{} is not built: cargo build -p itero-ai",
        bin.display()
    );
    let m = Machine::new();
    // a model in LM Studio's folder: a real GGUF file, its vocabulary only
    let lm = m.d.path().join("ev/.lmstudio/models/örnek/Sözlük-GGUF");
    std::fs::create_dir_all(&lm).unwrap();
    let vocab = Path::new(env!("CARGO_MANIFEST_DIR")).join("itero-ai/tests/data/vocab.gguf");
    std::fs::copy(&vocab, lm.join("Sözlük-F16.gguf")).unwrap();
    let (a, b) = (m.ai_workspace("bir"), m.ai_workspace("iki"));
    m.ok(&a, &["workspace", "add"]);
    m.ok(&b, &["workspace", "add"]);
    let said = m.d.path().join("daemon.err");
    let scans = || {
        std::fs::read_to_string(&said)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("itero: models:"))
            .count()
    };
    let _d = m.daemon_all_said(free_port(), &said);
    let told = || std::fs::read_to_string(&said).unwrap_or_default();
    assert!(
        within(ITERO_AI_STARTS, || scans() == 1),
        "no scan at its start: {}",
        told()
    );
    let index = m.d.path().join("önbellek/itero/models.json");
    let ix: Value = serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(ix["models"][0]["repo"], "örnek/Sözlük-GGUF", "{ix}");
    // two workspaces, one scan
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(scans(), 1);
    // a step asks for a model the list has not: one more
    m.ok(&a, &["ws", "emit", "soru.geldi"]);
    assert!(
        within(ITERO_AI_STARTS, || scans() == 2),
        "no scan when asked: {}\n{:?}",
        told(),
        m.ai_events(&a)
    );
}

#[test]
fn emit_reaches_every_workspace_on_the_list_and_ws_emit_one() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["ws", "add"]);
    m.ok(&b, &["ws", "add"]);
    let (na, nb) = (m.journal_name(&a), m.journal_name(&b));
    // no daemon: each waits in its journal, and is said by name
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = m.ok(
        &elsewhere,
        &["emit", "deneme.geldi", r#"{"şehir": "İzmir"}"#],
    );
    let said = |journal: &Journal| {
        journal
            .events(None)
            .unwrap()
            .into_iter()
            .filter(|r| r.event.kind == "deneme.geldi")
            .map(|r| (r.event.id, r.event.data["şehir"].clone()))
            .collect::<Vec<_>>()
    };
    for (dir, name) in [(&a, &na), (&b, &nb)] {
        let got = said(&m.journal(dir));
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].1, "İzmir");
        let line = format!("{name} {}", got[0].0);
        assert!(out.lines().any(|l| l == line), "{line}: {out}");
    }
    // --wait: nothing sent while no daemon runs them
    let (ok, why) = m.run(&elsewhere, &["emit", "deneme.geldi", "--wait"]);
    assert!(!ok && why.contains(&na) && why.contains(&nb), "{why}");
    assert_eq!(said(&m.journal(&a)).len(), 1);
    // one workspace, by the folder or by name
    m.ok(&a, &["ws", "emit", "deneme.geldi"]);
    m.ok(&elsewhere, &["ws", &na, "emit", "deneme.geldi"]);
    assert_eq!(said(&m.journal(&a)).len(), 3);
    assert_eq!(said(&m.journal(&b)).len(), 1);
    // one that is missing from its folder is said, and fails nothing: an
    // action's itero emit still ends well
    let moved = b.with_file_name("gitti");
    std::fs::rename(&b, &moved).unwrap();
    let (ok, out) = m.run(&elsewhere, &["emit", "deneme.geldi"]);
    assert!(
        ok && out.contains(&format!("{nb}: no workspace in")),
        "{out}"
    );
    assert_eq!(said(&m.journal(&a)).len(), 4);
    // a type that may not be sent reaches none
    let (ok, _) = m.run(&elsewhere, &["emit", "tek"]);
    assert!(!ok);
    assert_eq!(said(&m.journal(&a)).len(), 4);
    // every one missing: nothing was sent, and that fails
    std::fs::rename(&a, a.with_file_name("gitti-o-da")).unwrap();
    let (ok, out) = m.run(&elsewhere, &["emit", "deneme.geldi"]);
    assert!(
        !ok && out.contains(&format!("{na}: no workspace in")),
        "{out}"
    );
}

#[test]
fn emit_reads_its_file_from_the_folder_it_runs_in() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["ws", "add"]);
    m.ok(&b, &["ws", "add"]);
    let na = m.journal_name(&a);
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("istek.json"), r#"{"şehir": "İzmir"}"#).unwrap();
    let cities = |dir: &Path| {
        m.journal(dir)
            .events(None)
            .unwrap()
            .into_iter()
            .filter(|r| r.event.kind == "deneme.geldi")
            .map(|r| r.event.data["şehir"].clone())
            .collect::<Vec<_>>()
    };
    // --ws names the workspace; the file is still this folder's
    let file = ["--file", "istek.json", "deneme.geldi"];
    m.ok(&elsewhere, &[&["ws", &na, "emit"][..], &file].concat());
    assert_eq!(cities(&a), ["İzmir"]);
    assert!(cities(&b).is_empty());
    // itero emit: every workspace gets it
    m.ok(&elsewhere, &[&["emit"][..], &file].concat());
    assert_eq!(cities(&a), ["İzmir", "İzmir"]);
    assert_eq!(cities(&b), ["İzmir"]);
}

#[test]
fn emit_with_an_empty_list_says_how_to_fill_it() {
    let m = Machine::new();
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let (ok, out) = m.run(&elsewhere, &["emit", "deneme.geldi"]);
    assert!(
        !ok && out.contains("itero ws add") && out.contains("itero ws emit"),
        "{out}"
    );
}

#[test]
fn emit_wait_waits_for_every_workspaces_runs() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["ws", "add"]);
    m.ok(&b, &["ws", "add"]);
    let (na, nb) = (m.journal_name(&a), m.journal_name(&b));
    let port = free_port();
    let _d = m.daemon_all_on(port);
    assert_eq!(m.port(), port);
    until("both running", || {
        let (_, v) = get(port, "/workspaces");
        v.as_array()
            .is_some_and(|l| l.len() == 2 && l.iter().all(|w| w["state"] == "running"))
    });
    let v = m.json(&a, &["emit", "deneme.geldi", "--wait", "--timeout", "15s"]);
    for name in [&na, &nb] {
        assert_eq!(v[name]["runs"].as_array().map(Vec::len), Some(1), "{v}");
    }
    assert!(a.join("çıktı").is_file() && b.join("çıktı").is_file());
}

#[test]
fn the_machines_api_sends_an_event_to_every_workspace() {
    let m = Machine::new();
    let (a, b) = (m.busy_workspace("bir"), m.busy_workspace("iki"));
    m.ok(&a, &["ws", "add"]);
    m.ok(&b, &["ws", "add"]);
    let (na, nb) = (m.journal_name(&a), m.journal_name(&b));
    let port = free_port();
    let _d = m.daemon_all_on(port);
    assert_eq!(m.port(), port);
    until("both running", || {
        let (_, v) = get(port, "/workspaces");
        v.as_array()
            .is_some_and(|l| l.len() == 2 && l.iter().all(|w| w["state"] == "running"))
    });
    let post = |path: &str, body: Value| {
        http()
            .post(format!("http://127.0.0.1:{port}{path}"))
            .json(&body)
            .send()
            .unwrap()
    };
    // a bad type, and wait, send nothing
    let r = post("/events", serde_json::json!({ "type": "tek" }));
    assert_eq!(r.status().as_u16(), 400);
    let r = post(
        "/events?wait=5",
        serde_json::json!({ "type": "deneme.geldi" }),
    );
    assert_eq!(r.status().as_u16(), 400);
    assert!(!a.join("çıktı").exists());
    let r = post("/events", serde_json::json!({ "type": "deneme.geldi" }));
    assert_eq!(r.status().as_u16(), 202);
    let v: Value = r.json().unwrap();
    for (dir, name) in [(&a, &na), (&b, &nb)] {
        assert!(v["ids"][name.as_str()].is_string(), "{v}");
        until("its workflow", || dir.join("çıktı").is_file());
    }
    // none can take it: not accepted, and each says why
    for dir in [&a, &b] {
        let gone = dir.with_file_name(format!("{}-gitti", dir.file_name().unwrap().display()));
        std::fs::rename(dir, gone).unwrap();
    }
    let r = post("/events", serde_json::json!({ "type": "deneme.geldi" }));
    assert_eq!(r.status().as_u16(), 409);
    let v: Value = r.json().unwrap();
    assert_eq!(v["ids"], serde_json::json!({}), "{v}");
    for name in [&na, &nb] {
        assert!(v["errors"][name.as_str()].is_string(), "{v}");
    }
}

#[test]
fn the_machines_api_with_an_empty_list_sends_nothing() {
    let m = Machine::new();
    let port = free_port();
    let _d = m.daemon_all_on(port);
    until("it answers", || get(port, "/health").0 == 200);
    let r = http()
        .post(format!("http://127.0.0.1:{port}/events"))
        .json(&serde_json::json!({ "type": "deneme.geldi" }))
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 409);
    let v: Value = r.json().unwrap();
    assert!(
        v["error"]
            .as_str()
            .is_some_and(|e| e.contains("itero ws add")),
        "{v}"
    );
}

/// A server on a free port that gives every request `health`, for as long
/// as the test runs: a daemon of another itero.
fn fake_health(health: Value) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let mut s = s.unwrap();
            let mut asked = [0u8; 1024];
            let _ = s.read(&mut asked);
            let body = health.to_string();
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

#[test]
fn info_says_no_daemon_answers_from_any_folder() {
    let m = Machine::new();
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = m.ok(&elsewhere, &["info"]);
    let this = format!("itero {}", env!("CARGO_PKG_VERSION"));
    assert_eq!(out.lines().next(), Some(this.as_str()), "{out}");
    assert!(
        out.contains("daemon: none answering: no port yet") && out.contains("itero service status"),
        "{out}"
    );
    let state = m.d.path().join("durum/itero");
    std::fs::create_dir_all(&state).unwrap();
    let port = free_port();
    std::fs::write(state.join("port"), port.to_string()).unwrap();
    let out = m.ok(&elsewhere, &["info"]);
    assert!(
        out.contains(&format!("daemon: none answering on port {port}")),
        "{out}"
    );
    let v = m.json(&elsewhere, &["info", "--json"]);
    assert_eq!(v["itero"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["port"], port);
    assert_eq!(v["daemon"], Value::Null);
}

#[test]
fn info_shows_the_machines_daemon_and_its_workspaces() {
    let m = Machine::new();
    let a = m.busy_workspace("bir");
    m.ok(&a, &["ws", "add"]);
    let na = m.journal_name(&a);
    let port = free_port();
    let _d = m.daemon_all_on(port);
    until("a running", || {
        get(port, &format!("/ws/{na}/health")).0 == 200
    });
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let v = m.json(&elsewhere, &["info", "--json"]);
    assert_eq!(v["itero"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["daemon"]["itero"], env!("CARGO_PKG_VERSION"), "{v}");
    assert_eq!(v["port"], port, "{v}");
    assert!(v["workspaces"][&na]["passes"].is_u64(), "{v}");
    assert!(v["notifications"]["state"].is_string(), "{v}");
    let out = m.ok(&elsewhere, &["info"]);
    assert!(out.contains(&format!("port {port}")), "{out}");
    assert!(
        out.lines().any(|l| l.starts_with(&format!("  {na}: "))),
        "{out}"
    );
    assert!(!out.contains("itero service restart"), "{out}");
}

#[test]
fn info_says_when_the_daemon_runs_another_itero() {
    let m = Machine::new();
    let port = fake_health(serde_json::json!({
        "ok": true,
        "pid": 7,
        "daemon": { "pid": 7, "itero": "0.0.1", "started": "2026-10-31T09:00:00Z" },
        "workspaces": {
            "pala-su": { "passes": 3, "last_pass": "2026-10-31T09:01:00Z", "last_error": "kötü" }
        },
        "ai": { "state": "running", "model": "yerel/küçük", "failures": 0 },
    }));
    let state = m.d.path().join("durum/itero");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("port"), port.to_string()).unwrap();
    let elsewhere = m.d.path().join("başka");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = m.ok(&elsewhere, &["info"]);
    for says in [
        "itero 0.0.1",
        "itero service restart",
        "pala-su: 3 passes",
        "last error: kötü",
        "ai: running yerel/küçük",
    ] {
        assert!(out.contains(says), "{says}: {out}");
    }
}

/// A machine's daemon standing in: `/health` and `/workspaces` answer
/// `health` and `workspaces`, at the port the commands read.
fn fake_machine(m: &Machine, health: Value, workspaces: Value) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let mut s = s.unwrap();
            let mut asked = [0u8; 1024];
            let n = s.read(&mut asked).unwrap_or(0);
            let asked = String::from_utf8_lossy(&asked[..n]);
            let body = if asked.starts_with("GET /workspaces") {
                &workspaces
            } else {
                &health
            }
            .to_string();
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let state = m.d.path().join("durum/itero");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("port"), port.to_string()).unwrap();
}

fn emit_said(m: &Machine, w: &Path) -> String {
    let out = m
        .command(w)
        .args(["ws", "emit", "deneme.geldi"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn emit_says_why_the_machines_daemon_does_not_run_the_workspace() {
    let m = Machine::new();
    let w = m.workspace("eski");
    let why = "eski: journal.sqlite was made by a newer itero (journal schema 10; this itero knows up to 9): update itero";
    fake_machine(
        &m,
        serde_json::json!({ "ok": true, "daemon": { "pid": 7, "itero": "0.0.1" } }),
        serde_json::json!([{ "name": "eski", "folder": w, "state": "missing", "why": why }]),
    );
    let said = emit_said(&m, &w);
    assert!(
        said.contains(&format!("the machine's daemon does not run eski: {why}")),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "the machine's daemon runs itero 0.0.1, this is {}: itero service restart puts this one in place",
            env!("CARGO_PKG_VERSION")
        )),
        "{said}"
    );
    assert!(!said.contains("no daemon is running"), "{said}");
}

#[test]
fn emit_says_the_machines_daemon_does_not_have_a_workspace_off_its_list() {
    let m = Machine::new();
    let w = m.workspace("listede-yok");
    fake_machine(
        &m,
        serde_json::json!({ "ok": true, "daemon": { "pid": 7, "itero": env!("CARGO_PKG_VERSION") } }),
        serde_json::json!([]),
    );
    let said = emit_said(&m, &w);
    assert!(
        said.contains(
            "the machine's daemon does not run this workspace: itero ws add puts it on its list"
        ),
        "{said}"
    );
    assert!(!said.contains("runs itero"), "the same itero: {said}");
    // no machine's daemon at all: as before
    std::fs::remove_file(m.d.path().join("durum/itero/port")).unwrap();
    assert!(emit_said(&m, &w).contains("no daemon is running"));
}

#[test]
fn add_says_whether_the_machines_daemon_runs_the_workspace() {
    let m = Machine::new();
    let refused = m.workspace("reddedilen");
    fake_machine(
        &m,
        serde_json::json!({ "ok": true, "daemon": { "pid": 7, "itero": env!("CARGO_PKG_VERSION") } }),
        serde_json::json!([{ "name": "reddedilen", "folder": refused, "state": "missing", "why": "kötü bir günlük" }]),
    );
    let said = m.ok(&refused, &["ws", "add"]);
    assert!(
        said.contains("the machine's daemon does not run it: kötü bir günlük"),
        "{said}"
    );
    assert!(!said.contains("starts it now"), "{said}");
    // a real one, which takes it up
    let m = Machine::new();
    let _d = m.daemon_all();
    m.port();
    let good = m.workspace("iyi");
    let said = m.ok(&good, &["ws", "add"]);
    assert!(said.contains("the machine's daemon runs it"), "{said}");
}

#[test]
fn a_workspace_the_running_daemon_has_not_read_yet_is_pending() {
    let m = Machine::new();
    let w = m.workspace("yeni");
    // a daemon answers, and has none of the list yet
    fake_machine(
        &m,
        serde_json::json!({ "ok": true, "daemon": { "pid": 7, "itero": env!("CARGO_PKG_VERSION") } }),
        serde_json::json!([]),
    );
    m.ok(&w, &["ws", "add"]);
    let v = m.json(&w, &["ws", "list", "--json"]);
    assert_eq!(v[0]["state"], "pending", "{v}");
    assert!(v[0]["why"].as_str().unwrap().contains("takes it up"), "{v}");
    // with no daemon answering, it is stopped
    let m = Machine::new();
    let w = m.workspace("yeni");
    m.ok(&w, &["ws", "add"]);
    let v = m.json(&w, &["ws", "list", "--json"]);
    assert_eq!(v[0]["state"], "stopped", "{v}");
}
