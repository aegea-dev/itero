//! The machine's daemon opened by `daemon.json`'s `listen`: a second
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

    fn daemon_json(&self, v: Value) {
        let p = self.d.path().join("ayar/itero/daemon.json");
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

    /// `itero daemon --all`, opened, once both its listeners answer and it
    /// has started the listed workspaces (`/readyz`): it serves while it
    /// starts them, and a workspace's route answers 404 until then. The
    /// opened port as it says it on stderr (no port picked and let go).
    fn opened(&self) -> Opened {
        let said = self.d.path().join("said.txt");
        let child = self
            .command()
            .args(["daemon"])
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
                && status(local, "/readyz", None, None) == 200
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0" }));
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.command().args(["daemon"]).output().unwrap();
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.command().args(["daemon", "--no-serve"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("--no-serve"), "{said}");
}

#[test]
fn listen_must_be_an_address_and_a_port() {
    let m = Machine::new();
    m.daemon_json(json!({ "listen": "her yerde" }));
    let o = m.command().args(["daemon"]).output().unwrap();
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
    let o = m.command().args(["daemon"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("anahtar.pem"), "{said}");
}

#[test]
fn tls_needs_listen_and_both_files_every_problem_told() {
    let m = Machine::new();
    m.daemon_json(json!({ "tls": { "cert": "/yok/sertifika.pem", "fazla": 1 } }));
    let o = m.command().args(["daemon"]).output().unwrap();
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
        self.daemon_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
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
    m.daemon_json(json!({ "listen": "127.0.0.1:0",
                          "tls": { "cert": d.display().to_string(), "key": "/yok/anahtar.pem" } }));
    let o = m.command().args(["daemon"]).output().unwrap();
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains("is not a file"), "{said}");
    assert!(said.contains("tls.key: /yok/anahtar.pem:"), "{said}");
}

#[test]
fn probes_answer_anyone_and_say_nothing_else() {
    let m = Machine::new();
    m.ok(&["token", "create", "kişi"]);
    m.listed();
    m.daemon_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.opened();
    for port in [o.opened, o.local] {
        // ready once the list has been read once: "starting" until then
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let r = http()
                .get(format!("http://127.0.0.1:{port}/readyz"))
                .send()
                .unwrap();
            assert!(r.headers().get("itero-version").is_none());
            let code = r.status().as_u16();
            let text = r.text().unwrap();
            if code == 200 {
                assert_eq!(text, "ok\n");
                break;
            }
            assert_eq!((code, text.as_str()), (503, "starting\n"));
            assert!(Instant::now() < end, "not ready in 20s");
            std::thread::sleep(Duration::from_millis(50));
        }
        let r = http()
            .get(format!("http://127.0.0.1:{port}/livez"))
            .send()
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers().get("itero-version").is_none());
        assert_eq!(r.text().unwrap(), "ok\n");
    }
    // any Host, even on the local API: they say nothing worth guarding
    assert_eq!(
        status(o.local, "/livez", None, Some("itero.örnek.test")),
        200
    );
    // everything else on the opened listener still needs the token
    assert_eq!(status(o.opened, "/health", None, None), 401);
    assert_eq!(status(o.opened, "/workspaces", None, None), 401);
}

/// How long the daemon waits for a request's headers (`HEADERS` in
/// `src/net.rs`), and how long a test waits past it.
const HEADERS: Duration = Duration::from_secs(10);
const SLACK: Duration = Duration::from_secs(10);

/// Reads `c` until the daemon closes it: how long that took from `from`,
/// or why it was not closed within `HEADERS` and `SLACK`. Whatever the
/// daemon writes before closing is read and left.
fn closed(c: &mut impl std::io::Read, from: Instant) -> Result<Duration, String> {
    let mut buf = [0u8; 4096];
    loop {
        match c.read(&mut buf) {
            Ok(0) => return Ok(from.elapsed()),
            Ok(_) => {}
            // a TLS connection closed with no close_notify reads as an
            // unexpected end: closed all the same
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                return Ok(from.elapsed());
            }
            Err(e) => return Err(format!("still open after {:?}: {e}", from.elapsed())),
        }
    }
}

/// A plain connection to `port`, reads waiting up to `HEADERS` and `SLACK`.
fn connect(port: u16) -> std::net::TcpStream {
    let c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.set_read_timeout(Some(HEADERS + SLACK)).unwrap();
    c
}

/// A connection to wait on: how long it took to be closed, or why not.
type Case = Box<dyn FnOnce() -> Result<Duration, String> + Send>;

/// Each case on its own thread, so they wait out the limit together.
fn all_closed(cases: Vec<(&'static str, Case)>) {
    let waits: Vec<_> = cases
        .into_iter()
        .map(|(name, case)| (name, std::thread::spawn(case)))
        .collect();
    let mut wrong = Vec::new();
    for (name, wait) in waits {
        match wait.join().unwrap() {
            Ok(took) if took >= HEADERS - Duration::from_secs(1) => {}
            Ok(took) => wrong.push(format!("{name}: closed early, after {took:?}")),
            Err(why) => wrong.push(format!("{name}: {why}")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_request_slower_than_the_header_limit_is_closed_on_the_local_api() {
    use std::io::{Read, Write};
    let m = Machine::new();
    m.ok(&["token", "create", "kişi"]);
    let name = m.listed();
    m.daemon_json(json!({ "listen": "127.0.0.1:0" }));
    let o = m.opened();
    let port = o.local;
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    // a log stream answered before: an answer being written is not a
    // request being read, and stays open past the limit
    let mut stream = connect(port);
    write!(stream, "GET /ws/{name}/log/stream HTTP/1.1\r\n{host}\r\n").unwrap();
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(&head, b"HTTP/1.1 200");
    let h = host.clone();
    all_closed(vec![
        (
            "a client that sends nothing",
            Box::new(move || {
                let mut c = connect(port);
                closed(&mut c, Instant::now())
            }),
        ),
        (
            "a client that sends half its headers",
            Box::new(move || {
                let mut c = connect(port);
                let from = Instant::now();
                write!(c, "GET /health HTTP/1.1\r\n{h}").unwrap();
                closed(&mut c, from)
            }),
        ),
        (
            "a kept-alive connection left idle",
            Box::new(move || {
                let mut c = connect(port);
                write!(c, "GET /livez HTTP/1.1\r\n{host}\r\n").unwrap();
                let mut answer = Vec::new();
                let mut buf = [0u8; 1024];
                while !answer.ends_with(b"ok\n") {
                    let n = c.read(&mut buf).map_err(|e| format!("no answer: {e}"))?;
                    if n == 0 {
                        return Err("closed before its answer".into());
                    }
                    answer.extend_from_slice(&buf[..n]);
                }
                closed(&mut c, Instant::now())
            }),
        ),
    ]);
    // the stream answered before is still open: a read times out (or
    // reads a line the daemon logged), it does not end
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    match stream.read(&mut [0u8; 1024]) {
        Ok(0) => panic!("the log stream was closed"),
        Ok(_) => {}
        Err(e) => assert!(
            matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "{e}"
        ),
    }
}

#[test]
fn a_request_slower_than_the_header_limit_is_closed_on_the_opened_api_over_tls() {
    use rustls::pki_types::{CertificateDer, ServerName, pem::PemObject};
    use std::io::Write;
    let m = Machine::new();
    let token = m.ok(&["token", "create", "kişi"]).trim().to_string();
    let (authority, cert, key) = certificates();
    let (cert, key) = m.tls_files(&cert, &key);
    m.daemon_json(json!({ "listen": "127.0.0.1:0", "tls": { "cert": cert, "key": key } }));
    let o = m.opened();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(authority.as_bytes()).unwrap())
        .unwrap();
    let config = std::sync::Arc::new(
        rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth(),
    );
    let port = o.opened;
    all_closed(vec![(
        "a client past its handshake that sends half its headers",
        Box::new(move || {
            let tls =
                rustls::ClientConnection::new(config, ServerName::try_from("127.0.0.1").unwrap())
                    .unwrap();
            let mut c = rustls::StreamOwned::new(tls, connect(port));
            // the handshake is done once the first bytes are written
            write!(
                c,
                "GET /health HTTP/1.1\r\nHost: itero.örnek.test\r\nAuthorization: Bearer {token}\r\n"
            )
            .map_err(|e| format!("no handshake: {e}"))?;
            c.flush().unwrap();
            closed(&mut c, Instant::now())
        }),
    )]);
}
