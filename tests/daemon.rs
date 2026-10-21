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
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("itero.toml"), "").unwrap();
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
}

/// The order of the design: payment and stock side by side, the payment
/// waiting for its provider, then the order's processing and a mail.
fn order(ws: &Ws) {
    ws.flow(
        "order",
        json!({
            "on": ["order.placed"],
            "steps": [["#check-payment", "#check-stock"], "order-process", "#send-mail"],
            "success": ["#log-result"],
            "failure": ["#log-result"],
            "error": ["#log-result"]
        }),
    );
    ws.flow(
        "order-process",
        json!({
            "on": ["$paid", "$stock"],
            "steps": [
                {"#ship": {"on": {"$paid": true, "$stock": true}}},
                {"#remind-stock": {"on": {"$paid": true, "$stock": false}}}
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
    let errors = j.runs_page(None, Some("error"), 10).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].runtime.state["failed_step"], "#check-stock");
}

#[test]
fn a_message_reaches_every_waiting_run_it_matches() {
    let ws = Ws::new();
    ws.flow(
        "bekle",
        json!({"on": ["order.placed"], "steps": ["#stok-bekle", "#bitir"]}),
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
        ws.flow(n, json!({"on": ["çay.demlendi"], "steps": ["#hiç"]}));
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
        json!({"on": ["renk.sor"], "steps": [{"#renk": {"renk": "adımın"}}]}),
    );
    ws.emit("renk.sor", json!({"renk": "durumun"}));
    ws.pass(NOW);
    ws.args("renk", json!({"renk": "args'ın"}));
    ws.flow("adım", json!({"on": ["renk.sor"], "steps": ["#renk"]}));
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
        json!({"on": ["selam.ver"], "steps": [{"#selam": {"kime": "$müşteri"}}]}),
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
        json!({"on": ["order.placed"], "steps": ["#bekle"], "timeout": "1m"}),
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
        json!({"on": ["posta.bak"], "steps": ["#kutu", "#yaz"]}),
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
    ws.flow("saatlik", json!({"on": ["0 * * * *"], "steps": ["#say"]}));
    ws.action("say", "echo x >> sayaç.log");
    ws.pass("2026-10-21T14:01:00Z"); // seen: counting starts
    ws.pass("2026-10-21T15:00:10Z");
    ws.pass("2026-10-21T15:30:00Z");
    ws.pass("2026-10-21T16:00:10Z");
    assert_eq!(ws.read("sayaç.log"), "x\nx\n");
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
        ws.flow("say", json!({"on": ["say.bir"], "steps": ["#bir", "#iki"]}));
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
