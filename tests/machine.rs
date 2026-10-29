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
