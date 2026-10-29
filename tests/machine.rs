//! One itero per machine: the machine's list of workspaces, and the one
//! daemon that runs them. Every command runs with HOME and
//! XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
use itero::engine::journal::Journal;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    fn run(&self, dir: &Path, args: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_itero"))
            .args(args)
            .current_dir(dir)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .output()
            .unwrap();
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
