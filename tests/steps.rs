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
        json!({"on": [{ "event": "order.placed" }], "steps": ["#ödeme-kontrol", "#kargola"]}),
    )]);
    let a = r.start(json!({"sipariş": 43, "tutar": 120}));
    assert_eq!(names(&a), vec!["ödeme-kontrol"]);
    assert_eq!(state(&r), json!({"sipariş": 43, "tutar": 120}));
    assert_eq!(r.rt.phase, Phase::Steps);
}

#[test]
fn a_step_s_result_is_output_and_nothing_else_joins_the_state() {
    let mut r = Run::new(&[("a", json!({"steps": ["#bir", "#iki", "#üç"]}))]);
    let a = r.start(json!({"geçici": "x", "kalıcı": 1}));
    let a = r.done(&a.jobs[0], json!({"metin": "çay", "geçici": null}));
    assert_eq!(
        state(&r),
        json!({"geçici": "x", "kalıcı": 1, "output": {"metin": "çay", "geçici": null}})
    );
    assert_eq!(names(&a), vec!["iki"]);
    // the next step's result takes its place, printing nothing too
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(state(&r), json!({"geçici": "x", "kalıcı": 1, "output": ""}));
    assert_eq!(names(&a), vec!["üç"]);
    let a = r.done(&a.jobs[0], json!(524));
    assert_eq!(r.rt.state["output"], 524);
    assert!(a.jobs.is_empty());
    assert_eq!(r.rt.ended, Some(Ended::Done));
}

#[test]
fn a_step_s_args_join_the_state_unless_it_is_strict() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"#say": {"args": {"klasör": "İndirilenler", "parola": "$$POSTA"}}},
            {"#bildir": {"args": {"adet": "$output", "nerede": "$klasör"}, "strict": true}},
            "#son"
        ]}),
    )]);
    let a = r.start(json!({}));
    assert_eq!(
        a.jobs[0].data,
        json!({"klasör": "İndirilenler", "parola": "$$POSTA"})
            .as_object()
            .unwrap()
            .clone()
    );
    // a secret joins as its name, never its value
    assert_eq!(
        state(&r),
        json!({"klasör": "İndirilenler", "parola": "$$POSTA"})
    );
    let a = r.done(&a.jobs[0], json!(524));
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(
        a.jobs[0].data,
        json!({"adet": 524, "nerede": "İndirilenler"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(!r.rt.state.contains_key("adet"), "strict: the step's only");
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(names(&a), vec!["son"]);
}

#[test]
fn args_may_be_a_whole_object_or_one_field_of_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            "#say",
            {"#taşı": {"args": "$output"}},
            {"#bildir": {"args": {"n": "$output.sayı", "yok": "$output.hiç"}}},
            {"#son": {"args": "$output"}}
        ]}),
    )]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!({"sayı": 3, "klasör": "İndirilenler"}));
    assert_eq!(
        state(&r),
        json!({"sayı": 3, "klasör": "İndirilenler",
               "output": {"sayı": 3, "klasör": "İndirilenler"}})
    );
    assert_eq!(a.jobs[0].data["sayı"], 3);
    let a = r.done(&a.jobs[0], json!({"sayı": 4}));
    assert_eq!(
        a.jobs[0].data,
        json!({"n": 4, "yok": null}).as_object().unwrap().clone()
    );
    // a whole $output that is no object: the run stops there
    let a = r.done(&a.jobs[0], json!("çay"));
    assert!(a.jobs.is_empty());
    let Some(Ended::Error { why }) = &r.rt.ended else {
        panic!("{:?}", r.rt.ended)
    };
    assert!(why.contains("not an object"), "{why}");
    assert_eq!(r.rt.state["failed_step"], "#son");
}

#[test]
fn a_skipped_step_leaves_output_and_conditions_read_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            "#kontrol",
            {"#uyar": {"on": [{"$output": false}]}},
            {"#gönder": {"on": [{"$output": true}]}}
        ]}),
    )]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!(true));
    assert_eq!(names(&a), vec!["gönder"]);
    assert_eq!(r.rt.state["output"], true);
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
    // their objects merged into one output, a field both print by step name
    assert_eq!(
        state(&r),
        json!({"order": 43, "output": {"stock": true, "paid": true,
               "durum": {"#check-payment": "tamam", "#check-stock": "düşük"}}})
    );
}

#[test]
fn a_plain_value_side_by_side_is_kept_under_its_step_s_name() {
    let mut r = Run::new(&[("a", json!({"steps": [["#bir", "#iki", "#üç"], "#son"]}))]);
    let a = r.start(json!({}));
    let jobs = a.jobs.clone();
    r.done(&jobs[0], json!(3));
    r.done(&jobs[1], json!({"renk": "mavi"}));
    let a = r.done(&jobs[2], json!(""));
    assert_eq!(names(&a), vec!["son"]);
    // printing nothing adds nothing
    assert_eq!(r.rt.state["output"], json!({"#bir": 3, "renk": "mavi"}));
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
    // the workflow's result is its last step's
    assert_eq!(r.rt.state["output"], json!({"paid": true, "stock": true}));
}

#[test]
fn a_workflow_as_a_step_takes_args_and_gives_its_last_step_s_output() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [{"stok": {"args": {"depo": "İzmir"}}}, "#ship"]}),
        ),
        ("stok", json!({"steps": ["#depo-bak", "#ayır"]})),
    ]);
    let a = r.start(json!({"order": 43}));
    assert_eq!(names(&a), vec!["depo-bak"]);
    assert_eq!(r.rt.state["depo"], "İzmir");
    let a = r.done(&a.jobs[0], json!("A-3"));
    assert_eq!(names(&a), vec!["ayır"]);
    let a = r.done(&a.jobs[0], json!({"ayrıldı": true}));
    assert_eq!(names(&a), vec!["ship"]);
    assert_eq!(r.rt.state["output"], json!({"ayrıldı": true}));
}

#[test]
fn a_step_runs_only_when_its_on_holds() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [
                "#check",
                {"işle": {"args": "$output"}},
                {"#bildir": {"on": [{ "has": "$$API" }]}},
                "#bitir"
            ]}),
        ),
        (
            "işle",
            json!({"on": [{ "has": "$paid" }], "steps": [
                {"#ship":           {"on": [{"$paid": true,  "$stock": true}]}},
                {"#remind-payment": {"on": [{"$paid": false, "$stock": true}]}}
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
        (
            "işle",
            json!({"on": [{ "has": "$paid" }], "steps": ["#ship"]}),
        ),
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
    assert!(!r.rt.state.contains_key("payment_ref"), "nothing joins");
    let timers = a.timers.clone();
    let at: Vec<&str> = timers.iter().map(|t| t.at.as_str()).collect();
    assert_eq!(at, vec!["2026-10-21T11:00:00Z", "2026-10-22T10:00:00Z"]);

    // no field in common with the waiting step's output, or one that
    // disagrees: not this run's; the state is not what it is matched on
    assert!(r.message("m0", json!({"order": 43})).jobs.is_empty());
    assert!(r.message("m1", json!({"tutar": 120})).jobs.is_empty());
    assert!(
        r.message("m2", json!({"payment_ref": "ödeme-2", "paid": true}))
            .jobs
            .is_empty()
    );
    // this run's: it is the step's output, and the step runs again
    let a = r.message("m3", json!({"payment_ref": "ödeme-1", "paid": true}));
    assert_eq!(names(&a), vec!["check-payment"]);
    assert_eq!(a.jobs[0].path, pay.path);
    assert_eq!(
        a.jobs[0].output,
        Some(json!({"payment_ref": "ödeme-1", "paid": true}))
    );
    assert!(!r.rt.state.contains_key("paid"));
    assert!(a.timers.is_empty(), "no step waits now");
    // the same message again changes nothing
    assert!(
        r.message("m3", json!({"payment_ref": "ödeme-1"}))
            .jobs
            .is_empty()
    );
    let a = r.done(&a.jobs[0], json!({"paid": true}));
    assert_eq!(names(&a), vec!["ship"]);
    assert_eq!(r.rt.state["output"], json!({"stock": true, "paid": true}));
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

/// A cluster made, a plan applied and checked: the steps with something to
/// take back name their undo.
fn cluster() -> Value {
    json!({"steps": [{ "#küme-kur": { "undo": "#küme-sil" } },
                     "#plan",
                     { "#uygula": { "undo": "#geri-al" } },
                     "#dene"],
           "error": ["#bildir"]})
}

#[test]
fn on_error_the_finished_steps_are_undone_newest_first_then_the_error_list() {
    let mut r = Run::new(&[("küme", cluster())]);
    let mut a = r.start(json!({"ad": "deneme"}));
    for (step, output) in [
        ("küme-kur", json!({"küme": "k-1"})),
        ("plan", json!("plan-tamam")),
        ("uygula", json!({"sürüm": 2})),
    ] {
        assert_eq!(names(&a), vec![step]);
        a = r.done(&a.jobs[0], output);
    }
    let a = r.failed(&a.jobs[0], "exit 1: yanıt yok");
    assert_eq!(names(&a), vec!["geri-al"]);
    assert_eq!(r.rt.phase, Phase::Undo);
    // each undo gets the result of the step it takes back
    assert_eq!(r.rt.state["output"], json!({"sürüm": 2}));
    // the undos are told what broke, as the error list is
    assert_eq!(r.rt.state["why"], "exit 1: yanıt yok");
    assert_eq!(r.rt.state["failed_step"], "#dene");
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["küme-sil"]);
    assert_eq!(r.rt.state["output"], json!({"küme": "k-1"}));
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(r.rt.phase, Phase::Error);
    assert!(!r.rt.state.contains_key("undo_failed"), "every undo worked");
    r.done(&a.jobs[0], json!({}));
    assert_eq!(
        r.rt.ended,
        Some(Ended::Error {
            why: "exit 1: yanıt yok".into()
        })
    );
}

#[test]
fn only_the_steps_that_finished_are_undone() {
    let mut r = Run::new(&[("küme", cluster())]);
    let a = r.start(json!({"ad": "deneme"}));
    let a = r.done(&a.jobs[0], json!({}));
    // #plan breaks: the cluster is deleted, nothing was applied
    let a = r.failed(&a.jobs[0], "exit 1: plan reddedildi");
    assert_eq!(names(&a), vec!["küme-sil"]);

    // the first step breaks: nothing to undo, straight to the error list
    let mut r = Run::new(&[("küme", cluster())]);
    let a = r.start(json!({"ad": "deneme"}));
    let a = r.failed(&a.jobs[0], "exit 1: docker yok");
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(r.rt.phase, Phase::Error);
}

#[test]
fn an_undo_that_breaks_is_noted_and_the_others_still_run() {
    let mut r = Run::new(&[("küme", cluster())]);
    let mut a = r.start(json!({"ad": "deneme"}));
    for _ in 0..3 {
        a = r.done(&a.jobs[0], json!({}));
    }
    let a = r.failed(&a.jobs[0], "exit 1: yanıt yok");
    let a = r.failed(&a.jobs[0], "exit 2: kilitli");
    assert_eq!(names(&a), vec!["küme-sil"]);
    assert_eq!(
        a.notes,
        vec![json!({"event": "undo.failed", "step": "#geri-al", "why": "exit 2: kilitli"})]
    );
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["bildir"]);
    // the error list is told which undos broke: what they made may be left
    assert_eq!(
        r.rt.state["undo_failed"],
        json!(["#geri-al: exit 2: kilitli"])
    );
    r.done(&a.jobs[0], json!({}));
    // the run's why stays what broke first
    assert_eq!(
        r.rt.ended,
        Some(Ended::Error {
            why: "exit 1: yanıt yok".into()
        })
    );
    assert_eq!(r.rt.state["failed_step"], "#dene");
}

#[test]
fn a_failure_is_no_error_and_undoes_nothing() {
    let mut f = cluster();
    f["failure"] = json!(["#bildir"]);
    let mut r = Run::new(&[("küme", f)]);
    let mut a = r.start(json!({"ad": "deneme"}));
    for _ in 0..3 {
        a = r.done(&a.jobs[0], json!({}));
    }
    let a = r.done(&a.jobs[0], json!({"success": false}));
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(r.rt.phase, Phase::Failure);
}

#[test]
fn a_workflow_as_a_step_has_its_steps_undone_and_its_own_undo() {
    let mut r = Run::new(&[
        (
            "kurulum",
            json!({"steps": [{ "altyapı": { "undo": "#hepsini-sil" } }, "#dene"]}),
        ),
        (
            "altyapı",
            json!({"steps": [{ "#ağ-kur": { "undo": "#ağ-sil" } }, "#disk-kur"]}),
        ),
    ]);
    let a = r.start(json!({"ad": "deneme"}));
    let a = r.done(&a.jobs[0], json!({}));
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["dene"]);
    let a = r.failed(&a.jobs[0], "exit 1");
    assert_eq!(names(&a), vec!["hepsini-sil"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["ağ-sil"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert!(a.jobs.is_empty());
    assert_eq!(
        r.rt.ended,
        Some(Ended::Error {
            why: "exit 1".into()
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

    // or success passed on as args, into the state
    let passed = json!({"steps": ["#remind-stock", {"#kaydet": {"args": {"success": "$output"}}}],
                        "success": ["#teşekkür"], "failure": ["#log"]});
    let mut r = Run::new(&[("sipariş", passed)]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!(false));
    let a = r.done(&a.jobs[0], json!("kaydedildi"));
    assert_eq!(names(&a), vec!["log"]);

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

#[test]
fn a_step_that_cannot_start_starts_nothing_after_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": ["#bir", {"#iki": {"args": "$output"}}, {"#üç": {"args": {"z": 1}}}],
               "error": ["#bildir"]}),
    )]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!("çay"));
    assert_eq!(names(&a), vec!["bildir"]);
    assert!(!r.rt.state.contains_key("z"), "#üç never started");
    assert_eq!(r.rt.state["failed_step"], "#iki");
}

#[test]
fn a_run_written_before_output_is_read_and_goes_on() {
    // as 0.11 kept it: its undos by name, a list side by side with the
    // fields each entry printed
    let old = r##"{"phase": "steps", "state": {"ad": "deneme", "küme": "k-1"},
                   "undo": ["#küme-sil"],
                   "at": {"kind": "flow", "workflow": "küme", "step": 2,
                          "inner": {"kind": "action", "name": "uygula", "started": true}}}"##;
    let rt: Runtime = serde_json::from_str(old).unwrap();
    assert_eq!(rt.undo.len(), 1);
    assert_eq!(rt.undo[0].step, "#küme-sil");
    assert_eq!(rt.undo[0].output, Value::Null);
    let mut r = Run::new(&[("küme", cluster())]);
    r.rt = rt;
    let a = r.failed(
        &StepJob {
            path: vec![0, 2],
            action: "uygula".into(),
            version: None,
            data: Map::new(),
            item: None,
            waiting_since: None,
            output: None,
        },
        "exit 1",
    );
    assert_eq!(names(&a), vec!["küme-sil"]);
}

#[test]
fn a_secret_held_by_a_state_value_breaks_a_step_that_puts_it_in_text() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"#gir": {"args": {"parola": "$$POSTA"}}},
            {"#bildir": {"args": {"text": "giriş: $parola"}}}
        ], "error": ["#uyar"]}),
    )]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(names(&a), vec!["uyar"]);
    assert_eq!(r.rt.state["failed_step"], "#bildir");
    let why = r.rt.state["why"].as_str().unwrap();
    assert!(why.contains("a secret is a whole value only"), "{why}");
}
