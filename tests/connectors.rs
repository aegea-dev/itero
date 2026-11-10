//! Connectors: executables in `connectors/` that keep running, each line they
//! print the data of one message of their file name's type; and `itero
//! watch`, for connectors that watch a file.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "common/stop.rs"]
mod stop;
use itero::connectors::Copy;
use itero::connectors::supervise::{self, Policy};
use itero::engine::daemon::Health;
use itero::engine::journal::Journal;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Long enough for a slow machine: each test waits for what it reads, and
/// this only bounds a wait that would otherwise never end.
const LONG: Duration = Duration::from_secs(20);

/// The lines `out` prints, as they come, until it closes.
fn read_lines(out: impl std::io::Read + Send + 'static) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Waits for `child` to end.
fn ended(child: &mut std::process::Child) -> std::process::ExitStatus {
    let end = Instant::now() + LONG;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < end, "still running");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether `f` holds before `LONG` has passed.
fn until(f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + LONG;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

fn watch(args: &[&std::ffi::OsStr]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_itero"))
        .arg("watch")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// With no command: a line per change, its path and kind; it ends when
/// its stdin closes. (A burst giving one line is the debouncer's, tested
/// in `src/watch`.)
#[test]
fn itero_watch_prints_a_line_per_change_and_ends_with_its_stdin() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "From ayşe\n").unwrap();
    let mut child = watch(&[f.as_os_str()]);
    let stdin = child.stdin.take().unwrap();
    let lines = read_lines(child.stdout.take().unwrap());
    // nothing says when the watch is set: a mail each second until one is told
    let end = Instant::now() + LONG;
    let line = loop {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&f)
            .unwrap()
            .write_all(b"From can\n")
            .unwrap();
        match lines.recv_timeout(Duration::from_secs(1)) {
            Ok(line) => break line,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => panic!("{e}"),
        }
        assert!(Instant::now() < end, "no line for a change");
    };
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["path"], f.to_str().unwrap());
    assert_eq!(v["kind"], "changed");
    drop(stdin);
    assert!(ended(&mut child).success());
}

/// With a command: run once the watch is set, then after each change. Its
/// first run appends to the watched file: a second run follows only if the
/// watch was set before the first one began.
#[test]
fn itero_watch_runs_its_command_once_the_watch_is_set_then_after_each_change() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "From ayşe\n").unwrap();
    let runs = d.path().join("çalıştı");
    let script = d.path().join("oku.sh");
    std::fs::write(
        &script,
        "echo x >> \"$1\"\n\
         n=$(wc -l < \"$1\" | tr -d ' ')\n\
         [ \"$n\" = 1 ] && echo 'From can' >> \"$2\"\n\
         echo \"{\\\"çalışma\\\": $n}\"\n",
    )
    .unwrap();
    let mut child = watch(&[
        f.as_os_str(),
        "--".as_ref(),
        "sh".as_ref(),
        script.as_os_str(),
        runs.as_os_str(),
        f.as_os_str(),
    ]);
    let stdin = child.stdin.take().unwrap();
    let lines = read_lines(child.stdout.take().unwrap());
    // its lines are itero watch's
    assert_eq!(lines.recv_timeout(LONG).unwrap(), r#"{"çalışma": 1}"#);
    assert_eq!(lines.recv_timeout(LONG).unwrap(), r#"{"çalışma": 2}"#);
    drop(stdin);
    assert!(ended(&mut child).success());
}

/// A run that fails ends itero watch, its status in the error.
#[test]
fn itero_watch_ends_when_its_command_fails() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("gelen kutusu");
    std::fs::write(&f, "").unwrap();
    let mut child = watch(&[
        f.as_os_str(),
        "--".as_ref(),
        "sh".as_ref(),
        "-c".as_ref(),
        "exit 3".as_ref(),
    ]);
    let _stdin = child.stdin.take().unwrap();
    let status = ended(&mut child);
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(!status.success(), "{err}");
    assert!(err.contains("exit status: 3"), "{err}");
}

/// When itero watch ends, by its stdin closing or by SIGTERM (how the
/// daemon stops a connector), a run under way ends with it.
#[test]
fn itero_watch_ends_a_run_under_way_when_it_ends() {
    for how in ["stdin", "SIGTERM"] {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("gelen kutusu");
        std::fs::write(&f, "").unwrap();
        let pid_file = d.path().join("süreç");
        let mut child = watch(&[
            f.as_os_str(),
            "--".as_ref(),
            "sh".as_ref(),
            "-c".as_ref(),
            "echo $$ > \"$0.yazılıyor\" && mv \"$0.yazılıyor\" \"$0\" && exec sleep 600".as_ref(),
            pid_file.as_os_str(),
        ]);
        let stdin = child.stdin.take().unwrap();
        assert!(until(|| pid_file.exists()), "{how}: the run did not start");
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        match how {
            "stdin" => drop(stdin),
            // SAFETY: a plain signal to our own child
            _ => unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            },
        }
        ended(&mut child);
        // SAFETY: signal 0 only asks whether the process is there
        assert!(
            until(|| unsafe { libc::kill(pid, 0) } != 0),
            "{how}: the run is still there"
        );
    }
}

fn quick() -> Policy {
    Policy {
        first_pause: Duration::from_millis(10),
        max_pause: Duration::from_millis(40),
        calm_after: Duration::from_secs(60),
        give_up_after: 3,
        window: Duration::from_secs(60),
    }
}

/// A workspace with one connector, `connectors/deneme.başladı`.
fn ws_with_connector(d: &std::path::Path, script: &str) -> itero::workspace::Workspace {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(d.join("itero.config.json"), "{}").unwrap();
    std::fs::create_dir_all(d.join("connectors")).unwrap();
    let s = d.join("connectors/deneme.başladı");
    std::fs::write(&s, script).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    itero::workspace::find(d).unwrap()
}

fn log(ws: &itero::workspace::Workspace) -> itero::engine::log::Log {
    let w = Journal::open(ws).unwrap().workspace().unwrap();
    itero::engine::log::Log::new(ws, &w.id)
}

/// Runs the workspace's connector until it is given up on.
fn run_it(ws: &itero::workspace::Workspace) -> Health {
    let (kind, file) = ws.connector_files().unwrap().remove(0);
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-21T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let connector = Copy::plain(&kind, file.clone());
    supervise::run(
        ws,
        exe,
        &connector,
        &quick(),
        &wake,
        &health,
        &log(ws),
        &Default::default(),
        None,
    );
    health.into_inner().unwrap()
}

#[test]
fn each_line_is_a_message_of_the_connectors_type() {
    let d = tempfile::tempdir().unwrap();
    // `itero` is the daemon's own, found first on its PATH
    let ws = ws_with_connector(
        d.path(),
        "#!/bin/sh\nprintf '{\"konu\": \"çay\"}\\n'\nprintf 'bozuk satır\\n'\nprintf '{\"yer\": \"%s\"}\\n' \"$(pwd)\"\ncommand -v itero >/dev/null && echo '{\"itero\": true}'\nexit 1\n",
    );
    let h = run_it(&ws);
    assert_eq!(h.connectors["deneme.başladı"].state, "gave up");
    let j = Journal::open(&ws).unwrap();
    let mut evs = j.events(Some("itero.app")).unwrap();
    // the last is the daemon's own: the connector was given up on
    assert_eq!(evs.pop().unwrap().event.kind, "itero.connector.gave-up");
    // three lines a start, one bad; four starts before it is given up on
    assert_eq!(evs.len(), 3 * 4, "{evs:?}");
    assert!(evs.iter().all(|e| e.event.kind == "deneme.başladı"));
    assert_eq!(evs[0].event.data["konu"], "çay");
    let here = std::fs::canonicalize(d.path()).unwrap();
    assert_eq!(evs[1].event.data["yer"], here.display().to_string());
    assert_eq!(evs[2].event.data["itero"], true);
}

#[test]
fn a_line_is_one_json_object() {
    assert_eq!(
        supervise::parse_line(r#"{"konu": "çay"}"#).unwrap()["konu"],
        "çay"
    );
    for bad in ["bozuk", "[1]", r#""çay""#] {
        assert!(supervise::parse_line(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_connector_that_cannot_start_is_given_up_on_with_why() {
    let d = tempfile::tempdir().unwrap();
    let ws = ws_with_connector(d.path(), "#!/bin/sh\n");
    let (kind, file) = ws.connector_files().unwrap().remove(0);
    std::fs::remove_file(&file).unwrap();
    let (wake, _woken) = std::sync::mpsc::channel();
    let health = Mutex::new(Health::starting("2026-10-21T10:00:00Z"));
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_itero"));
    let connector = Copy::plain(&kind, file.clone());
    supervise::run(
        &ws,
        exe,
        &connector,
        &quick(),
        &wake,
        &health,
        &log(&ws),
        &Default::default(),
        None,
    );
    let h = health.into_inner().unwrap();
    let s = &h.connectors["deneme.başladı"];
    assert_eq!(s.state, "gave up");
    assert!(
        s.last_exit.as_deref().unwrap().starts_with("error: "),
        "{s:?}"
    );
    let log = std::fs::read_to_string(d.path().join(".itero/itero.log")).unwrap();
    let last: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(last["event"], "connector.gave-up");
    assert_eq!(last["connector"], "deneme.başladı");
    let own = Journal::open(&ws)
        .unwrap()
        .events(Some("itero.app"))
        .unwrap();
    let gave_up = own.last().unwrap();
    assert_eq!(gave_up.event.kind, "itero.connector.gave-up");
    assert_eq!(gave_up.event.data["connector"], "deneme.başladı");
}

#[test]
fn after_an_end_the_pause_doubles_resets_when_calm_and_gives_up() {
    let p = Policy {
        first_pause: Duration::from_secs(1),
        max_pause: Duration::from_secs(4),
        calm_after: Duration::from_secs(300),
        give_up_after: 3,
        window: Duration::from_secs(600),
    };
    let now = Instant::now();
    let mut ends = vec![];
    let quick = Duration::from_secs(1);
    let mut pause = Duration::ZERO;
    let mut pauses = vec![];
    for _ in 0..3 {
        pause = p.after_end(&mut ends, now, quick, pause).unwrap();
        pauses.push(pause.as_secs());
    }
    assert_eq!(pauses, vec![1, 2, 4]);
    // the fourth end within the window: given up
    assert_eq!(p.after_end(&mut ends, now, quick, pause), None);
    // a calm run sets the pause back; ends past the window are forgotten
    let later = now + Duration::from_secs(700);
    let mut ends = vec![now, now];
    assert_eq!(
        p.after_end(
            &mut ends,
            later,
            Duration::from_secs(400),
            Duration::from_secs(4)
        ),
        Some(Duration::from_secs(1))
    );
    assert_eq!(ends.len(), 1);
}

/// A workspace whose connector never reads its stdin and never prints: it
/// notes its pid in `kulak.pid`, then sleeps. A workflow listens to it.
fn deaf_connector(d: &std::path::Path) -> std::path::PathBuf {
    connector_running(d, "#!/bin/sh\necho $$ > kulak.pid\nexec sleep 600\n")
}

/// A workspace whose connector is `script`, which notes in `kulak.pid` the
/// pid that is to end with the daemon.
fn connector_running(d: &std::path::Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let root = d.join("çalışma");
    for dir in ["connectors", "workflows", "actions"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("itero.config.json"), "{}").unwrap();
    let exec = |p: std::path::PathBuf, text: &str| {
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    exec(root.join("connectors/kulak.duydu"), script);
    exec(root.join("actions/yaz"), "#!/bin/sh\nexit 0\n");
    std::fs::write(
        root.join("workflows/dinle.json"),
        r##"{ "on": [{ "event": "kulak.duydu" }], "steps": ["#yaz"] }"##,
    )
    .unwrap();
    root
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the process is there
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Starts `itero <args>` in `root`, waits for its connector's pid, stops
/// the daemon as Ctrl-C does, and says whether the connector ended too.
fn connector_ends_with(d: &std::path::Path, root: &std::path::Path, args: &[&str]) {
    let itero = |dir: &std::path::Path| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_itero"));
        c.current_dir(dir)
            .env("HOME", d.join("ev"))
            .env("XDG_STATE_HOME", d.join("durum"))
            .env("ITERO_NO_SECRET_STORE", "1")
            .env("ITERO_NOTIFIER", "/usr/bin/true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    };
    if args.contains(&"--all") {
        assert!(
            itero(root)
                .args(["workspace", "add"])
                .status()
                .unwrap()
                .success()
        );
    }
    // stopped however the test ends
    let daemon = stop::Daemon(
        itero(if args.contains(&"--all") { d } else { root })
            .args(args)
            .spawn()
            .unwrap(),
    );
    let pid_file = root.join("kulak.pid");
    assert!(
        until(|| std::fs::read_to_string(&pid_file).is_ok_and(|t| t.trim().parse::<i32>().is_ok())),
        "the connector never started"
    );
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(pid));
    // as Ctrl-C does: SIGTERM, then waited for
    drop(daemon);
    let gone = until(|| !alive(pid));
    if !gone {
        // SAFETY: the connector this test started
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(gone, "{args:?}: the connector {pid} outlived its daemon");
}

#[test]
fn a_connector_deaf_to_its_stdin_ends_with_its_daemon() {
    let d = tempfile::tempdir().unwrap();
    let root = deaf_connector(d.path());
    connector_ends_with(d.path(), &root, &["ws", "daemon", "--no-serve"]);
}

#[test]
fn a_connector_deaf_to_its_stdin_ends_with_the_machines_daemon() {
    let d = tempfile::tempdir().unwrap();
    let root = deaf_connector(d.path());
    connector_ends_with(d.path(), &root, &["daemon", "--all", "--no-serve"]);
}

#[test]
fn what_a_connector_runs_ends_with_its_daemon_too() {
    let d = tempfile::tempdir().unwrap();
    // the script waits on a program it started: a pipeline's, say
    let root = connector_running(
        d.path(),
        "#!/bin/sh\nsleep 600 &\necho $! > kulak.pid\nwait\n",
    );
    connector_ends_with(d.path(), &root, &["ws", "daemon", "--no-serve"]);
}
