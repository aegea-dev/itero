//! The daemon over the simple files, end to end: messages start runs, steps
//! run in workers, waiting runs get their messages, and runs end as done,
//! failed or error.
#![cfg(unix)]
mod common;
use itero::engine::daemon::{Pass, run_once};
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

    /// A message whose runs' ends are posted to `reply`.
    fn emit_replying(&self, kind: &str, data: Value, reply: &str) -> String {
        itero::engine::events::emit_with(&self.ws, kind, Some(data), Some(reply), NOW).unwrap()
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
fn a_step_that_prints_as_it_goes_runs_past_its_quiet_limit() {
    let ws = Ws::with(r#"{"limits": {"quiet": "1s"}}"#);
    ws.flow(
        "uzun",
        json!({"on": [{ "event": "order.placed" }], "steps": ["#uzun"]}),
    );
    ws.action(
        "uzun",
        "for i in 1 2 3 4 5 6 7 8 9 10; do echo \"adım $i\" >&2; sleep 0.25; done\necho '{\"bitti\": \"evet\"}'",
    );
    ws.emit("order.placed", json!({"order": 1}));
    ws.pass(NOW);
    assert_eq!(ws.statuses(), vec!["done"]);
    assert_eq!(ws.runs()[0].runtime.state["bitti"], "evet");
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
fn the_app_is_called_back_when_each_run_ends() {
    let w = Ws::new();
    w.action("selamla", "echo \"{\\\"selam\\\": \\\"merhaba $kim\\\"}\"");
    w.action("kır", "echo 'çöktü' >&2; exit 3");
    w.flow(
        "iyi",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }),
    );
    w.flow(
        "kötü",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#kır"] }),
    );
    let (url, bodies) = endpoint(vec![200]);
    let message = w.emit_replying("kişi.geldi", json!({ "kim": "ayşe" }), &url);
    w.pass(NOW);
    let mut got: Vec<Value> = bodies.try_iter().collect();
    got.sort_by_key(|b| b["workflow"].to_string());
    assert_eq!(got.len(), 2, "{got:?}");
    let (good, bad) = (&got[0], &got[1]);
    assert_eq!(good["workflow"], "iyi");
    assert_eq!(good["status"], "done");
    assert_eq!(good["message"], message.as_str());
    assert_eq!(good["state"]["selam"], "merhaba ayşe");
    assert_eq!(
        good["name"],
        w.runs()
            .iter()
            .find(|r| r.workflow == "iyi")
            .unwrap()
            .name
            .as_str()
    );
    assert_eq!(bad["status"], "error");
    // a message without a reply calls nothing
    w.emit("kişi.geldi", json!({ "kim": "can" }));
    w.pass(NOW);
    assert_eq!(bodies.try_iter().count(), 0);
}

#[test]
fn a_callback_the_app_refuses_is_tried_again_later() {
    let w = Ws::new();
    w.action("selamla", "true");
    w.flow(
        "iyi",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }),
    );
    let (url, bodies) = endpoint(vec![500, 200]);
    w.emit_replying("kişi.geldi", json!({ "kim": "ayşe" }), &url);
    w.pass(NOW);
    assert_eq!(bodies.try_iter().count(), 1, "refused once");
    w.pass(NOW);
    assert_eq!(bodies.try_iter().count(), 0, "not again at once");
    w.pass("2026-10-21T10:05:00Z");
    assert_eq!(bodies.try_iter().count(), 1, "taken later");
    w.pass("2026-10-21T11:00:00Z");
    assert_eq!(bodies.try_iter().count(), 0, "taken once");
}

#[test]
fn a_callback_no_app_takes_is_given_up_after_a_day() {
    let w = Ws::new();
    w.action("selamla", "true");
    w.flow(
        "iyi",
        json!({ "on": [{ "event": "kişi.geldi" }], "steps": ["#selamla"] }),
    );
    // a port no one listens on
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    w.emit_replying("kişi.geldi", json!({}), &format!("http://{closed}/itero"));
    w.pass(NOW);
    w.pass("2026-10-22T10:00:01Z");
    let gave_up = w
        .log()
        .into_iter()
        .any(|l| l["event"] == "error" && l["why"].as_str().unwrap_or("").contains("gave up"));
    assert!(gave_up, "{:#?}", w.log());
    let left: i64 = rusqlite::Connection::open(w.ws.journal())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM replies", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
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
                "steps": [{ "#ölç": { "sayfa": "ş".repeat(1_000_000) } }] }),
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
fn a_run_starts_with_its_workflows_settings_then_the_message() {
    let ws = Ws::new();
    ws.flow(
        "gözcü",
        json!({"on": [{ "event": "saat.geldi" }, { "$bölge": "avrupa" }],
               "steps": [{ "#bak": { "liste": "$siteler" } }]}),
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
