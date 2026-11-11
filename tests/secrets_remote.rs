//! Secrets on a server: from a folder the platform mounts (daemon.json's
//! `secrets`), and `itero secret --context` to the server's OS store,
//! never over plain HTTP to another host. Every command runs with HOME,
//! XDG_CONFIG_HOME and XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

struct Machine {
    d: tempfile::TempDir,
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "waited 20s for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Machine {
    fn new() -> Machine {
        Machine {
            d: tempfile::tempdir().unwrap(),
        }
    }

    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(dir)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_CONFIG_HOME", self.d.path().join("ayar"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .env_remove("ANAHTAR")
            .stdin(Stdio::null());
        c
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(dir).args(args).output().unwrap()
    }

    /// A workspace whose `yaz` workflow, on `yaz.geldi`, writes its arg
    /// `anahtar` (`$$ANAHTAR`) to `out`; on the list: its name there.
    fn workspace(&self, out: &Path) -> (PathBuf, String) {
        use std::os::unix::fs::PermissionsExt;
        let w = self.d.path().join("projeler/bir");
        std::fs::create_dir_all(w.join("workflows")).unwrap();
        std::fs::create_dir_all(w.join("actions")).unwrap();
        std::fs::write(w.join("itero.config.json"), "{}").unwrap();
        std::fs::write(
            w.join("workflows/yaz.json"),
            json!({ "on": [{ "event": "yaz.geldi" }],
                    "steps": [{ "#yaz": { "args": { "anahtar": "$$ANAHTAR" } } }] })
            .to_string(),
        )
        .unwrap();
        let a = w.join("actions/yaz");
        std::fs::write(
            &a,
            format!(
                "#!/bin/sh\nprintf '%s' \"$anahtar\" > '{}'\n",
                out.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o755)).unwrap();
        let o = self.run(&w, &["ws", "add"]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let ws = itero::workspace::find(&w).unwrap();
        let name = itero::engine::journal::Journal::open(&ws)
            .unwrap()
            .workspace()
            .unwrap()
            .name;
        (w, name)
    }

    /// `itero daemon`, once the workspace `name` runs: the API's address.
    fn daemon(&self, name: &str) -> (Daemon, String) {
        let d = Daemon(
            self.command(self.d.path())
                .arg("daemon")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let kept = self.d.path().join("durum/itero/port");
        let http = itero::net::client(Duration::from_secs(2)).unwrap();
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < end, "waited 20s for {name} to run");
            if let Some(port) = std::fs::read_to_string(&kept)
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok())
            {
                let url = format!("http://127.0.0.1:{port}/");
                let running = http
                    .get(format!("{url}workspaces"))
                    .send()
                    .ok()
                    .and_then(|r| r.json::<serde_json::Value>().ok())
                    .is_some_and(|v| {
                        v.as_array().is_some_and(|l| {
                            l.iter()
                                .any(|w| w["name"] == name && w["state"] == "running")
                        })
                    });
                if running {
                    return (d, url);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn daemon_json(&self, v: serde_json::Value) {
        let p = self.d.path().join("ayar/itero/daemon.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    }
}

#[test]
fn a_secret_in_the_servers_folder_reaches_a_step_its_workspaces_first() {
    let m = Machine::new();
    let out = m.d.path().join("çıktı");
    let (w, name) = m.workspace(&out);
    let folder = m.d.path().join("sırlar");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("ANAHTAR"), "genel\n").unwrap();
    m.daemon_json(json!({ "secrets": folder.display().to_string() }));
    let (_d, _) = m.daemon(&name);
    let o = m.run(&w, &["ws", "emit", "yaz.geldi"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    until("the step to write", || {
        std::fs::read_to_string(&out).is_ok_and(|t| !t.is_empty())
    });
    // the file's last newline is not the secret's
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "genel");
    // the workspace's own, <name>.ANAHTAR, comes first
    std::fs::write(folder.join(format!("{name}.ANAHTAR")), "özel").unwrap();
    std::fs::remove_file(&out).unwrap();
    m.run(&w, &["ws", "emit", "yaz.geldi"]);
    until("the step to write again", || out.exists());
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "özel");
}

#[test]
fn the_secrets_setting_must_be_a_folder() {
    let m = Machine::new();
    m.daemon_json(json!({ "secrets": "/yok/sırlar" }));
    let o = m.run(m.d.path(), &["daemon", "--no-serve"]);
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("secrets: /yok/sırlar"), "{said}");
}

#[test]
fn a_secret_sent_to_a_server_with_no_store_says_what_to_do() {
    let m = Machine::new();
    let out = m.d.path().join("çıktı");
    let (w, name) = m.workspace(&out);
    let (_d, url) = m.daemon(&name);
    let o = m
        .command(&w)
        .args(["context", "add", "yerel", &url, "--ws", &name])
        .env("ITERO_TOKEN", "")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let value = m.d.path().join("değer");
    std::fs::write(&value, "gizli\n").unwrap();
    let v = value.display().to_string();
    for args in [
        &["secret", "ANAHTAR", "--file", &v, "--context", "yerel"][..],
        &[
            "ws",
            "secret",
            "ANAHTAR",
            "--file",
            &v,
            "--context",
            "yerel",
        ],
    ] {
        let o = m.run(&w, args);
        let said = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{args:?}");
        assert!(said.contains("keeps no secrets"), "{args:?}: {said}");
        assert!(said.contains("its secrets folder"), "{args:?}: {said}");
    }
}

#[test]
fn a_secret_is_never_sent_over_plain_http_to_another_host() {
    let m = Machine::new();
    std::fs::create_dir_all(m.d.path().join("ayar/itero")).unwrap();
    std::fs::write(
        m.d.path().join("ayar/itero/contexts.json"),
        json!({ "contexts": [{ "name": "uzak", "url": "http://sunucu.örnek.test:8443/", "ws": "bir" }] })
            .to_string(),
    )
    .unwrap();
    let value = m.d.path().join("değer");
    std::fs::write(&value, "gizli").unwrap();
    let o = m.run(
        m.d.path(),
        &[
            "secret",
            "ANAHTAR",
            "--file",
            value.to_str().unwrap(),
            "--context",
            "uzak",
        ],
    );
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(said.contains("not sent over plain HTTP"), "{said}");
}
