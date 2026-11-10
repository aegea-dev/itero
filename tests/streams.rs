//! Following a workspace live: `GET /events/stream` and `GET /log/stream`,
//! on a real daemon.
#![cfg(unix)]
#[path = "common/stop.rs"]
mod stop;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use stop::Daemon;

/// Long enough for a slow machine: each test waits for what it reads, and
/// this only bounds a wait that would otherwise never end.
const LONG: Duration = Duration::from_secs(20);

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + LONG;
    while !f() {
        assert!(Instant::now() < end, "waited {LONG:?} for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http() -> reqwest::blocking::Client {
    // a stream is read for as long as a test needs it
    itero::net::client(Duration::from_secs(600)).unwrap()
}

/// A workspace whose workflows run the action `yaz` on `sipariş.verildi`
/// and on `kapı.açıldı`, and whose connector `kapı.açıldı` tells one door
/// once the file `açık` is in it. No workflow is on `hava.değişti`: those
/// start no run, so no `itero.run.done` comes among them.
struct Ws {
    d: tempfile::TempDir,
    root: PathBuf,
    port: u16,
}

impl Ws {
    fn new() -> Ws {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("çalışma");
        for dir in ["workflows", "actions", "connectors"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root = root.canonicalize().unwrap();
        let port = free_port();
        std::fs::write(
            root.join("itero.config.json"),
            json!({ "port": port, "workflow": { "keep": { "off": true }, "models": { "off": true } } })
                .to_string(),
        )
        .unwrap();
        for (name, kind) in [("sipariş", "sipariş.verildi"), ("kapı", "kapı.açıldı")] {
            std::fs::write(
                root.join(format!("workflows/{name}.json")),
                json!({ "on": [{ "event": kind }], "steps": ["#yaz"] }).to_string(),
            )
            .unwrap();
        }
        let exec = |path: &Path, text: String| {
            std::fs::write(path, text).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exec(&root.join("actions/yaz"), "#!/bin/sh\nexit 0\n".into());
        exec(
            &root.join("connectors/kapı.açıldı"),
            format!(
                "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\necho '{{\"kapı\": \"ön\"}}'\nexec sleep 600\n",
                root.join("açık").display()
            ),
        );
        Ws { d, root, port }
    }

    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(dir)
            .env("HOME", self.d.path().join("ev"))
            .env("XDG_STATE_HOME", self.d.path().join("durum"))
            .env("XDG_CACHE_HOME", self.d.path().join("önbellek"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        c
    }

    /// `itero daemon` in the workspace, once it answers.
    fn daemon(&self) -> Daemon {
        let child = self
            .command(&self.root)
            .args(["ws", "daemon"])
            .stderr(std::fs::File::create(self.d.path().join("söylenen")).unwrap())
            .spawn()
            .unwrap();
        let d = Daemon(child);
        until("the daemon to answer", || {
            http()
                .get(self.url("/health"))
                .send()
                .is_ok_and(|r| r.status() == 200)
        });
        d
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Posts an event; its id.
    fn post(&self, kind: &str, data: Value) -> String {
        let r = http()
            .post(self.url("/events"))
            .json(&json!({ "type": kind, "data": data }))
            .send()
            .unwrap();
        assert_eq!(r.status(), 202);
        r.json::<Value>().unwrap()["id"].as_str().unwrap().into()
    }

    /// `/health`'s `streams`.
    fn streams(&self) -> Value {
        let v: Value = http()
            .get(self.url("/health"))
            .send()
            .unwrap()
            .json()
            .unwrap();
        v["streams"].clone()
    }
}

/// A stream's lines as they come; `None` once it has ended.
struct Listener(Receiver<Option<String>>);

/// Follows `url`: its status, and its lines when it is 200.
fn listen(url: &str) -> (u16, Listener) {
    let r = http().get(url).send().unwrap();
    let status = r.status().as_u16();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(Some(line)).is_err() {
                return;
            }
        }
        let _ = tx.send(None);
    });
    (status, Listener(rx))
}

/// Follows `url`, which must answer 200.
fn follow(url: &str) -> Listener {
    let (status, l) = listen(url);
    assert_eq!(status, 200, "{url}");
    l
}

impl Listener {
    /// The next line, as JSON.
    fn next(&self) -> Value {
        match self.0.recv_timeout(LONG) {
            Ok(Some(line)) => serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}")),
            Ok(None) => panic!("the stream ended"),
            Err(e) => panic!("no line in {LONG:?}: {e}"),
        }
    }

    /// Nothing more within `wait`.
    fn quiet(&self, wait: Duration) {
        match self.0.recv_timeout(wait) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            other => panic!("expected nothing, got {other:?}"),
        }
    }

    /// The stream ends, with no line before.
    fn ends(&self) {
        match self.0.recv_timeout(LONG) {
            Ok(None) => {}
            other => panic!("expected the end, got {other:?}"),
        }
    }
}

#[test]
fn every_event_that_comes_in_is_followed_in_order() {
    let w = Ws::new();
    let _d = w.daemon();
    let l = follow(&w.url("/events/stream"));
    assert_eq!(w.streams()["events"], 1);
    let çay = w.post("sipariş.verildi", json!({ "ürün": "çay" }));
    let şeker = w.post("sipariş.verildi", json!({ "ürün": "şeker" }));
    // from another process
    let emitted = w
        .command(&w.root)
        .args(["ws", "emit", "sipariş.verildi", r#"{"ürün": "bal"}"#])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(emitted.success());
    // the connector tells once the door is open
    std::fs::write(w.root.join("açık"), "").unwrap();
    // the four, and itero's own itero.run.done as each run they started
    // ends (an event workflows can be on, too), in the order they came
    let (mut came, mut done) = (Vec::new(), Vec::new());
    while came.len() < 4 || done.len() < 4 {
        let e = l.next();
        assert_eq!(e.as_object().unwrap().len(), 5, "{e}");
        assert!(e["time"].as_str().unwrap().starts_with("20"), "{e}");
        if e["type"] == "itero.run.done" {
            assert_eq!(e["source"], "daemon");
            done.push(e["data"]["workflow"].as_str().unwrap().to_string());
        } else {
            came.push(e);
        }
    }
    for (e, ürün) in came[..3].iter().zip(["çay", "şeker", "bal"]) {
        assert_eq!(e["type"], "sipariş.verildi");
        assert_eq!(e["source"], "outside");
        assert_eq!(e["data"], json!({ "ürün": ürün }));
    }
    assert_eq!(came[0]["id"], çay.as_str());
    assert_eq!(came[1]["id"], şeker.as_str());
    assert_eq!(came[3]["type"], "kapı.açıldı");
    assert_eq!(came[3]["source"], "connector/kapı.açıldı");
    assert_eq!(came[3]["data"], json!({ "kapı": "ön" }));
    done.sort();
    assert_eq!(done, ["kapı", "sipariş", "sipariş", "sipariş"]);
    // the runs' inner messages (their jobs, their ends) are not sent
    l.quiet(Duration::from_secs(1));
}

#[test]
fn after_an_event_the_rest_come_then_new_ones_and_a_gone_one_is_410() {
    let w = Ws::new();
    let _d = w.daemon();
    let ids: Vec<String> = ["bir", "iki", "üç", "dört", "beş"]
        .iter()
        .map(|n| w.post("hava.değişti", json!({ "sıra": n })))
        .collect();
    let l = follow(&w.url(&format!("/events/stream?after={}", ids[1])));
    for id in &ids[2..] {
        assert_eq!(&l.next()["id"], id.as_str());
    }
    let altı = w.post("hava.değişti", json!({ "sıra": "altı" }));
    assert_eq!(l.next()["id"], altı.as_str());

    let r = http()
        .get(w.url("/events/stream?after=yok-böyle-bir-olay"))
        .send()
        .unwrap();
    assert_eq!(r.status(), 410);
    let v: Value = r.json().unwrap();
    assert!(
        v["error"].as_str().unwrap().contains("yok-böyle-bir-olay"),
        "{v}"
    );
}

#[test]
fn four_listeners_each_get_every_event() {
    let w = Ws::new();
    let _d = w.daemon();
    let ls: Vec<Listener> = (0..4).map(|_| follow(&w.url("/events/stream"))).collect();
    assert_eq!(w.streams()["events"], 4);
    let ids: Vec<String> = (0..20)
        .map(|i| w.post("hava.değişti", json!({ "sıra": i })))
        .collect();
    for l in &ls {
        for id in &ids {
            assert_eq!(&l.next()["id"], id.as_str());
        }
    }
}

#[test]
fn a_listener_that_goes_is_let_go() {
    let w = Ws::new();
    let _d = w.daemon();
    let r = http().get(w.url("/events/stream")).send().unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(w.streams(), json!({ "events": 1, "log": 0 }));
    drop(r);
    until("the stream to be let go", || w.streams()["events"] == 0);
}

#[test]
fn type_and_count_pick_the_events_and_end_it() {
    let w = Ws::new();
    let _d = w.daemon();
    let l = follow(&w.url(
        "/events/stream?type=sipari%C5%9F.verildi&type=kap%C4%B1.a%C3%A7%C4%B1ld%C4%B1&count=2",
    ));
    let one = follow(&w.url("/events/stream?count=1"));
    w.post("hava.değişti", json!({ "hava": "yağmur" }));
    let çay = w.post("sipariş.verildi", json!({ "ürün": "çay" }));
    w.post("hava.değişti", json!({ "hava": "güneş" }));
    let kapı = w.post("kapı.açıldı", json!({ "kapı": "arka" }));
    w.post("sipariş.verildi", json!({ "ürün": "şeker" }));
    assert_eq!(l.next()["id"], çay.as_str());
    assert_eq!(l.next()["id"], kapı.as_str());
    l.ends();
    assert_eq!(one.next()["type"], "hava.değişti");
    one.ends();
    until("both let go", || w.streams()["events"] == 0);
}

#[test]
fn bad_parameters_are_400_with_every_problem() {
    let w = Ws::new();
    let _d = w.daemon();
    let r = http()
        .get(w.url("/events/stream?every=1h&count=0&type=tek&bilinmeyen=1"))
        .send()
        .unwrap();
    assert_eq!(r.status(), 400);
    let v: Value = r.json().unwrap();
    let why = v["error"].as_str().unwrap();
    for says in ["every", "count", "tek", "bilinmeyen"] {
        assert!(why.contains(says), "{says}: {why}");
    }
    assert_eq!(why.matches("; ").count(), 3, "{why}");
    let r = http()
        .get(w.url("/log/stream?after=x&type=a.b"))
        .send()
        .unwrap();
    assert_eq!(r.status(), 400);
    let v: Value = r.json().unwrap();
    let why = v["error"].as_str().unwrap();
    assert!(why.contains("after") && why.contains("type"), "{why}");
}

/// How long each of three events, posted 350 ms apart, took to come.
fn waits(w: &Ws, every: &str) -> Vec<Duration> {
    let l = follow(&w.url(&format!("/events/stream?every={every}")));
    (0..3)
        .map(|i| {
            let at = Instant::now();
            w.post("hava.değişti", json!({ "sıra": i }));
            l.next();
            let took = at.elapsed();
            std::thread::sleep(Duration::from_millis(350).saturating_sub(took));
            took
        })
        .collect()
}

#[test]
fn every_sets_how_often_it_looks() {
    let w = Ws::new();
    let _d = w.daemon();
    // looking each second, three events over 0.7 s cannot all come within
    // 0.2 s: at most one look falls among them
    let slow = waits(&w, "1s");
    assert!(
        slow.iter().any(|t| *t >= Duration::from_millis(200)),
        "{slow:?}"
    );
    assert!(slow.iter().all(|t| *t < Duration::from_secs(3)), "{slow:?}");
    let quick = waits(&w, "10ms");
    assert!(
        quick.iter().all(|t| *t < Duration::from_secs(1)),
        "{quick:?}"
    );
}

#[test]
fn the_stream_works_through_the_machine_api() {
    let w = Ws::new();
    let added = w
        .command(&w.root)
        .args(["workspace", "add"])
        .status()
        .unwrap();
    assert!(added.success());
    let ws = itero::workspace::find(&w.root).unwrap();
    let name = itero::engine::journal::Journal::open(&ws)
        .unwrap()
        .workspace()
        .unwrap()
        .name;
    let port = free_port();
    let _d = Daemon(
        w.command(w.d.path())
            .args(["daemon", "--config", &server_file(&w, port)])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}/ws/{name}");
    until("the workspace to run", || {
        http()
            .get(format!("{base}/health"))
            .send()
            .is_ok_and(|r| r.status() == 200)
    });
    let l = follow(&format!("{base}/events/stream"));
    let r = http()
        .post(format!("{base}/events"))
        .json(&json!({ "type": "hava.değişti", "data": { "ürün": "çay" } }))
        .send()
        .unwrap();
    assert_eq!(r.status(), 202);
    assert_eq!(l.next()["data"], json!({ "ürün": "çay" }));
}

/// The `message` lines among the log lines `l` sends, by their event's
/// id, until `n` have come.
fn messages(l: &Listener, n: usize) -> Vec<String> {
    let mut ids = Vec::new();
    while ids.len() < n {
        let line = l.next();
        assert!(line["event"].is_string(), "{line}");
        if line["event"] == "message" && line["type"] == "hava.değişti" {
            ids.push(line["message"].as_str().unwrap().to_string());
        }
    }
    ids
}

#[test]
fn new_log_lines_are_followed_and_a_roll_loses_none() {
    let w = Ws::new();
    std::fs::write(
        w.root.join("workflows/döndür.json"),
        json!({ "on": [{ "event": "günlük.döndür" }], "steps": ["#roll-log"] }).to_string(),
    )
    .unwrap();
    let _d = w.daemon();
    let l = follow(&w.url("/log/stream"));
    assert_eq!(w.streams()["log"], 1);
    let before: Vec<String> = (0..3)
        .map(|i| w.post("hava.değişti", json!({ "sıra": i })))
        .collect();
    assert_eq!(messages(&l, 3), before);
    w.post("günlük.döndür", json!({}));
    let log = w.root.join(".itero");
    until("the roll", || log.join("itero.log.1").is_file());
    let after: Vec<String> = (3..6)
        .map(|i| w.post("hava.değişti", json!({ "sıra": i })))
        .collect();
    // none lost, none twice, across the roll; the roll's own run may
    // still write after it
    assert_eq!(messages(&l, 3), after);
    while let Ok(Some(line)) = l.0.recv_timeout(Duration::from_millis(500)) {
        let line: Value = serde_json::from_str(&line).unwrap();
        let again = line["event"] == "message" && line["type"] == "hava.değişti";
        assert!(!again, "{line}");
    }
}

#[test]
fn a_half_written_line_comes_once_it_ends() {
    use std::io::Write;
    let w = Ws::new();
    let _d = w.daemon();
    // one line asked: a half sent as one would end the stream there
    let l = follow(&w.url("/log/stream?count=1"));
    let path = w.root.join(".itero/itero.log");
    let append = |text: &str| {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    };
    append(r#"{"event":"yarım"#);
    l.quiet(Duration::from_millis(300));
    append("\"}\n");
    assert_eq!(l.next(), json!({ "event": "yarım" }));
    l.ends();
}

#[test]
fn the_log_stream_ends_after_its_count() {
    let w = Ws::new();
    let _d = w.daemon();
    let l = follow(&w.url("/log/stream?count=2"));
    w.post("hava.değişti", json!({}));
    w.post("hava.değişti", json!({}));
    w.post("hava.değişti", json!({}));
    l.next();
    l.next();
    l.ends();
    until("let go", || w.streams()["log"] == 0);
}

#[test]
fn the_log_stream_works_through_the_machine_api() {
    let w = Ws::new();
    let added = w
        .command(&w.root)
        .args(["workspace", "add"])
        .status()
        .unwrap();
    assert!(added.success());
    let ws = itero::workspace::find(&w.root).unwrap();
    let name = itero::engine::journal::Journal::open(&ws)
        .unwrap()
        .workspace()
        .unwrap()
        .name;
    let port = free_port();
    let _d = Daemon(
        w.command(w.d.path())
            .args(["daemon", "--config", &server_file(&w, port)])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://127.0.0.1:{port}/ws/{name}");
    until("the workspace to run", || {
        http()
            .get(format!("{base}/health"))
            .send()
            .is_ok_and(|r| r.status() == 200)
    });
    let l = follow(&format!("{base}/log/stream"));
    let r = http()
        .post(format!("{base}/events"))
        .json(&json!({ "type": "hava.değişti" }))
        .send()
        .unwrap();
    assert_eq!(r.status(), 202);
    let id: Value = r.json().unwrap();
    assert_eq!(messages(&l, 1), [id["id"].as_str().unwrap()]);
}

/// A `server.json` in the test's folder saying `port`, for `--config`.
fn server_file(w: &Ws, port: u16) -> String {
    let p = w.d.path().join(format!("sunucu-{port}.json"));
    std::fs::write(&p, format!(r#"{{ "port": {port} }}"#)).unwrap();
    p.display().to_string()
}
