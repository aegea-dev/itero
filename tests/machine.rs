//! One itero per machine: the machine's list of workspaces, and the one
//! daemon that runs them. Every command runs with HOME and
//! XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
use itero::engine::journal::Journal;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A daemon this test started, killed when the test ends.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

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
            .env("ITERO_NOTIFIER", "/usr/bin/true");
        c
    }

    /// `itero daemon --all`, from this machine's own folder.
    fn daemon_all(&self) -> Daemon {
        let child = self
            .command(self.d.path())
            .args(["daemon", "--all"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Daemon(child)
    }

    /// `itero daemon --all --port <port>`.
    fn daemon_all_on(&self, port: u16) -> Daemon {
        let child = self
            .command(self.d.path())
            .args(["daemon", "--all", "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
    m.ok(&a, &["emit", "deneme.geldi"]);
    until("a's workflow", || a.join("çıktı").is_file());
    // added while it runs: started
    m.ok(&b, &["workspace", "add"]);
    until("b to be run", || {
        itero::engine::lock::take(&m.ws(&b), "deneme").is_err()
    });
    m.ok(&b, &["emit", "deneme.geldi"]);
    until("b's workflow", || b.join("çıktı").is_file());
    // removed while it runs: stopped, its lock free
    m.ok(&a, &["workspace", "remove"]);
    until("a to be stopped", || {
        itero::engine::lock::take(&m.ws(&a), "deneme").is_ok()
    });
    // and b runs on
    std::fs::remove_file(b.join("çıktı")).unwrap();
    m.ok(&b, &["emit", "deneme.geldi"]);
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
        .post(format!("http://127.0.0.1:{port}/w/{na}/events"))
        .json(&serde_json::json!({ "type": "deneme.geldi" }))
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 202);
    until("a's workflow", || a.join("çıktı").is_file());
    assert!(!b.join("çıktı").exists());
    let (code, runs) = get(port, &format!("/w/{na}/runs"));
    assert_eq!(code, 200, "{runs}");
    assert_eq!(runs.as_array().map(Vec::len), Some(1), "{runs}");
    let (code, h) = get(port, &format!("/w/{nb}/health"));
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
    let (code, e) = get(port, "/w/yok/runs");
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
    own.arg("daemon")
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
        r##"{"on": [{"event": "itero.workspace.stuck"}], "steps": ["#not"]}"##,
    )
    .unwrap();
    let not = b.join("actions/not");
    std::fs::write(
        &not,
        format!(
            "#!/bin/sh\necho \"$workspace\" > '{}'\n",
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
        .args(["daemon", "--all", "--port", &port.to_string()])
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
    m.ok(&a, &["emit", "deneme.geldi"]);
    until("a stuck", || state(&na) == Some("stuck".into()));
    until("b to hear", || b.join("takıldı").is_file());
    assert_eq!(
        std::fs::read_to_string(b.join("takıldı")).unwrap().trim(),
        na
    );
    // b runs on meanwhile
    m.ok(&b, &["emit", "deneme.geldi"]);
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
        get(port, &format!("/w/{na}/health")).0 == 200
    });
    let v = m.json(&a, &["info", "--json"]);
    let d = &v["daemon"];
    assert_eq!(d["running"], true, "{v}");
    assert_eq!(d["run_by"], "machine", "{v}");
    let api = format!("http://127.0.0.1:{port}/w/{na}/");
    assert_eq!(d["api"], api.as_str(), "{v}");
    let text = m.ok(&a, &["info"]);
    assert!(
        text.contains(&api) && text.contains("the machine's daemon"),
        "{text}"
    );
    // `itero emit --wait` waits on the same daemon
    let out = m.ok(&a, &["emit", "deneme.geldi", "--wait"]);
    assert!(a.join("çıktı").is_file(), "{out}");
}
