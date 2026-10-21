//! The manager over steps: one decision per event, JSON in, JSON out.
use itero::engine::Event;
use itero::engine::steps::{self, Ended, Phase, Runtime, StepAnswer, StepInput, StepJob};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const NOW: &str = "2026-10-21T10:00:00Z";

/// A run of `root`, fed one event at a time, keeping its runtime.
struct Run {
    root: String,
    flows: BTreeMap<String, Value>,
    args: BTreeMap<String, Value>,
    defined: Vec<String>,
    rt: Runtime,
    now: String,
    seq: usize,
}

impl Run {
    fn new(flows: &[(&str, Value)]) -> Run {
        Run {
            root: flows[0].0.to_string(),
            flows: flows
                .iter()
                .map(|(n, f)| (n.to_string(), f.clone()))
                .collect(),
            args: BTreeMap::new(),
            defined: vec![],
            rt: Runtime::default(),
            now: NOW.into(),
            seq: 0,
        }
    }

    fn send(&mut self, kind: &str, data: Value) -> StepAnswer {
        self.seq += 1;
        let input = StepInput {
            now: self.now.clone(),
            workflow: self.root.clone(),
            flows: self.flows.clone(),
            args: self.args.clone(),
            defined: self.defined.clone(),
            runtime: self.rt.clone(),
            event: Event {
                id: format!("olay-{}", self.seq),
                kind: kind.into(),
                data,
            },
        };
        let a = steps::decide(&input).unwrap();
        self.rt = a.runtime.clone();
        a
    }

    fn start(&mut self, data: Value) -> StepAnswer {
        self.send("run.started", data)
    }

    fn done(&mut self, job: &StepJob, output: Value) -> StepAnswer {
        self.send(
            "action.finished",
            json!({"path": job.path, "output": output}),
        )
    }

    fn waiting(&mut self, job: &StepJob, output: Value) -> StepAnswer {
        self.send(
            "action.waiting",
            json!({"path": job.path, "output": output}),
        )
    }

    fn failed(&mut self, job: &StepJob, error: &str) -> StepAnswer {
        self.send("action.failed", json!({"path": job.path, "error": error}))
    }

    fn message(&mut self, id: &str, data: Value) -> StepAnswer {
        self.send(
            "message",
            json!({"id": id, "type": "payment.received", "data": data}),
        )
    }
}

fn names(a: &StepAnswer) -> Vec<String> {
    a.jobs.iter().map(|j| j.action.clone()).collect()
}

fn state(r: &Run) -> Value {
    Value::Object(r.rt.state.clone())
}

#[test]
fn a_run_starts_with_the_message_as_its_state() {
    let mut r = Run::new(&[(
        "sipariş",
        json!({"on": ["order.placed"], "steps": ["#ödeme-kontrol", "#kargola"]}),
    )]);
    let a = r.start(json!({"sipariş": 43, "tutar": 120}));
    assert_eq!(names(&a), vec!["ödeme-kontrol"]);
    assert_eq!(state(&r), json!({"sipariş": 43, "tutar": 120}));
    assert_eq!(r.rt.phase, Phase::Steps);
}

#[test]
fn an_output_joins_the_state_and_the_next_step_follows() {
    let mut r = Run::new(&[("a", json!({"steps": ["#bir", "#iki", "#üç"]}))]);
    let a = r.start(json!({"geçici": "x", "kalıcı": 1}));
    let a = r.done(&a.jobs[0], json!({"metin": "çay", "geçici": null}));
    assert_eq!(state(&r), json!({"metin": "çay", "kalıcı": 1}));
    assert_eq!(names(&a), vec!["iki"]);
    // printing nothing adds nothing
    let a = r.done(&a.jobs[0], Value::Null);
    assert_eq!(state(&r), json!({"metin": "çay", "kalıcı": 1}));
    assert_eq!(names(&a), vec!["üç"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert!(a.jobs.is_empty());
    assert_eq!(r.rt.ended, Some(Ended::Done));
}

#[test]
fn side_by_side_steps_run_together_and_the_next_waits_for_all() {
    let mut r = Run::new(&[(
        "sipariş",
        json!({"steps": [["#check-payment", "#check-stock"], "#ship"]}),
    )]);
    let a = r.start(json!({"order": 43}));
    assert_eq!(names(&a), vec!["check-payment", "check-stock"]);
    let (pay, stock) = (a.jobs[0].clone(), a.jobs[1].clone());
    let a = r.done(&stock, json!({"stock": true, "durum": "düşük"}));
    assert!(a.jobs.is_empty(), "the list is not done yet");
    let a = r.done(&pay, json!({"paid": true, "durum": "tamam"}));
    assert_eq!(names(&a), vec!["ship"]);
    assert_eq!(
        state(&r),
        json!({"order": 43, "stock": true, "paid": true,
               "durum": {"#check-payment": "tamam", "#check-stock": "düşük"}})
    );
}

#[test]
fn a_workflow_side_by_side_runs_its_steps_beside_the_action() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [["#check-payment", "stok"], "#ship"]}),
        ),
        ("stok", json!({"steps": ["#depo-bak", "#ayır"]})),
    ]);
    let a = r.start(json!({"order": 43}));
    assert_eq!(names(&a), vec!["check-payment", "depo-bak"]);
    let (pay, depo) = (a.jobs[0].clone(), a.jobs[1].clone());
    let a = r.done(&depo, json!({"raf": "A-3"}));
    assert_eq!(names(&a), vec!["ayır"]);
    let ayir = a.jobs[0].clone();
    let a = r.done(&pay, json!({"paid": true}));
    assert!(a.jobs.is_empty(), "stok is not done yet");
    let a = r.done(&ayir, json!({"stock": true}));
    assert_eq!(names(&a), vec!["ship"]);
}

#[test]
fn a_step_runs_only_when_its_on_holds() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [
                "#check",
                "işle",
                {"#bildir": {"on": ["$$API"]}},
                "#bitir"
            ]}),
        ),
        (
            "işle",
            json!({"on": ["$paid"], "steps": [
                {"#ship":           {"on": {"$paid": true,  "$stock": true}}},
                {"#remind-payment": {"on": {"$paid": false, "$stock": true}}}
            ]}),
        ),
    ]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({"paid": false, "stock": true}));
    assert_eq!(names(&a), vec!["remind-payment"]);
    // $$API is not defined: #bildir is skipped
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["bitir"]);

    // a workflow whose on does not hold is skipped as a whole
    let mut r = Run::new(&[
        ("sipariş", json!({"steps": ["#check", "işle", "#bitir"]})),
        ("işle", json!({"on": ["$paid"], "steps": ["#ship"]})),
    ]);
    r.defined = vec!["API".into()];
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({"stock": true}));
    assert_eq!(names(&a), vec!["bitir"]);
}

#[test]
fn a_step_that_is_not_ready_waits_for_matching_data() {
    let mut r = Run::new(&[(
        "sipariş",
        json!({"steps": [["#check-payment", "#check-stock"], "#ship"]}),
    )]);
    let a = r.start(json!({"order": 43}));
    let (pay, stock) = (a.jobs[0].clone(), a.jobs[1].clone());
    r.done(&stock, json!({"stock": true}));
    let a = r.waiting(&pay, json!({"payment_ref": "ödeme-1"}));
    assert!(a.jobs.is_empty());
    assert_eq!(
        r.rt.state["payment_ref"], "ödeme-1",
        "a waiting step's output joins"
    );
    let timers = a.timers.clone();
    let at: Vec<&str> = timers.iter().map(|t| t.at.as_str()).collect();
    assert_eq!(at, vec!["2026-10-21T11:00:00Z", "2026-10-22T10:00:00Z"]);

    // no field in common, or one that disagrees: not this run's
    assert!(r.message("m1", json!({"tutar": 120})).jobs.is_empty());
    assert!(
        r.message("m2", json!({"payment_ref": "ödeme-2", "paid": true}))
            .jobs
            .is_empty()
    );
    assert!(!r.rt.state.contains_key("paid"));
    // this run's: it joins, and the step runs again
    let a = r.message("m3", json!({"payment_ref": "ödeme-1", "paid": true}));
    assert_eq!(names(&a), vec!["check-payment"]);
    assert_eq!(a.jobs[0].path, pay.path);
    assert_eq!(r.rt.state["paid"], true);
    assert!(a.timers.is_empty(), "no step waits now");
    // the same message again changes nothing
    assert!(
        r.message("m3", json!({"payment_ref": "ödeme-1"}))
            .jobs
            .is_empty()
    );
    let a = r.done(&a.jobs[0], json!({"paid": true}));
    assert_eq!(names(&a), vec!["ship"]);
}

#[test]
fn a_waiting_step_runs_again_every_hour_and_gives_up_after_its_timeout() {
    let mut r = Run::new(&[(
        "sipariş",
        json!({"steps": ["#check-payment"], "timeout": "2h", "error": ["#alert-team"]}),
    )]);
    let a = r.start(json!({"order": 43}));
    let pay = a.jobs[0].clone();
    let a = r.waiting(&pay, json!({}));
    assert_eq!(a.timers[1].at, "2026-10-21T12:00:00Z");
    r.now = "2026-10-21T11:00:00Z".into();
    let a = r.send("timer.fired", json!({"kind": "retry"}));
    assert_eq!(names(&a), vec!["check-payment"]);
    assert_eq!(a.jobs[0].waiting_since.as_deref(), Some(NOW));
    r.waiting(&a.jobs[0], json!({}));
    r.now = "2026-10-21T12:00:00Z".into();
    let a = r.send("timer.fired", json!({"kind": "deadline"}));
    assert_eq!(names(&a), vec!["alert-team"]);
    assert_eq!(r.rt.phase, Phase::Error);
    assert_eq!(r.rt.state["failed_step"], "#check-payment");
    assert!(r.rt.state["why"].as_str().unwrap().contains("2h"));
}

#[test]
fn an_error_stops_the_steps_and_runs_the_error_list() {
    let mut r = Run::new(&[(
        "sipariş",
        json!({"steps": [["#check-payment", "#check-stock"], "#ship"],
               "success": ["#log"], "error": ["#log", "#alert-team"]}),
    )]);
    let a = r.start(json!({"order": 43}));
    let (pay, stock) = (a.jobs[0].clone(), a.jobs[1].clone());
    let a = r.failed(&pay, "exit 3: bağlantı yok");
    assert_eq!(names(&a), vec!["log"]);
    let log = a.jobs[0].clone();
    assert_eq!(r.rt.state["why"], "exit 3: bağlantı yok");
    assert_eq!(r.rt.state["failed_step"], "#check-payment");
    // the other branch's late answer is ignored
    assert!(r.done(&stock, json!({"stock": true})).jobs.is_empty());
    assert!(!r.rt.state.contains_key("stock"));
    let a = r.done(&log, json!({}));
    assert_eq!(names(&a), vec!["alert-team"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert!(a.jobs.is_empty());
    assert_eq!(
        r.rt.ended,
        Some(Ended::Error {
            why: "exit 3: bağlantı yok".into()
        })
    );
}

#[test]
fn success_false_runs_failure_otherwise_success() {
    let flow = json!({"steps": ["#remind-stock"], "success": ["#teşekkür"], "failure": ["#log"]});
    let mut r = Run::new(&[("sipariş", flow.clone())]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({"success": false}));
    assert_eq!(names(&a), vec!["log"]);
    assert_eq!(r.rt.phase, Phase::Failure);
    r.done(&a.jobs[0], json!({}));
    assert_eq!(r.rt.ended, Some(Ended::Failed));

    let mut r = Run::new(&[("sipariş", flow)]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["teşekkür"]);
    // a step of a list that breaks ends the run in error, no other list
    let a = r.failed(&a.jobs[0], "exit 1");
    assert!(a.jobs.is_empty());
    assert_eq!(
        r.rt.ended,
        Some(Ended::Error {
            why: "exit 1".into()
        })
    );
}

#[test]
fn a_list_of_args_runs_the_rest_once_per_item() {
    let mut r = Run::new(&[("posta", json!({"steps": ["#imap-check", "#notify"]}))]);
    r.args.insert(
        "imap-check".into(),
        json!([{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]),
    );
    let a = r.start(json!({"saat": 9}));
    assert!(a.jobs.is_empty());
    assert_eq!(a.split.len(), 2);
    assert_eq!(r.rt.ended, Some(Ended::Split));
    for (child, user) in a.split.iter().zip(["ayşe@örnek.test", "can@örnek.test"]) {
        let mut c = Run::new(&[("posta", json!({"steps": ["#imap-check", "#notify"]}))]);
        c.args = r.args.clone();
        c.rt = child.clone();
        let a = c.start(Value::Null);
        assert_eq!(names(&a), vec!["imap-check"]);
        assert_eq!(a.jobs[0].item, Some(json!({"user": user})));
        assert_eq!(c.rt.state["saat"], 9);
        let a = c.done(&a.jobs[0], json!({"text": "1 yeni"}));
        assert_eq!(names(&a), vec!["notify"]);
        assert_eq!(a.jobs[0].item, None, "the item is imap-check's");
    }
}

#[test]
fn runtimes_go_through_json() {
    let mut r = Run::new(&[
        ("a", json!({"steps": [["#bir", "iki"]]})),
        ("iki", json!({"steps": ["#üç"]})),
    ]);
    r.start(json!({"ö": 1}));
    let text = serde_json::to_string(&r.rt).unwrap();
    let back: Runtime = serde_json::from_str(&text).unwrap();
    assert_eq!(back, r.rt);
    let _: Map<String, Value> = r.rt.state.clone();
}
