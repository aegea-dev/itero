//! The daemon over the simple files, end to end: messages start runs, steps
//! run in workers, waiting runs get their messages, and runs end as done,
//! failed or error.
#![cfg(unix)]
use itero::engine::daemon::{Pass, run_once};
use itero::engine::journal::{Journal, Run};
use itero::engine::steps;
use serde_json::{Value, json};
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
        Ws::with("")
    }

    /// A workspace whose itero.toml says `toml`.
    fn with(toml: &str) -> Ws {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.toml"), toml).unwrap();
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

    fn args(&self, action: &str, v: Value) {
        self.write(&format!("args/{action}.json"), &v.to_string());
    }

    fn emit(&self, kind: &str, data: Value) {
        itero::engine::events::emit(&self.ws, kind, Some(data), NOW).unwrap();
    }

    fn pass(&self, now: &str) -> Pass {
        run_once(&self.ws, now.parse().unwrap(), exe()).unwrap()
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

    /// The log's lines.
    fn log(&self) -> Vec<Value> {
        self.read(".itero/itero.log")
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
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
/// waiting for its provider, then the order's processing and a mail.
fn order(ws: &Ws) {
    ws.flow(
        "order",
        json!({
            "on": [{ "event": "order.placed" }],
            "steps": [["#check-payment", "#check-stock"], "order-process", "#send-mail"],
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
    ws.action(
        "check-payment",
        r#"[ -n "$paid" ] && { echo "{\"paid\": $paid}"; exit 0; }
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
    assert_eq!(run.runtime.state["stock"], true);
    assert_eq!(run.runtime.state["payment_ref"], "ödeme-1");
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
    ws.action("stok-bekle", r#"[ -n "$arrived" ] || exit 75"#);
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
        json!({"on": [{ "event": "renk.sor" }], "steps": [{"#renk": {"renk": "adımın"}}]}),
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
    std::fs::remove_file(ws.d.path().join("args/renk.json")).unwrap();
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
        json!({"on": [{ "event": "selam.ver" }], "steps": [{"#selam": {"kime": "$müşteri"}}]}),
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
fn a_list_of_args_runs_the_rest_once_per_item() {
    let ws = Ws::new();
    ws.flow(
        "posta",
        json!({"on": [{ "event": "posta.bak" }], "steps": ["#kutu", "#yaz"]}),
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
    let w = Ws::new();
    w.action("yaz", "echo \"$kim\" >> yazılan.txt");
    let flow =
        |meta: Value| json!({ "meta": meta, "on": [{ "event": "selam.ver" }], "steps": ["#yaz"] });
    w.flow("kapalı", flow(json!({ "off": true })));
    w.flow("erken", flow(json!({ "from": "2026-10-22" })));
    w.flow("geç", flow(json!({ "until": "2026-10-20" })));
    w.flow(
        "açık",
        flow(json!({ "from": "2026-10-21", "until": "2026-10-21" })),
    );
    w.emit("selam.ver", json!({ "kim": "ayşe" }));
    w.pass(NOW);
    let names: Vec<String> = w.runs().into_iter().map(|r| r.workflow).collect();
    assert_eq!(names, ["açık"]);
    assert_eq!(w.read("yazılan.txt"), "ayşe\n");
}

#[test]
fn past_the_runs_limit_messages_wait_their_turn() {
    let w = Ws::with("[limits]\nruns = 1\n");
    w.action("bekle", "[ -n \"$tamam\" ] || exit 75");
    w.flow(
        "sıra",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#bekle"] }),
    );
    w.emit("iş.geldi", json!({ "iş": 1 }));
    w.emit("iş.geldi", json!({ "iş": 2 }));
    w.pass(NOW);
    assert_eq!(w.statuses(), ["waiting"], "the second waits for room");
    w.emit("iş.bitti", json!({ "iş": 1, "tamam": true }));
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
fn itero_messages_cannot_be_sent_from_outside() {
    let w = Ws::new();
    let e = itero::engine::events::emit(&w.ws, "itero.run.error", Some(json!({})), NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("itero.run.error"), "{e}");
}

#[test]
fn internal_runs_are_outside_the_runs_limit_and_say_so_in_the_log() {
    let w = Ws::with("[limits]\nruns = 1\nworkflows = 1\n");
    w.action("bekle", "exit 75");
    w.action("bak", "true");
    w.flow(
        "sıra",
        json!({ "on": [{ "event": "iş.geldi" }], "steps": ["#bekle"] }),
    );
    w.flow(
        "bakım",
        json!({ "meta": { "internal": true }, "on": [{ "event": "bakım.zamanı" }], "steps": ["#bak"] }),
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
    let w = Ws::with("[limits]\nspace = \"1KB\"\n");
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
    let w = Ws::with("[limits]\nlog = \"1KB\"\nlogs = 1\n");
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
fn a_built_in_workflow_is_replaced_or_turned_off_by_a_file_of_its_name() {
    let w = Ws::with("[limits]\nlog = \"1KB\"\n");
    w.flow(
        "logs",
        json!({ "meta": { "off": true }, "on": [{ "event": "itero.log.full" }], "steps": ["#roll-log"] }),
    );
    for _ in 0..3 {
        w.emit(
            "iş.geldi",
            json!({ "söz": "çok uzun bir satır ".repeat(10) }),
        );
        w.pass(NOW);
    }
    assert!(!w.d.path().join(".itero/itero.log.1").exists());
}
