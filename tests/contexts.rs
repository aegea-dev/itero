//! Contexts: `itero context add`, `use`, `list` and `remove`, kept in the
//! config folder's `contexts.json`. Every command runs with HOME,
//! XDG_CONFIG_HOME and XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

struct Machine {
    d: tempfile::TempDir,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            d: tempfile::tempdir().unwrap(),
        }
    }

    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(self.d.path())
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_CONFIG_HOME", self.d.path().join("ayar"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .env_remove("ITERO_TOKEN")
            .env_remove("ITERO_CONTEXT")
            .stdin(Stdio::null());
        c
    }

    /// `itero <args>`, with `token` as ITERO_TOKEN when given.
    fn run(&self, args: &[&str], token: Option<&str>) -> Output {
        let mut c = self.command();
        c.args(args);
        if let Some(t) = token {
            c.env("ITERO_TOKEN", t);
        }
        c.output().unwrap()
    }

    /// `itero <args>`, which must succeed: its stdout and stderr.
    fn ok(&self, args: &[&str], token: Option<&str>) -> String {
        let o = self.run(args, token);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(o.status.success(), "{args:?}: {said}");
        said
    }

    /// `itero <args>`, which must fail: what it says.
    fn fails(&self, args: &[&str], token: Option<&str>) -> String {
        let o = self.run(args, token);
        let said = String::from_utf8_lossy(&o.stderr).into_owned();
        assert!(!o.status.success(), "{args:?} succeeded: {said}");
        said
    }

    fn file(&self) -> PathBuf {
        self.d.path().join("ayar/itero/contexts.json")
    }

    fn contexts(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.file()).unwrap()).unwrap()
    }

    /// `itero daemon --all`, and its API's address once it answers: its
    /// port is the one it keeps in the state folder (no port picked and
    /// let go).
    fn server(&self) -> (Daemon, String) {
        let child = self
            .command()
            .args(["daemon", "--all"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let d = Daemon(child);
        let kept = self.d.path().join("durum/itero/port");
        let end = Instant::now() + Duration::from_secs(20);
        let http = itero::net::client(Duration::from_secs(2)).unwrap();
        loop {
            assert!(Instant::now() < end, "waited 20s for the daemon's API");
            if let Some(port) = std::fs::read_to_string(&kept)
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok())
            {
                let url = format!("http://127.0.0.1:{port}/");
                if http
                    .get(format!("{url}health"))
                    .send()
                    .is_ok_and(|r| r.status() == 200)
                {
                    return (d, url);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A server answering every request with `status` and `body`, and the
/// URL it is at: its listener stays bound for the test's life.
fn answering(status: u16, body: &'static str) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/", l.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// A server that closes each connection as it takes it.
fn closing() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/", l.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            drop(s);
        }
    });
    url
}

#[test]
fn add_checks_the_server_and_keeps_the_context() {
    let m = Machine::new();
    let (_d, url) = m.server();
    // an empty token is none: a server on 127.0.0.1 needs none
    let said = m.ok(
        &["context", "add", "yerel", &url, "--ws", "çalışma"],
        Some(""),
    );
    assert!(
        said.contains(&format!("itero {}", env!("CARGO_PKG_VERSION"))),
        "{said}"
    );
    assert_eq!(
        m.contexts(),
        json!({ "current": null,
                "contexts": [{ "name": "yerel", "url": url, "ws": "çalışma" }] })
    );
    let mode = std::fs::metadata(m.file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    // the same name again replaces it, and says so
    let said = m.ok(&["context", "add", "yerel", &url], Some("itero_çay"));
    assert!(said.contains("replaced yerel"), "{said}");
    assert_eq!(
        m.contexts()["contexts"],
        json!([{ "name": "yerel", "url": url, "token": "itero_çay" }])
    );
}

#[test]
fn add_refuses_a_server_that_does_not_answer() {
    let m = Machine::new();
    let url = closing();
    let said = m.fails(&["context", "add", "uzak", &url], Some(""));
    assert!(said.contains("cannot reach uzak"), "{said}");
    assert!(!m.file().exists());
}

#[test]
fn add_refuses_a_token_the_server_refuses() {
    let m = Machine::new();
    let url = answering(401, r#"{"error": "no token"}"#);
    let said = m.fails(&["context", "add", "uzak", &url], Some("itero_yanlış"));
    assert!(said.contains("uzak refused the token"), "{said}");
    assert!(!m.file().exists());
}

#[test]
fn add_refuses_a_url_that_is_not_http_and_tells_every_problem() {
    let m = Machine::new();
    let said = m.fails(&["context", "add", "a b", "ftp://örnek"], Some(""));
    assert!(said.contains("a b: a name is letters"), "{said}");
    assert!(said.contains("ftp://örnek: a URL starts"), "{said}");
}

#[test]
fn use_list_and_remove() {
    let m = Machine::new();
    let (_d, url) = m.server();
    m.ok(
        &["context", "add", "bir", &url, "--ws", "çay"],
        Some("itero_gizli"),
    );
    m.ok(&["context", "add", "iki", &url], Some(""));
    m.ok(&["context", "use", "iki"], None);
    assert_eq!(m.contexts()["current"], "iki");
    let o = m.run(&["context", "list", "--json"], None);
    assert!(o.status.success());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v,
        json!([
            { "name": "local", "url": null, "ws": null, "current": false },
            { "name": "bir", "url": url, "ws": "çay", "current": false },
            { "name": "iki", "url": url, "ws": null, "current": true },
        ])
    );
    // the token is never shown
    let text = m.ok(&["context", "list"], None);
    assert!(!text.contains("itero_gizli"), "{text}");
    assert!(text.contains("* iki"), "{text}");
    m.ok(&["context", "use", "local"], None);
    assert_eq!(m.contexts()["current"], Value::Null);
    m.ok(&["context", "use", "iki"], None);
    m.ok(&["context", "remove", "iki"], None);
    assert_eq!(m.contexts()["current"], Value::Null);
    assert_eq!(m.contexts()["contexts"].as_array().unwrap().len(), 1);
    let said = m.fails(&["context", "use", "iki"], None);
    assert!(said.contains("no context iki"), "{said}");
    let said = m.fails(&["context", "remove", "yok"], None);
    assert!(said.contains("no context yok"), "{said}");
}

#[test]
fn local_is_built_in() {
    let m = Machine::new();
    let url = closing();
    let said = m.fails(&["context", "add", "local", &url], Some(""));
    assert!(said.contains("local is this machine"), "{said}");
    let said = m.fails(&["context", "remove", "local"], None);
    assert!(said.contains("local is this machine"), "{said}");
}

#[test]
fn a_token_is_never_an_argument() {
    let m = Machine::new();
    let url = closing();
    let o = m.run(&["context", "add", "uzak", &url, "itero_gizli"], None);
    assert_eq!(o.status.code(), Some(2), "a usage error");
}

/// A machine with two workspaces on its list, `bir` and `iki` folders,
/// run by its daemon: their names on the list, and the server's URL.
fn two_workspaces(m: &Machine) -> (Daemon, String, String, String) {
    let mut names = vec![];
    for folder in ["bir", "iki"] {
        let w = m.d.path().join("projeler").join(folder);
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("itero.config.json"), "{}").unwrap();
        let o = m.command().current_dir(&w).args(["ws", "add"]).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let ws = itero::workspace::find(&w).unwrap();
        names.push(
            itero::engine::journal::Journal::open(&ws)
                .unwrap()
                .workspace()
                .unwrap()
                .name,
        );
    }
    let (d, url) = m.server();
    (d, url, names[0].clone(), names[1].clone())
}

/// The name of the workspace `ws info` answered for, with `env` set.
fn answered_by(m: &Machine, args: &[&str], env: Option<&str>) -> String {
    let mut c = m.command();
    c.args(["ws", "info", "--json"]).args(args);
    if let Some(e) = env {
        c.env("ITERO_CONTEXT", e);
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    v["workspace"]["name"].as_str().unwrap().to_string()
}

#[test]
fn the_context_is_the_flag_else_itero_context_else_the_current_one() {
    let m = Machine::new();
    let (_d, url, a, b) = two_workspaces(&m);
    m.ok(&["context", "add", "bir", &url, "--ws", &a], Some(""));
    m.ok(&["context", "add", "iki", &url, "--ws", &b], Some(""));
    m.ok(&["context", "use", "bir"], None);
    assert_eq!(answered_by(&m, &[], None), a);
    assert_eq!(answered_by(&m, &[], Some("iki")), b);
    assert_eq!(answered_by(&m, &["--context", "bir"], Some("iki")), a);
    // --ws picks another workspace on the same server
    assert_eq!(answered_by(&m, &["--context", "bir", "--ws", &b], None), b);
    // a context that is not there
    let said = m.fails(&["--context", "yok", "ws", "info"], None);
    assert!(said.contains("no context yok"), "{said}");
}

#[test]
fn with_no_workspace_named_it_names_the_servers() {
    let m = Machine::new();
    let (_d, url, a, b) = two_workspaces(&m);
    m.ok(&["context", "add", "sunucu", &url], Some(""));
    let said = m.fails(&["--context", "sunucu", "ws", "info"], None);
    assert!(said.contains("which workspace on sunucu"), "{said}");
    assert!(said.contains(&a) && said.contains(&b), "{said}");
}

#[test]
fn local_answers_in_process_with_no_daemon() {
    let m = Machine::new();
    let w = m.d.path().join("projeler/yerel");
    std::fs::create_dir_all(&w).unwrap();
    std::fs::write(w.join("itero.config.json"), "{}").unwrap();
    let o = m
        .command()
        .current_dir(&w)
        .env("ITERO_CONTEXT", "local")
        .args(["ws", "info", "--json"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["daemon"]["running"], false, "{v}");
}

#[test]
fn a_command_about_this_machine_refuses_a_context_given_to_it() {
    let m = Machine::new();
    let url = closing();
    // added by hand: the server is not needed to refuse
    std::fs::create_dir_all(m.d.path().join("ayar/itero")).unwrap();
    std::fs::write(
        m.file(),
        json!({ "contexts": [{ "name": "uzak", "url": url }] }).to_string(),
    )
    .unwrap();
    for args in [
        &["daemon"][..],
        &["service", "status"],
        &["ws", "add"],
        &["ws", "list"],
        &["secret", "ANAHTAR"],
        &["emit", "kapı.açıldı"],
        &["info"],
    ] {
        let mut a = args.to_vec();
        a.extend(["--context", "uzak"]);
        let said = m.fails(&a, None);
        assert!(said.contains("works on this machine"), "{args:?}: {said}");
    }
}
