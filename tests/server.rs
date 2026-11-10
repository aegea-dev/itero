//! `server.json`: how the machine's daemon serves, read at its start from
//! itero's config folder, or the file `--config` names. Every command runs
//! with HOME, XDG_CONFIG_HOME and XDG_STATE_HOME in a temporary folder.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .stdin(Stdio::null());
        c
    }

    /// The default `server.json`, written with `text`.
    fn server_json(&self, text: &str) -> PathBuf {
        let p = self.d.path().join("ayar/itero/server.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    fn kept_port(&self) -> PathBuf {
        self.d.path().join("durum/itero/port")
    }

    /// `itero daemon --all <args>`, running, and the port it answers on.
    fn daemon(&self, args: &[&str]) -> (Daemon, u16) {
        let child = self
            .command()
            .args(["daemon", "--all"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let d = Daemon(child);
        let http = itero::net::client(Duration::from_secs(2)).unwrap();
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < end, "waited 20s for the daemon's API");
            if let Some(port) = std::fs::read_to_string(self.kept_port())
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok())
                && http
                    .get(format!("http://127.0.0.1:{port}/health"))
                    .send()
                    .is_ok_and(|r| r.status() == 200)
            {
                return (d, port);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// `itero daemon --all <args>`, which must fail at once: what it says.
    /// One that starts instead is stopped after 20 seconds, failing the
    /// test rather than hanging it.
    fn refused(&self, args: &[&str]) -> String {
        let said = self.d.path().join("said.txt");
        let mut child = self
            .command()
            .args(["daemon", "--all"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&said).unwrap())
            .spawn()
            .unwrap();
        let ended =
            wait_timeout::ChildExt::wait_timeout(&mut child, Duration::from_secs(20)).unwrap();
        let text = || std::fs::read_to_string(&said).unwrap_or_default();
        let Some(status) = ended else {
            stop::stop(&mut child);
            panic!("it started: {}", text());
        };
        assert!(!status.success(), "it ended well: {}", text());
        text()
    }
}

#[test]
fn no_file_is_todays_daemon() {
    let m = Machine::new();
    let (_d, port) = m.daemon(&[]);
    assert!(port > 0);
}

#[test]
fn port_in_the_file_is_the_local_port() {
    let m = Machine::new();
    // a port the OS gave a first daemon: no port picked and let go
    let (d, port) = m.daemon(&[]);
    drop(d);
    std::fs::remove_file(m.kept_port()).unwrap();
    m.server_json(&format!(r#"{{ "port": {port} }}"#));
    let (_d, again) = m.daemon(&[]);
    assert_eq!(again, port);
}

#[test]
fn every_problem_is_told_at_once_and_nothing_is_served() {
    let m = Machine::new();
    let file = m.server_json(r#"{ "port": "sekiz", "dinle": 1, "fazla": true }"#);
    let said = m.refused(&[]);
    let at = file.display().to_string();
    for problem in [
        "port: a number from 1 to 65535, not \"sekiz\"",
        "dinle: not a setting of server.json",
        "fazla: not a setting of server.json",
    ] {
        assert!(said.contains(&format!("{at}: {problem}")), "{said}");
    }
    assert!(!m.kept_port().exists(), "nothing listened");
}

#[test]
fn a_file_that_is_not_json_or_not_an_object_is_said() {
    let m = Machine::new();
    m.server_json("{ port: 1 ");
    assert!(m.refused(&[]).contains("not JSON"));
    m.server_json("[1]");
    assert!(m.refused(&[]).contains("a JSON object"));
    m.server_json(r#"{ "port": 70000 }"#);
    assert!(
        m.refused(&[])
            .contains("port: a number from 1 to 65535, not 70000")
    );
}

#[test]
fn config_reads_that_file_and_a_missing_one_is_an_error() {
    let m = Machine::new();
    let other = m.d.path().join("sunucu.json");
    std::fs::write(&other, r#"{ "port": 0 }"#).unwrap();
    let said = m.refused(&["--config", other.to_str().unwrap()]);
    assert!(
        said.contains("sunucu.json: port: a number from 1 to 65535, not 0"),
        "{said}"
    );
    let gone = m.d.path().join("yok.json");
    let said = m.refused(&["--config", gone.to_str().unwrap()]);
    assert!(said.contains("yok.json"), "{said}");
    // the default file missing is no error: today's daemon
    let (_d, _) = m.daemon(&[]);
}

#[test]
fn the_daemon_takes_no_port_flag_now() {
    let m = Machine::new();
    let said = m.refused(&["--port", "8787"]);
    assert!(said.contains("unexpected argument '--port'"), "{said}");
}
