//! The daemon over the simple files, end to end: messages start runs, steps
//! run in workers, waiting runs get their messages, and runs end as done,
//! failed or error.
#![cfg(unix)]
mod common;
use chrono::{DateTime, Utc};
use itero::engine::daemon::{Pass, next_due, run_once};
use itero::engine::journal::{Journal, Run};
use itero::engine::steps;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

const NOW: &str = "2026-10-21T10:00:00Z";

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_itero"))
}

/// A workspace in a temporary folder.
struct Ws {
    d: tempfile::TempDir,
    ws: itero::workspace::Workspace,
}

impl Ws {
    fn new() -> Ws {
        Ws::with("{}")
    }

    /// A workspace whose itero.config.json says `manifest`.
    fn with(manifest: &str) -> Ws {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.config.json"), manifest).unwrap();
        let ws = itero::workspace::find(d.path()).unwrap();
        Ws { d, ws }
    }

    fn write(&self, rel: &str, text: &str) {
        let f = self.d.path().join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }

    fn flow(&self, name: &str, v: Value) {
        self.write(&format!("workflows/{name}.json"), &v.to_string());
    }

    /// An action: a shell script, run in the workspace.
    fn action(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let rel = format!("actions/{name}");
        self.write(&rel, &format!("#!/bin/sh\n{body}\n"));
        let f = self.d.path().join(rel);
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// An action's args, in the manifest; `Null` takes them out.
    fn args(&self, action: &str, v: Value) {
        let f = self.d.path().join("itero.config.json");
        let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        m["action"][action]["args"] = v;
        std::fs::write(f, m.to_string()).unwrap();
    }

    fn emit(&self, kind: &str, data: Value) {
        itero::engine::events::emit(&self.ws, kind, Some(data), NOW).unwrap();
    }

    fn pass(&self, now: &str) -> Pass {
        run_once(&self.ws, now.parse().unwrap(), exe()).unwrap()
    }

    /// When the daemon next has something to do with no message.
    fn next_due(&self, now: &str) -> Option<DateTime<Utc>> {
        next_due(&self.ws, now.parse().unwrap()).unwrap()
    }

    fn runs(&self) -> Vec<Run> {
        Journal::open(&self.ws).unwrap().runs().unwrap()
    }

    fn statuses(&self) -> Vec<&'static str> {
        self.runs()
            .iter()
            .map(|r| steps::status(&r.runtime))
            .collect()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.d.path().join(rel)).unwrap_or_default()
    }

    /// The log's lines, those written whole.
    fn log(&self) -> Vec<Value> {
        common::log_lines(&self.read(".itero/itero.log"))
    }

    /// The types of the messages the log tells of.
    fn messages(&self) -> Vec<String> {
        self.log()
            .iter()
            .filter(|l| l["event"] == "message")
            .map(|l| l["type"].as_str().unwrap().to_string())
            .collect()
    }
}

/// The order of the design: payment and stock side by side, the payment
/// waiting for its provider, then the order's processing and a mail. Each
/// result is passed on as the next step's args.
fn order(ws: &Ws) {
    ws.flow(
        "order",
        json!({
            "on": [{ "event": "order.placed" }],
            "steps": [
                ["#check-payment", "#check-stock"],
                {"order-process": {"args": "$output"}},
                {"#send-mail": {"args": "$output"}}
            ],
            "success": ["#log-result"],
            "failure": ["#log-result"],
            "error": ["#log-result"]
        }),
    );
    ws.flow(
        "order-process",
        json!({
            "on": [{ "has": ["$paid", "$stock"] }],
            "steps": [
                {"#ship": {"on": [{"$paid": true, "$stock": true}]}},
                {"#remind-stock": {"on": [{"$paid": true, "$stock": false}]}}
            ]
        }),
    );
    // the payment's message, when it came, is its output
    ws.action(
        "check-payment",
        r#"case "$output" in *'"paid":true'*) echo '{"paid": true}'; exit 0 ;; esac
echo '{"payment_ref": "ödeme-1"}'
exit 75"#,
    );
    ws.action("check-stock", r#"echo "{\"stock\": $in_stock}""#);
    ws.action("ship", r#"echo '{"tracking": "TR123"}'"#);
    ws.action("remind-stock", r#"echo '{"success": false}'"#);
    ws.action(
        "send-mail",
        r#"echo "sipariş $order: ${tracking:-kargo yok}" >> gönderilen.log"#,
    );
    ws.action(
        "log-result",
        r#"echo "${success:-true} ${failed_step:-} ${why:-}" >> sonuç.log"#,
    );
}

#[test]
fn an_order_waits_for_its_payment_then_ships() {
    let ws = Ws::new();
    order(&ws);
    ws.emit("order.placed", json!({"order": 43, "in_stock": true}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"]);
    let run = &ws.runs()[0];
    assert!(
        !run.runtime.state.contains_key("payment_ref"),
        "nothing merges"
    );
    // another order's payment: not this run's
    ws.emit(
        "payment.received",
        json!({"payment_ref": "ödeme-2", "paid": true}),
    );
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"]);
    ws.emit(
        "payment.received",
        json!({"payment_ref": "ödeme-1", "paid": true}),
    );
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.read("gönderilen.log"), "sipariş 43: TR123\n");
    assert_eq!(ws.read("sonuç.log"), "true  \n");
}

#[test]
fn an_order_out_of_stock_ends_as_failed() {
    let ws = Ws::new();
    order(&ws);
    ws.emit("order.placed", json!({"order": 44, "in_stock": false}));
    ws.emit(
        "payment.received",
        json!({"payment_ref": "ödeme-1", "paid": true}),
    );
    // the payment came before the step waited for it: it still reaches it
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["failed"]);
    assert_eq!(ws.read("gönderilen.log"), "sipariş 44: kargo yok\n");
    assert_eq!(ws.read("sonuç.log"), "false  \n");
}

#[test]
fn an_action_that_breaks_runs_the_error_list() {
    let ws = Ws::new();
    order(&ws);
    ws.action("check-stock", "echo 'depo kapalı' >&2; exit 3");
    ws.emit("order.placed", json!({"order": 45, "in_stock": true}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["error"]);
    assert_eq!(
        ws.read("sonuç.log"),
        "true #check-stock exit 3: depo kapalı\n"
    );
    let j = Journal::open(&ws.ws).unwrap();
    let errors = j.runs_page(None, Some("error"), &[], 10).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].runtime.state["failed_step"], "#check-stock");
}

#[test]
fn a_message_reaches_every_waiting_run_it_matches() {
    let ws = Ws::new();
    ws.flow(
        "bekle",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#stok-bekle", "#bitir"]}),
    );
    // it waits for news of its product, which the news is matched on
    ws.action(
        "stok-bekle",
        r#"case "$output" in *'"arrived":true'*) exit 0 ;; esac
echo "{\"product\": \"$product\"}"; exit 75"#,
    );
    ws.action("bitir", r#"echo "$order" >> biten.log"#);
    ws.emit("order.placed", json!({"order": 1, "product": "çay"}));
    ws.emit("order.placed", json!({"order": 2, "product": "çay"}));
    ws.emit("order.placed", json!({"order": 3, "product": "kahve"}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"; 3]);
    ws.emit("stock.arrived", json!({"product": "çay", "arrived": true}));
    ws.pass(NOW);
    let mut done: Vec<String> = ws.read("biten.log").lines().map(str::to_string).collect();
    done.sort();
    assert_eq!(done, vec!["1", "2"]);
}

#[test]
fn every_workflow_on_a_message_starts_a_run() {
    let ws = Ws::new();
    for n in ["bir", "iki", "üç", "dört", "beş"] {
        ws.flow(
            n,
            json!({"on": [{ "event": "çay.demlendi" }], "steps": ["#hiç"]}),
        );
    }
    ws.action("hiç", "true");
    ws.emit("çay.demlendi", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"; 5]);
}

#[test]
fn a_value_comes_from_the_step_then_args_then_the_state() {
    let ws = Ws::new();
    ws.action("renk", r#"echo "$renk" >> renkler.log"#);
    ws.flow(
        "adım",
        json!({"on": [{ "event": "renk.sor" }], "steps": [{"#renk": {"args": {"renk": "adımın"}}}]}),
    );
    ws.emit("renk.sor", json!({"renk": "durumun"}));
    ws.pass(NOW);
    ws.args("renk", json!({"renk": "args'ın"}));
    ws.flow(
        "adım",
        json!({"on": [{ "event": "renk.sor" }], "steps": ["#renk"]}),
    );
    ws.emit("renk.sor", json!({"renk": "durumun"}));
    ws.pass(NOW);
    ws.args("renk", Value::Null);
    ws.emit("renk.sor", json!({"renk": "durumun"}));
    ws.pass(NOW);
    assert_eq!(ws.read("renkler.log"), "adımın\nargs'ın\ndurumun\n");
}

#[test]
fn a_state_value_can_be_named_in_a_steps_data() {
    let ws = Ws::new();
    ws.action("selam", r#"echo "$kime" >> selam.log"#);
    ws.flow(
        "selam",
        json!({"on": [{ "event": "selam.ver" }], "steps": [{"#selam": {"args": {"kime": "$müşteri"}}}]}),
    );
    ws.emit("selam.ver", json!({"müşteri": "Ayşe"}));
    ws.pass(NOW);
    assert_eq!(ws.read("selam.log"), "Ayşe\n");
}

#[test]
fn a_run_that_waits_past_its_timeout_ends_in_error() {
    let ws = Ws::new();
    ws.flow(
        "bekle",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#bekle"], "timeout": "1m"}),
    );
    ws.action("bekle", "exit 75");
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"]);
    ws.pass("2026-10-21T10:02:00Z");
    assert_eq!(ws.statuses(), vec!["error"]);
    assert_eq!(ws.runs()[0].runtime.state["why"], "waited 1m for data");
}

#[test]
fn steps_side_by_side_that_print_plain_values_are_read_by_name() {
    let ws = Ws::new();
    ws.flow(
        "fotoğraf",
        json!({"on": [{ "event": "photo.added" }],
               "steps": [["#boyut", "#tür"],
                         { "#yaz": { "args": { "metin": "$output.#boyut bayt, $output.#tür" } } }]}),
    );
    ws.action("boyut", "echo 1064");
    ws.action("tür", "echo image/png");
    ws.action("yaz", "echo \"$metin\" > yazılan.txt");
    ws.emit("photo.added", json!({"path": "p.png"}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.read("yazılan.txt"), "1064 bayt, image/png\n");
}

/// Whether process `pid` still runs: one ended and not yet reaped (in a
/// container, nothing may reap it) does not.
fn alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(") ").and_then(|(_, r)| r.chars().next()));
    #[cfg(not(target_os = "linux"))]
    let state = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().chars().next());
    state.is_some_and(|c| c != 'Z')
}

/// The pid an action wrote to `file`, once it is there.
fn pid_in(ws: &Ws, file: &str) -> u32 {
    assert!(soon(|| !ws.read(file).trim().is_empty()), "no {file}");
    ws.read(file).trim().parse().unwrap()
}

/// The journal's jobs: each one's action, whether it was handled, and its
/// note.
fn jobs(ws: &Ws) -> Vec<(String, bool, Option<String>)> {
    Journal::open(&ws.ws)
        .unwrap()
        .events(Some("itero.worker"))
        .unwrap()
        .into_iter()
        .map(|r| {
            let action = r.event.data["job"]["action"].as_str().unwrap().to_string();
            (action, r.handled.is_some(), r.note)
        })
        .collect()
}

/// The one job of `action`, saved as stopped.
fn saved_as_stopped(ws: &Ws, action: &str) {
    let found: Vec<_> = jobs(ws).into_iter().filter(|j| j.0 == action).collect();
    assert_eq!(found.len(), 1, "{found:?}");
    let (_, handled, note) = &found[0];
    assert!(handled, "{found:?}");
    assert!(
        note.as_deref().unwrap_or("").contains("stopped"),
        "{found:?}"
    );
}

#[test]
fn a_race_ends_with_its_first_step_and_stops_the_slow_ones_process() {
    let ws = Ws::new();
    ws.flow(
        "yarış",
        json!({"on": [{ "event": "yarış.başladı" }],
               "steps": [{ "race": ["#hızlı", "#yavaş"] }]}),
    );
    ws.action("hızlı", "sleep 0.5; echo hızlı");
    // its sleep runs apart from its shell, in its process group
    ws.action("yavaş", "sleep 30 &\necho $! > uyku.pid\nwait");
    ws.emit("yarış.başladı", json!({}));
    let started = std::time::Instant::now();
    let pass = ws.pass(NOW);
    let took = started.elapsed();
    let pid = pid_in(&ws, "uyku.pid");
    assert!(soon(|| !alive(pid)), "sleep {pid} still runs");
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(
        ws.runs()[0].runtime.state["output"],
        json!({"#hızlı": "hızlı", "winner": "#hızlı"})
    );
    // the slow one is saved as stopped, not lost: it does not run again
    assert_eq!(pass.lost, 0);
    saved_as_stopped(&ws, "yavaş");
    ws.pass(NOW);
    saved_as_stopped(&ws, "yavaş");
    assert!(ws.log().iter().any(|l| l["event"] == "step.stopped"));
}

#[test]
fn a_step_stopped_before_it_started_never_runs() {
    // one step at a time: the slow one waits its turn, and loses meanwhile
    let ws = Ws::with(r#"{"limits": {"steps": 1}}"#);
    ws.flow(
        "yarış",
        json!({"on": [{ "event": "yarış.başladı" }],
               "steps": [{ "race": ["#hızlı", "#yavaş"] }]}),
    );
    ws.action("hızlı", "echo hızlı");
    ws.action("yavaş", "echo koştu > yavaş.txt");
    ws.emit("yarış.başladı", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.read("yavaş.txt"), "");
    saved_as_stopped(&ws, "yavaş");
}

#[test]
fn any_waits_for_the_slow_step_when_the_fast_one_breaks() {
    let ws = Ws::new();
    ws.flow(
        "biri",
        json!({"on": [{ "event": "biri.başladı" }],
               "steps": [{ "any": ["#kırık", "#yavaş"] }]}),
    );
    ws.action("kırık", "echo bozuldu >&2; exit 1");
    ws.action("yavaş", "sleep 1; echo yavaş");
    ws.emit("biri.başladı", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(
        ws.runs()[0].runtime.state["output"],
        json!({"#yavaş": "yavaş", "winner": "#yavaş",
               "failed": ["#kırık"], "why": {"#kırık": "exit 1: bozuldu"}})
    );
}

#[test]
fn a_waiting_step_that_lost_a_race_is_not_woken_by_a_later_message() {
    let ws = Ws::new();
    ws.flow(
        "bekleyen",
        json!({"on": [{ "event": "yarış.başladı" }],
               "steps": [{ "race": ["#bekle", "#hızlı"] }, "#sonra"]}),
    );
    // both wait for the same message: only the step still in the run gets it
    ws.action(
        "bekle",
        r#"echo uyandı >> bekle.log; echo '{"sipariş": "ş-1"}'; exit 75"#,
    );
    ws.action("hızlı", "sleep 1; echo hızlı");
    ws.action(
        "sonra",
        r#"case "$output" in *geldi*) echo bitti; exit 0 ;; esac
echo '{"sipariş": "ş-1"}'; exit 75"#,
    );
    ws.emit("yarış.başladı", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"]);
    assert_eq!(ws.read("bekle.log"), "uyandı\n");
    ws.emit("veri.geldi", json!({"sipariş": "ş-1", "geldi": "şimdi"}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.read("bekle.log"), "uyandı\n", "the loser ran again");
}

/// A pass over `ws`, on a thread of its own.
fn pass_apart(ws: &Ws) -> std::thread::JoinHandle<Pass> {
    let root = ws.d.path().to_path_buf();
    std::thread::spawn(move || {
        let ws = itero::workspace::find(&root).unwrap();
        run_once(&ws, NOW.parse().unwrap(), exe()).unwrap()
    })
}

/// Ends process `pid`, if it still runs: what a failing test leaves.
fn end(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output();
}

#[test]
fn a_waiting_step_wins_a_race_when_its_data_comes_while_the_other_runs() {
    let ws = Ws::new();
    ws.flow(
        "onay",
        json!({"on": [{ "event": "iş.başladı" }],
               "steps": [{ "race": ["#onay", "#derle"] }]}),
    );
    ws.action(
        "onay",
        r#"case "$output" in *onaylandı*) echo onaylandı; exit 0 ;; esac
echo '{"iş": "ş-1"}'; exit 75"#,
    );
    ws.action("derle", "sleep 20 &\necho $! > derle.pid\nwait");
    ws.emit("iş.başladı", json!({}));
    let started = std::time::Instant::now();
    let pass = pass_apart(&ws);
    let pid = pid_in(&ws, "derle.pid");
    assert!(soon(|| ws.statuses() == vec!["waiting"]));
    // the approval comes while the build still runs
    ws.emit("onay.geldi", json!({"iş": "ş-1", "onaylandı": true}));
    pass.join().unwrap();
    let took = started.elapsed();
    let gone = soon(|| !alive(pid));
    end(pid);
    assert!(took < secs(15), "the pass waited for the build: {took:?}");
    assert!(gone, "sleep {pid} still ran");
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(
        ws.runs()[0].runtime.state["output"],
        json!({"#onay": "onaylandı", "winner": "#onay"})
    );
    saved_as_stopped(&ws, "derle");
}

#[test]
fn a_list_whose_step_breaks_lets_the_others_run_to_their_end_then_breaks() {
    for group in [
        json!(["#kırık", "#yavaş"]),
        json!({ "all": ["#kırık", "#yavaş"] }),
    ] {
        let ws = Ws::new();
        ws.flow(
            "liste",
            json!({"on": [{ "event": "liste.başladı" }],
                   "steps": [group, "#sonra"], "error": ["#hata"]}),
        );
        ws.action("kırık", "echo bozuldu >&2; exit 1");
        ws.action("yavaş", "sleep 1; echo bitti > yavaş.txt");
        ws.action("sonra", "echo koştu > sonra.txt");
        // the error list runs once the slow step has ended
        ws.action("hata", r#"echo "$failed_step $(cat yavaş.txt)" > hata.txt"#);
        ws.emit("liste.başladı", json!({}));
        let pass = ws.pass(NOW);
        assert_eq!(ws.statuses(), vec!["error"], "{group}");
        assert_eq!(ws.read("yavaş.txt"), "bitti\n", "{group}");
        assert_eq!(ws.read("hata.txt"), "#kırık bitti\n", "{group}");
        assert_eq!(ws.read("sonra.txt"), "", "{group}");
        assert_eq!(ws.runs()[0].runtime.state["why"], "exit 1: bozuldu");
        assert_eq!(pass.stopped, 0, "{group}");
        assert!(jobs(&ws).iter().all(|j| j.2.is_none()), "{:?}", jobs(&ws));
    }
}

#[test]
fn a_pass_whose_manager_keeps_being_killed_still_stops_the_steps_that_lose() {
    let ws = Ws::new();
    ws.flow(
        "yarış",
        json!({"on": [{ "event": "yarış.başladı" }],
               "steps": [{ "race": ["#hızlı", "#yavaş"] }]}),
    );
    ws.action("hızlı", "sleep 2; echo hızlı");
    ws.action("yavaş", "sleep 30 &\necho $! > uyku.pid\nwait");
    ws.emit("yarış.başladı", json!({}));
    // every manager dies while this file is there
    let die = ws.d.path().join("ölsün");
    let mut daemon = std::process::Command::new(exe())
        .args(["daemon", "--once", "--now", NOW])
        .current_dir(ws.d.path())
        .env("ITERO_TEST_DIE_AT", "manager")
        .env("ITERO_TEST_DIE_WHILE", &die)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = pid_in(&ws, "uyku.pid");
    std::fs::write(&die, "").unwrap();
    // the fast step's result waits: its manager keeps being killed
    let deferred = soon(|| {
        ws.log()
            .iter()
            .any(|l| l["why"].as_str().is_some_and(|w| w.contains("given again")))
    });
    std::fs::remove_file(&die).unwrap();
    let ended = soon(|| daemon.try_wait().unwrap().is_some());
    let gone = soon(|| !alive(pid));
    if !ended {
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
    end(pid);
    assert!(deferred, "no manager was killed: {:?}", ws.log());
    assert!(ended, "the pass waited for the slow step");
    assert!(gone, "sleep {pid} still ran");
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.runs()[0].runtime.state["output"]["winner"], "#hızlı");
    saved_as_stopped(&ws, "yavaş");
}

#[test]
fn what_a_pass_saves_has_the_time_it_was_saved() {
    let ws = Ws::new();
    ws.flow(
        "uyku",
        json!({"on": [{ "event": "uyku.başladı" }], "steps": ["#uyku"]}),
    );
    ws.action("uyku", "sleep 2; echo uyandı");
    ws.emit("uyku.başladı", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    let run = &ws.runs()[0];
    let j = Journal::open(&ws.ws).unwrap();
    let events = j
        .events(Some(&itero::engine::journal::subject(&run.id)))
        .unwrap();
    let done = events
        .iter()
        .find(|e| e.event.kind == "action.finished")
        .unwrap();
    // two seconds after the pass began, not when it began
    let later = "2026-10-21T10:00:02Z";
    assert!(done.event.time.as_str() >= later, "{:?}", done.event);
    assert!(run.ended.as_deref().unwrap() >= later, "{:?}", run.ended);
}

#[test]
fn a_step_quiet_past_its_limit_is_stopped_and_run_again() {
    let ws = Ws::with(r#"{"limits": {"quiet": "1s"}}"#);
    ws.flow(
        "sessiz",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#sessiz"]}),
    );
    ws.action("sessiz", "echo başladı >> denemeler.txt; sleep 30");
    ws.emit("order.placed", json!({"order": 1}));
    let started = std::time::Instant::now();
    ws.pass(NOW);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(ws.statuses(), vec!["error"]);
    // under load, a try may be stopped before the action's first line
    assert!(ws.read("denemeler.txt").contains("başladı"));
    assert_eq!(
        ws.runs()[0].runtime.state["why"],
        "lost 3 times: quiet for 1s"
    );
}

#[test]
fn an_actions_own_quiet_overrides_the_workspaces() {
    let ws = Ws::with(
        r#"{"limits": {"quiet": "1s"}, "action": {"düşünen": {"limits": {"quiet": "10s"}}}}"#,
    );
    ws.flow(
        "yavaş",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#düşünen", "#sessiz"]}),
    );
    ws.action("düşünen", "sleep 2; echo düşündü");
    ws.action("sessiz", "sleep 30");
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["error"]);
    let run = &ws.runs()[0];
    assert_eq!(run.runtime.state["why"], "lost 3 times: quiet for 1s");
    assert_eq!(run.runtime.state["failed_step"], "#sessiz");
}

#[test]
fn a_step_that_prints_as_it_goes_runs_past_its_quiet_limit() {
    let ws = Ws::with(r#"{"limits": {"quiet": "1s"}}"#);
    ws.flow(
        "uzun",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#uzun"]}),
    );
    ws.action(
        "uzun",
        "for i in 1 2 3 4 5 6 7 8 9 10; do echo \"adım $i\" >&2; sleep 0.25; done\necho bitti",
    );
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.runs()[0].runtime.state["output"], "bitti");
}

#[test]
fn a_running_step_shows_its_last_line_on_stderr() {
    let ws = Ws::new();
    ws.flow(
        "uzun",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#uzun"]}),
    );
    ws.action(
        "uzun",
        "echo '[1/2] ilk' >&2; sleep 1; echo '[2/2] ikinci' >&2; sleep 3; echo bitti",
    );
    ws.emit("order.placed", json!({"order": 1}));
    let root = ws.d.path().to_path_buf();
    let pass = std::thread::spawn(move || {
        let ws = itero::workspace::find(&root).unwrap();
        run_once(&ws, NOW.parse().unwrap(), exe()).unwrap();
    });
    let runs = || {
        let o = std::process::Command::new(exe())
            .args(["wf", "runs", "--json"])
            .current_dir(ws.d.path())
            .output()
            .unwrap();
        serde_json::from_slice::<Value>(&o.stdout).unwrap_or(Value::Null)
    };
    let started = std::time::Instant::now();
    let mut seen = Value::Null;
    while started.elapsed() < std::time::Duration::from_secs(4) {
        seen = runs()[0]["progress"].clone();
        if seen["said"] == "[2/2] ikinci" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(seen["step"], "#uzun", "{seen}");
    assert_eq!(seen["said"], "[2/2] ikinci", "{seen}");
    pass.join().unwrap();
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(
        runs()[0]["progress"],
        Value::Null,
        "a step that ended shows none"
    );
}

#[test]
fn a_step_past_its_most_is_stopped_even_while_it_prints() {
    let ws = Ws::with(r#"{"limits": {"quiet": "1s", "most": "1s"}}"#);
    ws.flow(
        "döngü",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#döngü"]}),
    );
    ws.action(
        "döngü",
        "while true; do echo 'hâlâ çalışıyor' >&2; sleep 0.1; done",
    );
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["error"]);
    assert_eq!(
        ws.runs()[0].runtime.state["why"],
        "lost 3 times: ran for 1s"
    );
}

/// An app's endpoint on 127.0.0.1: answers each request with the next of
/// `statuses` (the last one again after them), and hands over each body.
fn endpoint(statuses: Vec<u16>) -> (String, std::sync::mpsc::Receiver<Value>) {
    use std::io::{BufRead, BufReader, Read};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/itero", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let status = *statuses.get(i).or(statuses.last()).unwrap();
            write!(stream, "HTTP/1.1 {status} X\r\ncontent-length: 0\r\n\r\n").unwrap();
            let _ = tx.send(serde_json::from_slice(&body).unwrap());
        }
    });
    (url, rx)
}

#[test]
fn a_workflow_posts_its_result_from_its_success_and_error_steps() {
    let w = Ws::new();
    w.action("selamla", "echo \"{\\\"selam\\\": \\\"merhaba $kim\\\"}\"");
    w.action("kır", "echo 'çöktü' >&2; exit 3");
    // what the workflow guide shows: the app's URL comes in the event's data
    w.action(
        "gönder",
        r#"curl -fsS --retry 5 -H 'content-type: application/json' -d "{\"selam\": \"$selam\", \"why\": \"$why\"}" "$reply_url""#,
    );
    w.flow(
        "iyi",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"],
                "success": [{ "#gönder": { "args": "$output" } }] }),
    );
    w.flow(
        "kötü",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#kır"], "error": ["#gönder"] }),
    );
    let (url, bodies) = endpoint(vec![200]);
    w.emit("kişi.geldi", json!({ "kim": "ayşe", "reply_url": url }));
    w.pass(NOW);
    let mut got: Vec<Value> = bodies.try_iter().collect();
    got.sort_by_key(|b| b["selam"].to_string());
    assert_eq!(got.len(), 2, "{got:?} {:#?}", w.log());
    assert!(
        got[0]["why"].as_str().unwrap().contains("exit 3"),
        "{got:?}"
    );
    assert_eq!(got[1]["selam"], "merhaba ayşe");
}

#[test]
fn a_missing_program_is_named_in_the_runs_why() {
    let w = Ws::new();
    w.action("komut", "yok-böyle-komut --bir şey");
    w.write("actions/dil", "#!/usr/bin/env yok-böyle-dil\n");
    w.write("actions/yol", "#!/opt/yok-böyle/dil\n");
    for a in ["dil", "yol"] {
        use std::os::unix::fs::PermissionsExt;
        let f = w.d.path().join("actions").join(a);
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for (name, action) in [("bir", "komut"), ("iki", "dil"), ("üç", "yol")] {
        w.flow(
            name,
            json!({ "on": [{ "event": "dene.bunu" }], "steps": [format!("#{action}")] }),
        );
    }
    w.emit("dene.bunu", json!({}));
    w.pass(NOW);
    let why = |workflow: &str| {
        let run = w
            .runs()
            .into_iter()
            .find(|r| r.workflow == workflow)
            .unwrap();
        assert_eq!(steps::status(&run.runtime), "error");
        run.runtime.state["why"].as_str().unwrap().to_string()
    };
    assert!(
        why("bir").starts_with("needs yok-böyle-komut: not found on PATH"),
        "{}",
        why("bir")
    );
    assert!(
        why("iki").starts_with("needs yok-böyle-dil: not found on PATH"),
        "{}",
        why("iki")
    );
    assert!(
        why("üç").contains("needs /opt/yok-böyle/dil"),
        "{}",
        why("üç")
    );
}

#[test]
fn a_value_too_big_for_the_environment_is_on_stdin_only() {
    let w = Ws::new();
    // stdin kept as it came: no jq, which not every system has
    w.action(
        "ölç",
        "echo \"${sayfa:-yok}\" > ortam.txt; cat > girdi.json",
    );
    w.flow(
        "büyük",
        json!({ "on": [{ "event": "sayfa.geldi" }], "steps": ["#ölç"] }),
    );
    // 2 MB: past every system's limit for one environment variable
    w.emit("sayfa.geldi", json!({ "sayfa": "ş".repeat(1_000_000) }));
    w.pass(NOW);
    let state = &w.runs()[0].runtime.state;
    assert_eq!(w.statuses(), ["done"], "{:?}", state.get("why"));
    assert_eq!(w.read("ortam.txt"), "yok\n", "not in the environment");
    let stdin: Value = serde_json::from_str(&w.read("girdi.json")).unwrap();
    assert_eq!(
        stdin["sayfa"].as_str().unwrap().chars().count(),
        1_000_000,
        "whole on stdin"
    );
}

#[test]
fn a_big_value_only_a_step_gives_is_an_error_naming_it() {
    let w = Ws::new();
    w.action("ölç", "true");
    w.flow(
        "büyük",
        json!({ "on": [{ "event": "sayfa.geldi" }],
                "steps": [{ "#ölç": { "args": { "sayfa": "ş".repeat(1_000_000) }, "strict": true } }] }),
    );
    w.emit("sayfa.geldi", json!({}));
    w.pass(NOW);
    assert_eq!(w.statuses(), ["error"]);
    let why = w.runs()[0].runtime.state["why"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(why.contains("sayfa") && why.contains("too big"), "{why}");
}

#[test]
fn a_list_of_args_runs_the_rest_once_per_item() {
    let ws = Ws::new();
    ws.flow(
        "posta",
        json!({"on": [{ "event": "posta.bak" }], "steps": ["#kutu", {"#yaz": {"args": "$output"}}]}),
    );
    ws.args(
        "kutu",
        json!([{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]),
    );
    ws.action("kutu", r#"echo "{\"kutu\": \"$user\"}""#);
    ws.action("yaz", r#"echo "$kutu" >> kutular.log"#);
    ws.emit("posta.bak", json!({}));
    ws.pass(NOW);
    let mut got: Vec<String> = ws.read("kutular.log").lines().map(str::to_string).collect();
    got.sort();
    assert_eq!(got, vec!["ayşe@örnek.test", "can@örnek.test"]);
    let mut st = ws.statuses();
    st.sort();
    assert_eq!(st, vec!["done", "done", "split"]);
}

#[test]
fn a_schedule_starts_a_run_once_per_slot() {
    let ws = Ws::new();
    ws.flow(
        "saatlik",
        json!({"on": [{ "schedule": "0 * * * *" }], "steps": ["#say"]}),
    );
    ws.action("say", "echo x >> sayaç.log");
    ws.pass("2026-10-21T14:01:00Z"); // seen: counting starts
    ws.pass("2026-10-21T15:00:10Z");
    ws.pass("2026-10-21T15:30:00Z");
    ws.pass("2026-10-21T16:00:10Z");
    assert_eq!(ws.read("sayaç.log"), "x\nx\n");
}

#[test]
fn a_run_starts_with_its_workflows_settings_then_the_message() {
    let ws = Ws::new();
    ws.flow(
        "gözcü",
        json!({"on": [{ "event": "saat.geldi" }, { "$bölge": "avrupa" }],
               "steps": [{ "#bak": { "args": { "liste": "$siteler" } } }]}),
    );
    ws.action("bak", r#"echo "$liste $en_az" >> görülen.txt"#);
    ws.write(
        "itero.config.json",
        r#"{"workflow": {"gözcü": {"settings": {"siteler": ["https://örnek.test"], "en_az": 10, "bölge": "avrupa"}}}}"#,
    );
    ws.emit("saat.geldi", json!({"en_az": 5}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    let state = &ws.runs()[0].runtime.state;
    assert_eq!(state["siteler"], json!(["https://örnek.test"]));
    assert_eq!(state["en_az"], 5, "the message wins over a setting");
    // a step names a setting as "$siteler"; an action gets one as $en_az
    assert_eq!(ws.read("görülen.txt"), "[\"https://örnek.test\"] 5\n");

    // a setting the on condition needs, and it does not hold
    ws.write(
        "itero.config.json",
        r#"{"workflow": {"gözcü": {"settings": {"bölge": "asya"}}}}"#,
    );
    ws.emit("saat.geldi", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.runs().len(), 1);
}

#[test]
fn a_scheduled_run_gets_its_settings_and_its_entry_can_turn_it_off() {
    let ws = Ws::new();
    ws.flow(
        "saatlik",
        json!({"on": [{ "schedule": "0 * * * *" }], "steps": ["#say"]}),
    );
    ws.action("say", r#"echo "$ad" >> sayaç.log"#);
    ws.write(
        "itero.config.json",
        r#"{"workflow": {"saatlik": {"settings": {"ad": "şükrü"}}}}"#,
    );
    ws.pass("2026-10-21T14:01:00Z");
    ws.pass("2026-10-21T15:00:10Z");
    assert_eq!(ws.read("sayaç.log"), "şükrü\n");
    ws.write(
        "itero.config.json",
        r#"{"workflow": {"saatlik": {"settings": {"ad": "şükrü"}, "off": true}}}"#,
    );
    ws.pass("2026-10-21T16:00:10Z");
    assert_eq!(ws.read("sayaç.log"), "şükrü\n", "off in its entry");
}

#[test]
fn an_action_that_reads_nothing_runs_once() {
    let ws = Ws::new();
    ws.flow(
        "say",
        json!({"on": [{ "event": "say.bir" }], "steps": ["#say"]}),
    );
    // it never reads its stdin, and the state is more than a pipe holds:
    // the worker's write to it fails, which is no reason to stop
    ws.action("say", "echo x >> sayı.log");
    ws.emit("say.bir", json!({"büyük": "ç".repeat(200_000)}));
    ws.pass(NOW);
    assert_eq!(ws.read("sayı.log"), "x\n");
    assert_eq!(ws.statuses(), vec!["done"]);
}

#[test]
fn a_message_nothing_wants_is_noted() {
    let ws = Ws::new();
    ws.emit("kimse.istemez", json!({}));
    ws.pass(NOW);
    let j = Journal::open(&ws.ws).unwrap();
    let e = &j.events(Some("itero.app")).unwrap()[0];
    assert!(e.handled.is_some());
    assert!(e.note.as_deref().unwrap().contains("no workflow"), "{e:?}");
}

/// The daemon killed at each point between a step and its answer loses no
/// step and repeats none, once its effect is not yet done.
#[test]
fn a_daemon_killed_anywhere_carries_on_with_nothing_lost_or_repeated() {
    for point in ["manager", "answer", "job-started", "worker", "result"] {
        let ws = Ws::new();
        ws.flow(
            "say",
            json!({"on": [{ "event": "say.bir" }], "steps": ["#bir", "#iki"]}),
        );
        ws.action("bir", "echo bir >> sayı.log");
        ws.action("iki", "echo iki >> sayı.log");
        ws.emit("say.bir", json!({}));
        let once = ws.d.path().join("öldü");
        let killed = std::process::Command::new(exe())
            .args(["daemon", "--once", "--now", NOW])
            .current_dir(ws.d.path())
            .env("ITERO_TEST_DIE_AT", point)
            .env("ITERO_TEST_DIE_ONCE", &once)
            .output()
            .unwrap();
        assert!(once.exists(), "{point}: nothing died: {killed:?}");
        let again = std::process::Command::new(exe())
            .args(["daemon", "--once", "--now", NOW])
            .current_dir(ws.d.path())
            .output()
            .unwrap();
        assert!(again.status.success(), "{point}: {again:?}");
        assert_eq!(ws.read("sayı.log"), "bir\niki\n", "{point}");
        assert_eq!(ws.statuses(), vec!["done"], "{point}");
    }
}

#[test]
fn a_workflow_off_or_outside_its_dates_starts_nothing() {
    let w = Ws::with(
        r#"{"workflow": {
             "kapalı": { "off": true },
             "erken": { "from": "2026-10-22" },
             "geç": { "until": "2026-10-20" },
             "açık": { "from": "2026-10-21", "until": "2026-10-21" } }}"#,
    );
    w.action("yaz", "echo \"$kim\" >> yazılan.txt");
    for name in ["kapalı", "erken", "geç", "açık"] {
        w.flow(
            name,
            json!({ "on": [{ "event": "selam.ver" }], "steps": ["#yaz"] }),
        );
    }
    w.emit("selam.ver", json!({ "kim": "ayşe" }));
    w.pass(NOW);
    let names: Vec<String> = w.runs().into_iter().map(|r| r.workflow).collect();
    assert_eq!(names, ["açık"]);
    assert_eq!(w.read("yazılan.txt"), "ayşe\n");
}

#[test]
fn past_the_runs_limit_messages_wait_their_turn() {
    let w = Ws::with(r#"{"limits": {"runs": 1}}"#);
    w.action(
        "bekle",
        r#"case "$output" in *'"tamam":true'*) exit 0 ;; esac
echo "{\"numara\": $numara}"; exit 75"#,
    );
    w.flow(
        "sıra",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#bekle"] }),
    );
    w.emit("iş.geldi", json!({ "numara": 1 }));
    w.emit("iş.geldi", json!({ "numara": 2 }));
    w.pass(NOW);
    assert_eq!(w.statuses(), ["waiting"], "the second waits for room");
    w.emit("iş.bitti", json!({ "numara": 1, "tamam": true }));
    w.pass(NOW);
    assert_eq!(w.statuses(), ["done", "waiting"]);
}

#[test]
fn a_run_in_error_sends_itero_run_error_never_to_its_own_workflow() {
    let w = Ws::new();
    w.action("kır", "echo 'çöktü' >&2; exit 3");
    w.action("uyar", "echo \"$workflow: $why\" >> uyarılar.txt; exit 3");
    w.flow(
        "bozuk",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#kır"] }),
    );
    w.flow(
        "uyarı",
        json!({ "on": [{ "event": "itero.run.error" }], "steps": ["#uyar"] }),
    );
    w.emit("iş.geldi", json!({}));
    w.pass(NOW);
    w.pass(NOW);
    let said = w.read("uyarılar.txt");
    assert_eq!(said.lines().count(), 1, "{said}");
    assert!(said.starts_with("bozuk: exit 3: çöktü"), "{said}");
    let error = w
        .log()
        .into_iter()
        .find(|l| l["event"] == "message" && l["type"] == "itero.run.error")
        .unwrap();
    assert_eq!(error["data"]["workflow"], "bozuk");
    assert_eq!(error["data"]["failed_step"], "#kır");
    let errors = w
        .messages()
        .iter()
        .filter(|t| *t == "itero.run.error")
        .count();
    assert_eq!(
        errors, 2,
        "one for bozuk, one for uyarı, which starts nothing"
    );
}

#[test]
fn a_run_done_sends_itero_run_done_with_its_last_result() {
    let w = Ws::new();
    w.action("söyle", "echo 'merhaba dünya'");
    w.action("yaz", "echo \"$workflow: $output\" >> bitenler.txt");
    w.flow(
        "selam",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#söyle"] }),
    );
    w.flow(
        "izle",
        json!({ "on": [{ "event": "itero.run.done" }], "steps": ["#yaz"] }),
    );
    w.emit("iş.geldi", json!({}));
    w.pass(NOW);
    w.pass(NOW);
    assert_eq!(w.read("bitenler.txt"), "selam: merhaba dünya\n");
    let done = w
        .log()
        .into_iter()
        .find(|l| l["event"] == "message" && l["type"] == "itero.run.done")
        .unwrap();
    assert_eq!(done["data"]["workflow"], "selam");
    assert_eq!(done["data"]["output"], "merhaba dünya");
}

#[test]
fn a_workflow_can_start_when_one_named_workflow_is_done() {
    let w = Ws::new();
    w.action("söyle", "echo 'tamam'");
    w.action("yaz", "echo \"$workflow\" >> bitenler.txt");
    for name in ["yedek", "başka"] {
        w.flow(
            name,
            json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#söyle"] }),
        );
    }
    w.flow(
        "rapor",
        json!({ "on": [{ "event": "itero.run.done" }, { "$workflow": "yedek" }], "steps": ["#yaz"] }),
    );
    w.emit("iş.geldi", json!({}));
    w.pass(NOW);
    w.pass(NOW);
    assert_eq!(w.read("bitenler.txt"), "yedek\n");
}

#[test]
fn a_run_started_by_a_run_message_sends_no_itero_run_done() {
    // two workflows that watch every run's end would start each other forever
    let w = Ws::new();
    w.action("söyle", "echo 'tamam'");
    w.action("yaz", "echo \"$workflow\" >> bitenler.txt");
    w.flow(
        "selam",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#söyle"] }),
    );
    for name in ["izle", "gözle"] {
        w.flow(
            name,
            json!({ "on": [{ "event": "itero.run.done" }], "steps": ["#yaz"] }),
        );
    }
    w.emit("iş.geldi", json!({}));
    for _ in 0..4 {
        w.pass(NOW);
    }
    assert_eq!(w.read("bitenler.txt"), "selam\nselam\n");
    let done = w
        .messages()
        .iter()
        .filter(|t| *t == "itero.run.done")
        .count();
    assert_eq!(done, 1, "only selam's run says it is done");
}

#[test]
fn itero_messages_cannot_be_sent_from_outside() {
    let w = Ws::new();
    let e = itero::engine::events::emit(&w.ws, "itero.run.error", Some(json!({})), NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("itero.run.error"), "{e}");
}

#[test]
fn internal_runs_are_outside_the_runs_limit_and_say_so_in_the_log() {
    let w = Ws::with(
        r#"{"limits": {"runs": 1, "workflows": 1}, "workflow": {"bakım": {"internal": true}}}"#,
    );
    w.action("bekle", "exit 75");
    w.action("bak", "true");
    w.flow(
        "sıra",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#bekle"] }),
    );
    w.flow(
        "bakım",
        json!({ "on": [{ "event": "bakım.zamanı" }], "steps": ["#bak"] }),
    );
    w.emit("iş.geldi", json!({}));
    w.emit("bakım.zamanı", json!({}));
    w.pass(NOW);
    assert_eq!(w.statuses(), ["waiting", "done"]);
    let mine: Vec<Value> = w
        .log()
        .into_iter()
        .filter(|l| l["workflow"] == "bakım")
        .collect();
    assert!(!mine.is_empty());
    assert!(mine.iter().all(|l| l["internal"] == true), "{mine:#?}");
}

#[test]
fn keep_forgets_ended_runs_every_hour() {
    let w = Ws::new();
    w.action("yaz", "true");
    w.flow(
        "eski",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#yaz"] }),
    );
    w.emit("iş.geldi", json!({}));
    w.pass("2026-10-10T09:30:00Z");
    assert_eq!(w.runs().len(), 1);
    // a day later, at the hour: keep runs and forgets what ended a day ago
    w.pass("2026-10-11T10:00:00Z");
    let names: Vec<String> = w.runs().into_iter().map(|r| r.workflow).collect();
    assert_eq!(names, ["keep"], "the old run forgotten; keep's own stays");
}

#[test]
fn past_its_space_the_daemon_says_so_and_starts_no_run() {
    let w = Ws::with(r#"{"limits": {"space": "1KB"}}"#);
    w.action("yaz", "true");
    w.flow(
        "iş",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#yaz"] }),
    );
    w.pass(NOW);
    w.emit("iş.geldi", json!({}));
    w.pass(NOW);
    let full: Vec<Value> = w
        .log()
        .into_iter()
        .filter(|l| l["type"] == "itero.space.full")
        .collect();
    assert_eq!(full.len(), 1, "sent once");
    assert_eq!(full[0]["data"]["limit"], 1024);
    assert!(full[0]["data"]["used"].as_u64().unwrap() > 1024);
    assert!(w.log().iter().any(|l| l["event"] == "error"));
    assert!(
        w.runs().iter().all(|r| r.workflow == "keep"),
        "no run of iş"
    );
}

#[test]
fn past_its_size_the_log_is_rolled() {
    let w = Ws::with(r#"{"limits": {"log": "1KB", "logs": 1}}"#);
    w.action("yaz", "true");
    w.flow(
        "iş",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#yaz"] }),
    );
    for _ in 0..3 {
        w.emit(
            "iş.geldi",
            json!({ "söz": "çok uzun bir satır ".repeat(10) }),
        );
        w.pass(NOW);
    }
    w.pass(NOW);
    assert!(w.read(".itero/itero.log.1").contains("itero.log.full"));
    assert!(!w.d.path().join(".itero/itero.log.2").exists(), "logs = 1");
    assert!(w.read(".itero/itero.log").len() < 1024 * 4);
}

#[test]
fn a_built_in_workflow_is_turned_off_by_its_entry() {
    let w = Ws::with(r#"{"limits": {"log": "1KB"}, "workflow": {"logs": {"off": true}}}"#);
    for _ in 0..3 {
        w.emit(
            "iş.geldi",
            json!({ "söz": "çok uzun bir satır ".repeat(10) }),
        );
        w.pass(NOW);
    }
    assert!(!w.d.path().join(".itero/itero.log.1").exists());
}

/// A daemon process, killed however the test ends.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits up to 20 seconds for `ok`.
fn soon(mut ok: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// The threads of process `pid`.
fn threads(pid: u32) -> usize {
    #[cfg(target_os = "linux")]
    return std::fs::read_dir(format!("/proc/{pid}/task"))
        .unwrap()
        .count();
    #[cfg(target_os = "macos")]
    {
        let o = std::process::Command::new("ps")
            .args(["-M", "-p", &pid.to_string()])
            .output()
            .unwrap();
        // a header line, then one line per thread
        String::from_utf8_lossy(&o.stdout).lines().count() - 1
    }
}

#[test]
fn a_path_is_watched_by_the_daemon_one_run_per_burst() {
    let w = Ws::new();
    let root = w.d.path().to_path_buf();
    let inbox = root.join("gelen kutusu");
    w.action("say", "echo \"$path $kind\" >> görülen.txt");
    let on = |paths: Vec<String>| {
        let on: Vec<Value> = paths.into_iter().map(|p| json!({ "path": p })).collect();
        json!({ "on": on, "steps": ["#say"] })
    };
    w.flow("posta", on(vec![inbox.display().to_string()]));
    let daemon = Running(
        std::process::Command::new(exe())
            .args(["daemon", "--no-serve"])
            .current_dir(&root)
            .env("ITERO_TEST_YER", "ikinci yer")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let watching = |p: &std::path::Path| {
        let p = p.display().to_string();
        soon(|| {
            w.log()
                .iter()
                .any(|l| l["event"] == "paths.watched" && l["paths"].to_string().contains(&p))
        })
    };
    assert!(watching(&inbox), "{:#?}", w.log());
    for i in 0..5 {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inbox)
            .unwrap()
            .write_all(format!("From ayşe {i}\n").as_bytes())
            .unwrap();
    }
    assert!(soon(|| !w.read("görülen.txt").is_empty()));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let seen = w.read("görülen.txt");
    assert_eq!(seen.lines().count(), 1, "one run for the burst: {seen}");
    assert!(seen.starts_with(&inbox.display().to_string()), "{seen}");

    // the workflow moves to another path, with $$ from the environment
    let second = root.join("ikinci yer");
    w.flow(
        "posta",
        on(vec![format!("{}/$$ITERO_TEST_YER", root.display())]),
    );
    assert!(watching(&second), "{:#?}", w.log());
    std::fs::write(&inbox, "artık izlenmiyor").unwrap();
    std::fs::write(&second, "ğ").unwrap();
    assert!(soon(|| w.read("görülen.txt").lines().count() == 2));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let seen = w.read("görülen.txt");
    assert_eq!(seen.lines().count(), 2, "{seen}");
    assert!(
        seen.lines()
            .nth(1)
            .unwrap()
            .starts_with(&second.display().to_string())
    );

    // ten paths: still one watching thread
    let before = threads(daemon.0.id());
    let many: Vec<String> = (0..10)
        .map(|i| root.join(format!("dosya-{i}")).display().to_string())
        .collect();
    w.flow("posta", on(many));
    assert!(watching(&root.join("dosya-9")));
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(threads(daemon.0.id()), before);
}

fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

/// The built-in keep runs hourly in local time: off, so that times here are
/// the same in every time zone.
const KEEP_OFF: &str = r#"{"workflow": {"keep": {"off": true}}}"#;

#[test]
fn with_nothing_due_there_is_no_next_time() {
    let ws = Ws::with(KEEP_OFF);
    ws.flow(
        "dinle",
        json!({"on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"]}),
    );
    ws.action("selamla", "true");
    ws.emit("kişi.geldi", json!({}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.next_due(NOW), None);
}

#[test]
fn the_next_time_is_a_schedules_next_slot() {
    let ws = Ws::with(KEEP_OFF);
    // every minute: the same slots in every time zone
    ws.flow(
        "dakikalık",
        json!({"on": [{ "schedule": "* * * * *" }], "steps": ["#say"]}),
    );
    ws.action("say", "true");
    // a schedule not seen yet is due at once: the pass starts its count
    assert_eq!(
        ws.next_due("2026-10-21T14:00:30Z"),
        Some(at("2026-10-21T14:00:30Z"))
    );
    ws.pass("2026-10-21T14:00:30Z");
    assert_eq!(
        ws.next_due("2026-10-21T14:00:30Z"),
        Some(at("2026-10-21T14:01:00Z"))
    );
}

#[test]
fn the_next_time_is_a_waiting_runs_deadline() {
    let ws = Ws::with(KEEP_OFF);
    ws.flow(
        "bekle",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#bekle"], "timeout": "1m"}),
    );
    ws.action("bekle", "exit 75");
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["waiting"]);
    // its deadline, a minute after it began to wait, before its hourly retry
    let since = waiting_since(&ws);
    assert!(since >= at(NOW), "{since}");
    assert_eq!(ws.next_due(NOW), Some(since + chrono::Duration::minutes(1)));
}

/// When the one waiting step of the workspace's one run began to wait.
fn waiting_since(ws: &Ws) -> DateTime<Utc> {
    fn find(v: &Value) -> Option<&str> {
        match v {
            Value::Object(o) => o
                .get("waiting_since")
                .and_then(Value::as_str)
                .or_else(|| o.values().find_map(find)),
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    let rt = serde_json::to_value(&ws.runs()[0].runtime).unwrap();
    at(find(&rt).expect("a step waits"))
}

#[test]
fn the_next_time_is_the_earliest_of_them() {
    let ws = Ws::with(KEEP_OFF);
    ws.flow(
        "dakikalık",
        json!({"on": [{ "schedule": "* * * * *" }], "steps": ["#say"]}),
    );
    ws.action("say", "true");
    ws.flow(
        "bekle",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#bekle"], "timeout": "30s"}),
    );
    ws.action("bekle", "exit 75");
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass("2026-10-21T10:00:10Z");
    // the schedule at 10:01, the deadline 30 seconds after the step began
    // to wait, at 10:00:40 or a moment later
    let since = waiting_since(&ws);
    assert!(since < at("2026-10-21T10:00:30Z"), "{since}");
    assert_eq!(
        ws.next_due(NOW),
        Some(since + chrono::Duration::seconds(30))
    );
}

#[test]
fn an_idle_workspace_is_next_due_at_keeps_hourly_slot() {
    let ws = Ws::new();
    ws.pass(NOW);
    let next = ws.next_due(NOW).expect("keep is hourly");
    assert!(
        next > at(NOW) && next <= at("2026-10-21T11:00:00Z"),
        "{next}"
    );
}

#[test]
fn a_workflow_that_is_off_or_over_has_no_next_time() {
    let ws = Ws::with(
        r#"{"workflow": {"keep": {"off": true}, "kapalı": {"off": true},
            "bitti": {"until": "2026-10-21T09:00:00Z"}}}"#,
    );
    for name in ["kapalı", "bitti"] {
        ws.flow(
            name,
            json!({"on": [{ "schedule": "* * * * *" }], "steps": ["#say"]}),
        );
    }
    ws.action("say", "true");
    ws.pass(NOW);
    assert_eq!(ws.next_due(NOW), None);
}

#[test]
fn a_workflow_from_a_later_date_is_next_due_at_its_first_slot_from_then() {
    let ws = Ws::with(
        r#"{"workflow": {"keep": {"off": true}, "sonra": {"from": "2026-10-21T12:30:30Z"}}}"#,
    );
    ws.flow(
        "sonra",
        json!({"on": [{ "schedule": "* * * * *" }], "steps": ["#say"]}),
    );
    ws.action("say", "echo x >> sayaç.log");
    ws.pass(NOW);
    let next = ws.next_due(NOW).unwrap();
    assert_eq!(next, at("2026-10-21T12:31:00Z"));
    // and at that time it runs, once
    ws.pass("2026-10-21T12:31:00Z");
    assert_eq!(ws.read("sayaç.log"), "x\n");
}

/// The path watcher, running on its own thread as in the daemon; what it
/// sends on the channel wakes the daemon.
fn watching(ws: &Ws) -> std::sync::mpsc::Receiver<()> {
    let (wake, woken) = std::sync::mpsc::channel();
    let w = ws.ws.clone();
    let id = Journal::open(&w).unwrap().workspace().unwrap().id;
    let log = itero::engine::log::Log::new(&w, &id);
    std::thread::spawn(move || itero::engine::daemon::watch_paths(&w, &wake, &log));
    // its first look
    std::thread::sleep(std::time::Duration::from_millis(300));
    woken
}

fn secs(n: u64) -> std::time::Duration {
    std::time::Duration::from_secs(n)
}

fn path_messages(ws: &Ws) -> i64 {
    let j = rusqlite::Connection::open(ws.ws.journal()).unwrap();
    j.query_row(
        "SELECT COUNT(*) FROM events WHERE type = 'itero.path.changed'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn an_edit_to_a_workflow_or_the_manifest_wakes_the_daemon_and_nothing_else_does() {
    let ws = Ws::with(KEEP_OFF);
    ws.flow(
        "dinle",
        json!({"on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"]}),
    );
    let woken = watching(&ws);
    // idle, nothing wakes it
    assert!(woken.recv_timeout(secs(2)).is_err());
    ws.flow(
        "dinle",
        json!({"on": [{ "event": "kişi.gitti" }], "steps": ["#selamla"]}),
    );
    woken.recv_timeout(secs(5)).expect("a workflow edited");
    ws.flow(
        "yeni",
        json!({"on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"]}),
    );
    woken.recv_timeout(secs(5)).expect("a workflow added");
    ws.write(
        "itero.config.json",
        r#"{"workflow": {"keep": {"off": false}}}"#,
    );
    woken.recv_timeout(secs(5)).expect("the manifest edited");
    // an edit is no message
    assert_eq!(path_messages(&ws), 0);
}

#[test]
fn a_watched_path_sends_a_message_and_wakes_the_daemon() {
    let ws = Ws::with(KEEP_OFF);
    ws.write("gelen/rapor.txt", "ilk");
    ws.flow(
        "izle",
        json!({"on": [{ "path": "gelen/rapor.txt" }], "steps": ["#oku"]}),
    );
    let woken = watching(&ws);
    ws.write("gelen/rapor.txt", "ikinci");
    woken.recv_timeout(secs(5)).expect("the path changed");
    assert_eq!(path_messages(&ws), 1);
}

#[test]
fn a_path_whose_folder_comes_later_is_watched_once_it_is_there() {
    let ws = Ws::with(KEEP_OFF);
    ws.flow(
        "izle",
        json!({"on": [{ "path": "sonra/gelecek/rapor.txt" }], "steps": ["#oku"]}),
    );
    let woken = watching(&ws);
    // its folders come, then the file
    std::fs::create_dir_all(ws.d.path().join("sonra/gelecek")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    while woken.try_recv().is_ok() {}
    ws.write("sonra/gelecek/rapor.txt", "geldi");
    woken.recv_timeout(secs(5)).expect("the file came");
    assert!(path_messages(&ws) >= 1);
}
