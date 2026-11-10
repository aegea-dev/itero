//! The machine's daemon opened by `server.json`'s `listen`: a second
//! listener with the same routes, every request there with a token
//! `itero token create` made, any Host; the local API beside it as it
//! was. Every command runs with HOME, XDG_CONFIG_HOME and XDG_STATE_HOME
//! in a temporary folder. The opened listener binds 127.0.0.1 here: opened
//! is the listener's, not the address's, and binding every interface
//! could ask a Mac's firewall.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use stop::Daemon;

struct Machine {
    d: tempfile::TempDir,
}

/// A running opened daemon: its local port, its opened port, and where
/// its stderr goes.
struct Opened {
    _d: Daemon,
    local: u16,
    opened: u16,
    said: PathBuf,
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

    fn ok(&self, args: &[&str]) -> String {
        let o = self.command().args(args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn server_json(&self, v: Value) {
        let p = self.d.path().join("ayar/itero/server.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    }

    /// A workspace on the list, `{}` its manifest: its name there.
    fn listed(&self) -> String {
        let w = self.d.path().join("projeler/bir");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("itero.config.json"), "{}").unwrap();
        let o = self
            .command()
            .current_dir(&w)
            .args(["ws", "add"])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let ws = itero::workspace::find(&w).unwrap();
        itero::engine::journal::Journal::open(&ws)
            .unwrap()
            .workspace()
            .unwrap()
            .name
    }

    /// `itero daemon --all`, opened, once both its listeners answer: the
    /// opened port as it says it on stderr (no port picked and let go).
    fn opened(&self) -> Opened {
        let said = self.d.path().join("said.txt");
        let child = self
            .command()
            .args(["daemon", "--all"])
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&said).unwrap())
            .spawn()
            .unwrap();
        let d = Daemon(child);
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let text = std::fs::read_to_string(&said).unwrap_or_default();
            assert!(
                Instant::now() < end,
                "waited 20s for both listeners: {text}"
            );
            let opened = text
                .lines()
                .find_map(|l| l.strip_prefix("itero: opened on 127.0.0.1:"))
                .and_then(|p| p.trim().parse::<u16>().ok());
            let local = std::fs::read_to_string(self.d.path().join("durum/itero/port"))
                .ok()
                .and_then(|p| p.trim().parse::<u16>().ok());
            if let (Some(opened), Some(local)) = (opened, local)
                && status(local, "/health", None, None) == 200
            {
                return Opened {
                    _d: d,
                    local,
                    opened,
                    said,
                };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn http() -> reqwest::blocking::Client {
    itero::net::client(Duration::from_secs(10)).unwrap()
}

/// A GET's status, with `token` and a `host` of its own when given.
fn status(port: u16, path: &str, token: Option<&str>, host: Option<&str>) -> u16 {
    let mut r = http().get(format!("http://127.0.0.1:{port}{path}"));
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    if let Some(h) = host {
        r = r.header(reqwest::header::HOST, h);
    }
    r.send().map(|r| r.status().as_u16()).unwrap_or(0)
}

#[test]
fn the_opened_api_lets_in_a_token_and_nobody_else() {
    let m = Machine::new();
    let token = m.ok(&["token", "create", "kişi"]).trim().to_string();
    let name = m.listed();
    m.server_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.opened();
    // no token, a wrong one: 401, saying how to sign in
    let r = http()
        .get(format!("http://127.0.0.1:{}/health", o.opened))
        .send()
        .unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.headers()["www-authenticate"], "Bearer");
    // a stranger learns nothing, not even which itero answers
    assert!(r.headers().get("itero-version").is_none());
    let v: Value = r.json().unwrap();
    assert!(v["error"].as_str().unwrap().contains("token"), "{v}");
    assert_eq!(status(o.opened, "/health", Some("itero_yanlış"), None), 401);
    // the token, whatever Host: the machine's routes and a workspace's
    let host = Some("itero.örnek.test");
    assert_eq!(status(o.opened, "/health", Some(&token), host), 200);
    assert_eq!(status(o.opened, "/workspaces", Some(&token), host), 200);
    let path = format!("/ws/{name}/runs");
    assert_eq!(status(o.opened, &path, Some(&token), host), 200);
    // what changes something is logged with the token's name
    let r = http()
        .post(format!("http://127.0.0.1:{}/events", o.opened))
        .bearer_auth(&token)
        .json(&json!({ "type": "kapı.açıldı", "data": {} }))
        .send()
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    let said = std::fs::read_to_string(&o.said).unwrap();
    assert!(said.contains("kişi: POST /events"), "{said}");
    // no TLS: said at the start
    assert!(said.contains("serving without TLS on 127.0.0.1:"), "{said}");
    // revoked: out at the next request
    m.ok(&["token", "revoke", "kişi"]);
    assert_eq!(status(o.opened, "/health", Some(&token), None), 401);
    // the local API beside it, as it was: no token, its Host only
    assert_eq!(status(o.local, "/health", None, None), 200);
    assert_eq!(status(o.local, "/health", None, host), 403);
}

#[test]
fn opened_with_no_token_it_does_not_start() {
    let m = Machine::new();
    m.server_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.command().args(["daemon", "--all"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(
        said.contains("no token can reach it: itero token create NAME first"),
        "{said}"
    );
}

#[test]
fn listen_with_no_serve_is_a_mistake() {
    let m = Machine::new();
    m.ok(&["token", "create", "kişi"]);
    m.server_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m
        .command()
        .args(["daemon", "--all", "--no-serve"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("--no-serve"), "{said}");
}

#[test]
fn listen_must_be_an_address_and_a_port() {
    let m = Machine::new();
    m.server_json(json!({ "listen": "her yerde" }));
    let o = m.command().args(["daemon", "--all"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(
        said.contains("listen: an address and a port, like 0.0.0.0:8443, not \"her yerde\""),
        "{said}"
    );
}

/// A certificate authority, and a certificate it signed for 127.0.0.1
/// with its key: (authority, certificate, key), PEM.
fn certificates() -> (String, String, String) {
    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
    let mut authority = CertificateParams::new(Vec::<String>::new()).unwrap();
    authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let authority = CertifiedIssuer::self_signed(authority, KeyPair::generate().unwrap()).unwrap();
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["127.0.0.1".to_string()])
        .unwrap()
        .signed_by(&key, &authority)
        .unwrap();
    (authority.pem(), cert.pem(), key.serialize_pem())
}

impl Machine {
    /// `tls` files written in the machine's folder: their paths.
    fn tls_files(&self, cert: &str, key: &str) -> (String, String) {
        let d = self.d.path().join("tls");
        std::fs::create_dir_all(&d).unwrap();
        let (c, k) = (d.join("sertifika.pem"), d.join("anahtar.pem"));
        std::fs::write(&c, cert).unwrap();
        std::fs::write(&k, key).unwrap();
        (c.display().to_string(), k.display().to_string())
    }
}

#[test]
fn tls_serves_the_opened_api_over_https() {
    let m = Machine::new();
    let token = m.ok(&["token", "create", "kişi"]).trim().to_string();
    let (authority, cert, key) = certificates();
    let (cert, key) = m.tls_files(&cert, &key);
    m.server_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
    let o = m.opened();
    // reqwest's rustls needs ring as the process's provider, as itero's does
    itero::net::client(Duration::from_secs(1)).unwrap();
    let https = reqwest::blocking::Client::builder()
        .tls_certs_only([reqwest::Certificate::from_pem(authority.as_bytes()).unwrap()])
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let url = format!("https://127.0.0.1:{}/health", o.opened);
    let r = https.get(&url).bearer_auth(&token).send().unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        https.get(&url).send().unwrap().status(),
        401,
        "a token still"
    );
    // plain HTTP to it gets no answer
    assert_eq!(status(o.opened, "/health", Some(&token), None), 0);
    let said = std::fs::read_to_string(&o.said).unwrap();
    assert!(!said.contains("without TLS"), "{said}");
    // the local API beside it stays plain HTTP
    assert_eq!(status(o.local, "/health", None, None), 200);
}

#[test]
fn a_key_that_is_not_the_certificates_stops_the_daemon() {
    let m = Machine::new();
    m.ok(&["token", "create", "kişi"]);
    let (_, cert, _) = certificates();
    let (_, _, other) = certificates();
    let (cert, key) = m.tls_files(&cert, &other);
    m.server_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
    let o = m.command().args(["daemon", "--all"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("anahtar.pem"), "{said}");
}

#[test]
fn tls_needs_listen_and_both_files_every_problem_told() {
    let m = Machine::new();
    m.server_json(json!({ "tls": { "cert": "/yok/sertifika.pem", "fazla": 1 } }));
    let o = m.command().args(["daemon", "--all"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    for problem in [
        "tls: for listen, which is not given",
        "tls.key: the private key's PEM file, not given",
        "tls.fazla: not a setting of tls (cert, key)",
        "tls.cert: /yok/sertifika.pem:",
    ] {
        assert!(said.contains(problem), "{problem}: {said}");
    }
}

impl Machine {
    /// An opened daemon with TLS, a token for it, and a client that
    /// trusts its certificate authority.
    fn opened_tls(&self, key_mode: u32) -> (Opened, String, reqwest::blocking::Client) {
        use std::os::unix::fs::PermissionsExt;
        let token = self.ok(&["token", "create", "kişi"]).trim().to_string();
        let (authority, cert, key) = certificates();
        let (cert, key) = self.tls_files(&cert, &key);
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(key_mode)).unwrap();
        self.server_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
        let o = self.opened();
        itero::net::client(Duration::from_secs(1)).unwrap();
        let https = reqwest::blocking::Client::builder()
            .tls_certs_only([reqwest::Certificate::from_pem(authority.as_bytes()).unwrap()])
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        (o, token, https)
    }
}

#[test]
fn idle_connections_do_not_hold_up_a_client_and_are_closed() {
    use std::io::Read;
    let m = Machine::new();
    let (o, token, https) = m.opened_tls(0o600);
    // connections that never start a handshake
    let idle: Vec<std::net::TcpStream> = (0..8)
        .map(|_| std::net::TcpStream::connect(("127.0.0.1", o.opened)).unwrap())
        .collect();
    let began = Instant::now();
    let url = format!("https://127.0.0.1:{}/health", o.opened);
    let r = https.get(&url).bearer_auth(&token).send().unwrap();
    assert_eq!(r.status(), 200);
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    // one that stays silent is closed once the handshake's time is up
    let mut one = idle.into_iter().next().unwrap();
    one.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let began = Instant::now();
    let read = one.read(&mut [0u8; 16]);
    assert!(
        matches!(read, Ok(0)),
        "{read:?} after {:?}",
        began.elapsed()
    );
}

#[test]
fn a_key_others_can_read_is_said_at_the_start() {
    let m = Machine::new();
    let (o, _, _) = m.opened_tls(0o644);
    let said = std::fs::read_to_string(&o.said).unwrap();
    assert!(said.contains("anahtar.pem: readable by others"), "{said}");
}

#[test]
fn a_folder_for_the_certificate_is_told_with_the_other_problems() {
    let m = Machine::new();
    let d = m.d.path().join("tls");
    std::fs::create_dir_all(&d).unwrap();
    m.server_json(json!({ "listen": "127.0.0.1:0",
                          "tls": { "cert": d.display().to_string(), "key": "/yok/anahtar.pem" } }));
    let o = m.command().args(["daemon", "--all"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("is not a file"), "{said}");
    assert!(said.contains("tls.key: /yok/anahtar.pem:"), "{said}");
}
