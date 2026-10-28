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
    repeat: BTreeMap<String, Value>,
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
            repeat: BTreeMap::new(),
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
            repeat: self.repeat.clone(),
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
    // the other branch runs to its end first; what it gives is not kept
    assert!(r.failed(&pay, "exit 3: bağlantı yok").jobs.is_empty());
    let a = r.done(&stock, json!({"stock": true}));
    assert_eq!(names(&a), vec!["log"]);
    assert!(!r.rt.state.contains_key("stock"));
    assert_eq!(r.rt.state.get("output"), None);
    let log = a.jobs[0].clone();
    assert_eq!(r.rt.state["why"], "exit 3: bağlantı yok");
    assert_eq!(r.rt.state["failed_step"], "#check-payment");
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
fn repeat_runs_the_rest_once_per_item() {
    let mut r = Run::new(&[("posta", json!({"steps": ["#imap-check", "#notify"]}))]);
    r.args
        .insert("imap-check".into(), json!({"host": "imap.örnek.test"}));
    r.repeat.insert(
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
        c.repeat = r.repeat.clone();
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

// ---- steps side by side say how to wait ----

const AD: [&str; 3] = ["#çay", "#şeker", "#ılık"];
const ALAN: [&str; 3] = ["sıcak", "tatlı", "ılık"];

/// A run of `group`, then `#sonra`; a break goes to `#hata`. The jobs are
/// the group's steps, in order: paths `[0, 0, i]`.
fn grup(group: Value) -> (Run, Vec<StepJob>) {
    let mut r = Run::new(&[(
        "küme",
        json!({"steps": [group, "#sonra"], "error": ["#hata"]}),
    )]);
    let a = r.start(json!({}));
    assert_eq!(a.jobs.len(), 3, "{:?}", names(&a));
    (r, a.jobs)
}

/// What step `i` prints.
fn basar(i: usize) -> Value {
    json!({ ALAN[i]: i })
}

fn goes_on(a: &StepAnswer) -> bool {
    names(a) == vec!["sonra"]
}

fn breaks(a: &StepAnswer) -> bool {
    names(a) == vec!["hata"]
}

/// The steps the answer noted as stopped, sorted.
fn stopped(a: &StepAnswer) -> Vec<String> {
    let mut v: Vec<String> = a
        .notes
        .iter()
        .filter(|n| n["event"] == "step.stopped")
        .map(|n| n["step"].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

fn paths(jobs: &[&StepJob]) -> Vec<Vec<usize>> {
    let mut v: Vec<Vec<usize>> = jobs.iter().map(|j| j.path.clone()).collect();
    v.sort();
    v
}

fn sorted(a: &StepAnswer) -> Vec<Vec<usize>> {
    let mut v = a.stopped.clone();
    v.sort();
    v
}

#[test]
fn all_ends_when_every_step_ended_well_in_any_order() {
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let (mut r, j) = grup(json!({"all": AD}));
        for i in &order[..2] {
            assert!(r.done(&j[*i], basar(*i)).jobs.is_empty(), "{order:?}");
        }
        let a = r.done(&j[order[2]], basar(order[2]));
        assert!(goes_on(&a), "{order:?}");
        assert!(a.stopped.is_empty() && stopped(&a).is_empty());
        assert_eq!(
            r.rt.state["output"],
            json!({"sıcak": 0, "tatlı": 1, "ılık": 2})
        );
    }
}

#[test]
fn all_breaks_when_a_step_broke_once_the_others_ran_to_their_end() {
    for group in [json!(AD), json!({"all": AD})] {
        for i in 0..3 {
            let (mut r, j) = grup(group.clone());
            let a = r.failed(&j[i], "exit 3: bağlantı yok");
            assert!(a.jobs.is_empty(), "{group} {i}");
            assert!(a.stopped.is_empty() && stopped(&a).is_empty(), "{i}");
            assert_eq!(r.rt.ended, None);
            let others: Vec<&StepJob> = (0..3).filter(|k| *k != i).map(|k| &j[k]).collect();
            // what the others print is not kept, and one more break is not
            // the first
            assert!(r.done(others[0], json!({"geç": 1})).jobs.is_empty());
            let a = r.failed(others[1], "exit 4: geç");
            assert!(breaks(&a), "{group} {i}");
            assert_eq!(r.rt.state["failed_step"], AD[i]);
            assert_eq!(r.rt.state["why"], "exit 3: bağlantı yok");
            assert_eq!(r.rt.state.get("output"), None, "{group} {i}");
            assert!(a.stopped.is_empty() && stopped(&a).is_empty(), "{i}");
        }
    }
}

#[test]
fn all_that_breaks_stops_a_waiting_step_and_a_workflow_goes_no_further() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [["#çay", "#şeker", "demleme"], "#sonra"], "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su", "#yaprak"]})),
    ]);
    let a = r.start(json!({}));
    assert_eq!(names(&a), vec!["çay", "şeker", "su"]);
    let (cay, seker, su) = (a.jobs[0].clone(), a.jobs[1].clone(), a.jobs[2].clone());
    r.waiting(&cay, json!({"sipariş": 7}));
    let a = r.failed(&seker, "exit 1: şeker yok");
    assert!(a.jobs.is_empty());
    // the waiting step waits no more; #su still runs
    assert_eq!(stopped(&a), vec!["#çay"]);
    assert_eq!(sorted(&a), vec![cay.path.clone()]);
    assert!(a.timers.is_empty(), "nothing waits now");
    assert_eq!(r.rt.ended, None);
    assert!(r.message("m1", json!({"sipariş": 7})).jobs.is_empty());
    // #su ends: its workflow goes no further, and the group breaks
    let a = r.done(&su, json!({"sıcak": true}));
    assert!(breaks(&a), "{:?}", names(&a));
    assert_eq!(r.rt.state["failed_step"], "#şeker");
    assert_eq!(r.rt.state.get("output"), None);
}

#[test]
fn a_step_that_starts_to_wait_in_a_group_that_broke_waits_no_more() {
    let (mut r, j) = grup(json!(AD));
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    let a = r.waiting(&j[1], json!({"sipariş": 7}));
    assert!(a.jobs.is_empty() && a.timers.is_empty(), "{a:?}");
    assert_eq!(stopped(&a), vec!["#şeker"]);
    assert!(r.message("m1", json!({"sipariş": 7})).jobs.is_empty());
    let a = r.done(&j[2], basar(2));
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#çay");
}

#[test]
fn race_ends_with_the_first_step_to_end_and_names_the_winner() {
    for i in 0..3 {
        let (mut r, j) = grup(json!({"race": AD}));
        let a = r.done(&j[i], basar(i));
        assert!(goes_on(&a), "{i}");
        assert_eq!(r.rt.state["output"], json!({ ALAN[i]: i, "winner": AD[i] }));
        let others: Vec<&StepJob> = (0..3).filter(|k| *k != i).map(|k| &j[k]).collect();
        assert_eq!(sorted(&a), paths(&others));
        assert_eq!(stopped(&a).len(), 2);
        // the others' answers come too late
        assert!(r.done(others[0], json!({"geç": 9})).jobs.is_empty());
        assert!(r.failed(others[1], "exit 1").jobs.is_empty());
        assert_eq!(r.rt.state["output"], json!({ ALAN[i]: i, "winner": AD[i] }));
    }
}

#[test]
fn race_breaks_when_the_first_to_end_broke() {
    let (mut r, j) = grup(json!({"race": AD}));
    let a = r.failed(&j[2], "exit 2: yok");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#ılık");
    assert_eq!(stopped(&a), vec!["#çay", "#şeker"]);
}

#[test]
fn any_ends_with_the_first_step_to_end_well() {
    let (mut r, j) = grup(json!({"any": AD}));
    assert!(r.failed(&j[0], "exit 1").jobs.is_empty());
    let a = r.done(&j[1], basar(1));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"tatlı": 1, "winner": "#şeker", "failed": ["#çay"], "why": {"#çay": "exit 1"}})
    );
    assert_eq!(stopped(&a), vec!["#ılık"]);
    assert_eq!(sorted(&a), vec![j[2].path.clone()]);
    // the same when the winner is first
    let (mut r, j) = grup(json!({"any": AD}));
    let a = r.done(&j[2], json!(7));
    assert!(goes_on(&a));
    assert_eq!(r.rt.state["output"], json!({"#ılık": 7, "winner": "#ılık"}));
    assert_eq!(stopped(&a), vec!["#çay", "#şeker"]);
}

#[test]
fn any_breaks_when_every_step_broke_with_the_first_by_time() {
    let (mut r, j) = grup(json!({"any": AD}));
    assert!(r.failed(&j[2], "exit 2: ılık").jobs.is_empty());
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    let a = r.failed(&j[1], "exit 3: şeker");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#ılık");
    assert_eq!(r.rt.state["why"], "exit 2: ılık");
    assert!(a.stopped.is_empty(), "none was left running");
}

#[test]
fn some_with_a_count_ends_when_that_many_ended_well() {
    for (first, second, left) in [(0, 2, 1), (2, 1, 0), (1, 0, 2)] {
        let (mut r, j) = grup(json!({"some": AD, "count": 2}));
        assert!(r.done(&j[first], basar(first)).jobs.is_empty());
        let a = r.done(&j[second], basar(second));
        assert!(goes_on(&a), "{first} {second}");
        let mut want = serde_json::Map::new();
        for k in [first, second] {
            want.insert(ALAN[k].into(), json!(k));
        }
        assert_eq!(r.rt.state["output"], Value::Object(want));
        assert_eq!(stopped(&a), vec![AD[left]]);
        assert_eq!(sorted(&a), vec![j[left].path.clone()]);
    }
    // a step that broke does not count, and is named
    let (mut r, j) = grup(json!({"some": AD, "count": 2}));
    assert!(r.failed(&j[1], "exit 1").jobs.is_empty());
    assert!(r.done(&j[0], basar(0)).jobs.is_empty());
    let a = r.done(&j[2], basar(2));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"sıcak": 0, "ılık": 2, "failed": ["#şeker"], "why": {"#şeker": "exit 1"}})
    );
    assert!(a.stopped.is_empty());
}

#[test]
fn some_with_a_count_breaks_when_the_count_cannot_be_reached() {
    let (mut r, j) = grup(json!({"some": AD, "count": 2}));
    assert!(r.failed(&j[1], "exit 1: şeker").jobs.is_empty());
    let a = r.failed(&j[0], "exit 2: çay");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#şeker");
    assert_eq!(stopped(&a), vec!["#ılık"]);
    // count 3 of 3: one break is enough
    let (mut r, j) = grup(json!({"some": AD, "count": 3}));
    let a = r.failed(&j[2], "exit 1");
    assert!(breaks(&a));
    assert_eq!(stopped(&a), vec!["#çay", "#şeker"]);
}

#[test]
fn some_with_names_ends_when_the_named_steps_ended_well() {
    let group = json!({"some": AD, "names": ["#çay", "#ılık"]});
    let (mut r, j) = grup(group.clone());
    assert!(r.done(&j[2], basar(2)).jobs.is_empty());
    let a = r.done(&j[0], basar(0));
    assert!(goes_on(&a));
    assert_eq!(r.rt.state["output"], json!({"sıcak": 0, "ılık": 2}));
    assert_eq!(stopped(&a), vec!["#şeker"]);
    // a step not named that ended meanwhile is part of the result
    let (mut r, j) = grup(group.clone());
    assert!(r.done(&j[1], basar(1)).jobs.is_empty());
    assert!(r.done(&j[0], basar(0)).jobs.is_empty());
    let a = r.done(&j[2], basar(2));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"sıcak": 0, "tatlı": 1, "ılık": 2})
    );
    assert!(a.stopped.is_empty());
    // a step not named that broke does not matter
    let (mut r, j) = grup(group.clone());
    assert!(r.failed(&j[1], "exit 1").jobs.is_empty());
    assert!(r.done(&j[0], basar(0)).jobs.is_empty());
    assert!(goes_on(&r.done(&j[2], basar(2))));
    // a named one that broke breaks the group
    let (mut r, j) = grup(group);
    assert!(r.done(&j[0], basar(0)).jobs.is_empty());
    let a = r.failed(&j[2], "exit 4: ılık");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#ılık");
    assert_eq!(stopped(&a), vec!["#şeker"]);
}

#[test]
fn settled_ends_when_every_step_ended_and_never_breaks() {
    let (mut r, j) = grup(json!({"settled": AD}));
    assert!(r.failed(&j[1], "exit 1: şeker yok").jobs.is_empty());
    assert!(r.done(&j[0], basar(0)).jobs.is_empty());
    let a = r.failed(&j[2], "exit 2: ılık yok");
    assert!(goes_on(&a));
    assert!(a.stopped.is_empty());
    assert_eq!(
        r.rt.state["output"],
        json!({"sıcak": 0, "failed": ["#şeker", "#ılık"],
               "why": {"#şeker": "exit 1: şeker yok", "#ılık": "exit 2: ılık yok"}})
    );
    // nothing broke: failed is empty
    let (mut r, j) = grup(json!({"settled": AD}));
    for i in [2, 0] {
        r.done(&j[i], basar(i));
    }
    let a = r.done(&j[1], basar(1));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"sıcak": 0, "tatlı": 1, "ılık": 2, "failed": [], "why": {}})
    );
}

#[test]
fn itero_s_own_keys_win_over_a_step_s_field_of_the_same_name() {
    let (mut r, j) = grup(json!({"race": AD}));
    let a = r.done(&j[0], json!({"winner": "tavşan", "n": 1}));
    assert!(goes_on(&a));
    assert_eq!(r.rt.state["output"], json!({"winner": "#çay", "n": 1}));
    let (mut r, j) = grup(json!({"settled": AD}));
    r.done(&j[0], json!({"failed": 5, "why": "x"}));
    r.done(&j[1], basar(1));
    r.done(&j[2], basar(2));
    assert_eq!(
        r.rt.state["output"],
        json!({"failed": [], "why": {}, "tatlı": 1, "ılık": 2})
    );
}

#[test]
fn fail_never_waits_for_all_and_never_breaks() {
    let (mut r, j) = grup(json!({"all": AD, "fail": "never"}));
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    assert!(r.done(&j[1], basar(1)).jobs.is_empty());
    let a = r.done(&j[2], basar(2));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"tatlı": 1, "ılık": 2, "failed": ["#çay"], "why": {"#çay": "exit 1: çay"}})
    );
    // a race whose first step broke ends, and stops the others
    let (mut r, j) = grup(json!({"race": AD, "fail": "never"}));
    let a = r.failed(&j[1], "exit 1: şeker");
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["#çay", "#ılık"]);
    assert_eq!(
        r.rt.state["output"],
        json!({"failed": ["#şeker"], "why": {"#şeker": "exit 1: şeker"}})
    );
}

#[test]
fn fail_any_breaks_the_group_at_the_first_break() {
    let (mut r, j) = grup(json!({"any": AD, "fail": "any"}));
    let a = r.failed(&j[0], "exit 1: çay");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#çay");
    assert_eq!(stopped(&a), vec!["#ılık", "#şeker"]);
    // settled waits for its steps all the same: #şeker runs to its end
    let (mut r, j) = grup(json!({"settled": AD, "fail": "any"}));
    r.done(&j[0], basar(0));
    let a = r.failed(&j[2], "exit 1: ılık");
    assert!(a.jobs.is_empty() && a.stopped.is_empty());
    let a = r.done(&j[1], basar(1));
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#ılık");
}

#[test]
fn fail_all_breaks_only_when_every_step_broke() {
    // some 2 of 3 would break at two breaks: it waits for the last
    let (mut r, j) = grup(json!({"some": AD, "count": 2, "fail": "all"}));
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    assert!(r.failed(&j[1], "exit 2: şeker").jobs.is_empty());
    assert_eq!(r.rt.ended, None);
    let a = r.failed(&j[2], "exit 3: ılık");
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#çay");
    // one ended well: the count cannot be reached, nothing breaks, and the
    // group ends with the last
    let (mut r, j) = grup(json!({"some": AD, "count": 2, "fail": "all"}));
    r.failed(&j[0], "exit 1");
    r.failed(&j[1], "exit 2");
    let a = r.done(&j[2], basar(2));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"ılık": 2, "failed": ["#çay", "#şeker"],
               "why": {"#çay": "exit 1", "#şeker": "exit 2"}})
    );
}

#[test]
fn fail_with_names_breaks_when_one_of_them_broke() {
    let (mut r, j) = grup(json!({"all": AD, "fail": ["#ılık"]}));
    // all would break at #çay; here only #ılık's break counts, and the
    // group breaks once #şeker has ended
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    assert_eq!(r.rt.ended, None);
    let a = r.failed(&j[2], "exit 2: ılık");
    assert!(a.jobs.is_empty() && a.stopped.is_empty());
    let a = r.done(&j[1], basar(1));
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#ılık");
    assert_eq!(r.rt.state["why"], "exit 2: ılık");
    // and when it did not break, the group ends with the others
    let (mut r, j) = grup(json!({"all": AD, "fail": "#ılık"}));
    r.failed(&j[0], "exit 1");
    r.done(&j[2], basar(2));
    let a = r.done(&j[1], basar(1));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"tatlı": 1, "ılık": 2, "failed": ["#çay"], "why": {"#çay": "exit 1"}})
    );
}

#[test]
fn a_waiting_step_stopped_by_a_race_waits_no_more() {
    let mut r = Run::new(&[(
        "küme",
        json!({"steps": [{"race": ["#çay", "#şeker"]}, "#sonra"], "error": ["#hata"]}),
    )]);
    let a = r.start(json!({}));
    let (cay, seker) = (a.jobs[0].clone(), a.jobs[1].clone());
    let a = r.waiting(&cay, json!({"sipariş": 7}));
    assert!(!a.timers.is_empty());
    let a = r.done(&seker, json!({"tatlı": true}));
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["#çay"]);
    assert_eq!(sorted(&a), vec![cay.path.clone()]);
    assert!(a.timers.is_empty(), "nothing waits now");
    assert_eq!(
        r.rt.state["output"],
        json!({"tatlı": true, "winner": "#şeker"})
    );
    // the data it waited for comes, and no step takes it
    assert!(r.message("m1", json!({"sipariş": 7})).jobs.is_empty());
    // an hour later, nothing runs again
    r.now = "2026-10-21T11:00:00Z".into();
    assert!(
        r.send("timer.fired", json!({"kind": "retry"}))
            .jobs
            .is_empty()
    );
    // a late wait of the stopped step is ignored
    let a = r.waiting(&cay, json!({"sipariş": 7}));
    assert!(a.log.iter().any(|l| l.contains("ignored")), "{:?}", a.log);
}

#[test]
fn a_step_that_waited_too_long_is_a_step_that_broke() {
    let mut r = Run::new(&[(
        "küme",
        json!({"steps": [{"settled": ["#çay", "#şeker"]}, "#sonra"],
               "timeout": "2h", "error": ["#hata"]}),
    )]);
    let a = r.start(json!({}));
    let (cay, seker) = (a.jobs[0].clone(), a.jobs[1].clone());
    r.waiting(&cay, json!({"sipariş": 7}));
    r.now = "2026-10-21T12:00:00Z".into();
    let a = r.send("timer.fired", json!({"kind": "deadline"}));
    assert!(a.jobs.is_empty(), "settled goes on when #şeker ends");
    let a = r.done(&seker, json!({"tatlı": true}));
    assert!(goes_on(&a));
    let out = &r.rt.state["output"];
    assert_eq!(out["failed"], json!(["#çay"]));
    assert!(out["why"]["#çay"].as_str().unwrap().contains("2h"));
}

#[test]
fn a_workflow_stopped_by_a_race_stops_all_its_actions() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"race": ["#çay", "demleme"]}, "#sonra"]}),
        ),
        ("demleme", json!({"steps": [["#su", "#yaprak"], "#bekle"]})),
    ]);
    let a = r.start(json!({}));
    assert_eq!(names(&a), vec!["çay", "su", "yaprak"]);
    let jobs = a.jobs.clone();
    assert_eq!(jobs[1].path, vec![0, 0, 1, 0, 0]);
    let a = r.done(&jobs[0], json!({"içildi": true}));
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["demleme"], "one note for the workflow");
    assert_eq!(sorted(&a), vec![jobs[1].path.clone(), jobs[2].path.clone()]);
    assert_eq!(
        r.rt.state["output"],
        json!({"içildi": true, "winner": "#çay"})
    );
    // what the workflow's actions print now reaches nothing
    assert!(r.done(&jobs[1], json!({})).jobs.is_empty());
}

/// A run of `group`, then `#sonra`, where `önbellek` and `arşiv` are
/// workflows whose `on` does not hold; a break goes to `#hata`.
fn atlanan(group: Value) -> (Run, StepAnswer) {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [group, "#sonra"], "error": ["#hata"]}),
        ),
        (
            "önbellek",
            json!({"on": [{"has": "$yok"}], "steps": ["#oku"]}),
        ),
        ("arşiv", json!({"on": [{"has": "$yok"}], "steps": ["#oku"]})),
    ]);
    let a = r.start(json!({}));
    (r, a)
}

#[test]
fn a_step_that_does_not_run_is_out_of_its_group() {
    // it does not win a race or any: the others are waited for
    for key in ["race", "any"] {
        let (mut r, a) = atlanan(json!({ key: ["önbellek", "#canlı"] }));
        assert_eq!(names(&a), vec!["canlı"], "{key}");
        assert!(stopped(&a).is_empty(), "{key}");
        let a = r.done(&a.jobs[0], json!({"fiyat": 5}));
        assert!(goes_on(&a), "{key}");
        assert_eq!(
            r.rt.state["output"],
            json!({"fiyat": 5, "winner": "#canlı"}),
            "{key}"
        );
    }
    // nor count toward some, nor break all
    let (mut r, a) = atlanan(json!({"some": ["önbellek", "#canlı", "#yedek"], "count": 2}));
    assert!(r.done(&a.jobs[0], json!({"a": 1})).jobs.is_empty());
    assert!(goes_on(&r.done(&a.jobs[1], json!({"b": 2}))));
    let (mut r, a) = atlanan(json!({"all": ["önbellek", "#canlı"]}));
    assert!(goes_on(&r.done(&a.jobs[0], json!({"fiyat": 5}))));
    assert_eq!(r.rt.state["output"], json!({"fiyat": 5}));
    // when no step of it runs, the group ends with nothing
    let (_, a) = atlanan(json!({"race": ["önbellek", "arşiv"]}));
    assert!(goes_on(&a));
}

#[test]
fn a_group_that_ends_while_a_step_broke_says_which_and_why() {
    // a race whose first step broke, kept going by fail
    let (mut r, j) = grup(json!({"race": AD, "fail": "all"}));
    let a = r.failed(&j[1], "exit 7: bağlantı yok");
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"failed": ["#şeker"], "why": {"#şeker": "exit 7: bağlantı yok"}})
    );
    // any, its winner after a break
    let (mut r, j) = grup(json!({"any": AD}));
    r.failed(&j[0], "exit 1: çay");
    assert!(goes_on(&r.done(&j[2], basar(2))));
    assert_eq!(
        r.rt.state["output"],
        json!({"ılık": 2, "winner": "#ılık", "failed": ["#çay"], "why": {"#çay": "exit 1: çay"}})
    );
    // nothing broke: no failed, but for settled and fail never
    let (mut r, j) = grup(json!({"any": AD}));
    assert!(goes_on(&r.done(&j[0], basar(0))));
    assert_eq!(r.rt.state["output"], json!({"sıcak": 0, "winner": "#çay"}));
}

#[test]
fn a_workflow_that_breaks_inside_a_group_is_a_step_of_the_group_that_broke() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"settled": ["#çay", "demleme"]}, "#sonra"], "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su", "#yaprak"]})),
    ]);
    let a = r.start(json!({}));
    let (cay, su) = (a.jobs[0].clone(), a.jobs[1].clone());
    assert!(r.done(&cay, basar(0)).jobs.is_empty());
    let a = r.done(&su, json!({}));
    assert_eq!(names(&a), vec!["yaprak"]);
    let a = r.failed(&a.jobs[0], "exit 1: yaprak yok");
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"sıcak": 0, "failed": ["demleme"],
               "why": {"demleme": "exit 1: yaprak yok"}})
    );
    // without settled, the step that broke is named, not the workflow
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [["#çay", "demleme"], "#sonra"], "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su", "#yaprak"]})),
    ]);
    let a = r.start(json!({}));
    let (cay, su) = (a.jobs[0].clone(), a.jobs[1].clone());
    let a = r.done(&su, json!({}));
    let a = r.failed(&a.jobs[0], "exit 1: yaprak yok");
    assert!(a.jobs.is_empty() && a.stopped.is_empty(), "#çay runs on");
    let a = r.done(&cay, basar(0));
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#yaprak");
}

#[test]
fn a_group_in_a_group_decides_for_its_own_steps() {
    let flows = || {
        [
            (
                "küme",
                json!({"steps": [{"all": ["#çay", "iç"]}, "#sonra"], "error": ["#hata"]}),
            ),
            (
                "iç",
                json!({"steps": [{"any": ["#şeker", "#ılık"]}, "#karıştır"]}),
            ),
        ]
    };
    let mut r = Run::new(&flows());
    let a = r.start(json!({}));
    assert_eq!(names(&a), vec!["çay", "şeker", "ılık"]);
    let j = a.jobs.clone();
    let a = r.failed(&j[1], "exit 1: şeker");
    assert!(a.jobs.is_empty());
    let a = r.done(&j[2], basar(2));
    assert_eq!(
        names(&a),
        vec!["karıştır"],
        "the inner any ended with #ılık"
    );
    let a = r.done(&a.jobs[0], json!({"karışık": true}));
    assert!(a.jobs.is_empty());
    let a = r.done(&j[0], basar(0));
    assert!(goes_on(&a));
    // the inner any broke: every step of it did; the outer all breaks
    // once #çay has ended
    let mut r = Run::new(&flows());
    let j = r.start(json!({})).jobs;
    r.failed(&j[1], "exit 1: şeker");
    let a = r.failed(&j[2], "exit 2: ılık");
    assert!(a.jobs.is_empty() && a.stopped.is_empty());
    let a = r.done(&j[0], basar(0));
    assert!(breaks(&a));
    assert_eq!(r.rt.state["failed_step"], "#şeker");
    // and a race around a group still breaking stops what runs in it
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"race": ["iç", "#hızlı"]}, "#sonra"], "error": ["#hata"]}),
        ),
        ("iç", json!({"steps": [["#çay", "#şeker"]]})),
    ]);
    let j = r.start(json!({})).jobs;
    assert_eq!(names(&r.failed(&j[0], "exit 1: çay")), Vec::<String>::new());
    let a = r.done(&j[2], json!({"hız": 1}));
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["iç"]);
    assert_eq!(sorted(&a), vec![j[1].path.clone()]);
}

#[test]
fn a_group_is_read_from_a_run_written_before_groups_said_how_to_wait() {
    let old = r##"{"phase": "steps", "state": {},
        "at": {"kind": "flow", "workflow": "küme", "step": 0,
               "inner": {"kind": "side", "entries": [
                  {"name": "#çay", "output": {"sıcak": 0}},
                  {"name": "#şeker", "at": {"kind": "action", "name": "şeker", "started": true}}]}}}"##;
    let rt: Runtime = serde_json::from_str(old).unwrap();
    let mut r = Run::new(&[(
        "küme",
        json!({"steps": [["#çay", "#şeker"], "#sonra"], "error": ["#hata"]}),
    )]);
    r.rt = rt;
    let seker = StepJob {
        path: vec![0, 0, 1],
        action: "şeker".into(),
        version: None,
        data: Map::new(),
        item: None,
        waiting_since: None,
        output: None,
    };
    let a = r.done(&seker, json!({"tatlı": 1}));
    assert!(goes_on(&a));
    assert_eq!(r.rt.state["output"], json!({"sıcak": 0, "tatlı": 1}));
    // and a group's position goes through JSON
    let (r, _) = grup(json!({"some": AD, "names": ["#çay"], "fail": ["#ılık"]}));
    let text = serde_json::to_string(&r.rt).unwrap();
    assert_eq!(serde_json::from_str::<Runtime>(&text).unwrap(), r.rt);
}

#[test]
fn a_list_of_args_joins_the_state_as_one_object_unless_strict() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"#sor": {"args": [{"url": "$adres", "adet": "$n"}, {"title": "etiket", "text": "ayna $n"}]}},
            {"#bildir": {"args": [{"fiyat": "$output"}], "strict": true}},
            "#son"
        ]}),
    )]);
    let a = r.start(json!({"adres": "http://127.0.0.1:9/fiyat", "n": 2}));
    assert_eq!(
        a.jobs[0].data,
        json!({"url": "http://127.0.0.1:9/fiyat", "adet": 2, "etiket": "ayna 2"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(r.rt.state["etiket"], "ayna 2");
    assert_eq!(r.rt.state["adet"], 2);
    let a = r.done(&a.jobs[0], json!(42.5));
    assert_eq!(a.jobs[0].data["fiyat"], 42.5);
    assert!(!r.rt.state.contains_key("fiyat"), "strict: the step's only");
}

#[test]
fn one_action_twice_in_a_group_runs_with_each_step_s_args_and_is_read_by_its_as() {
    let mut r = Run::new(&[(
        "fiyat",
        json!({"steps": [
            {"any": [
                {"#ayna": {"args": [{"url": "$bir"}], "as": "ayna-1"}},
                {"#ayna": {"args": [{"url": "$iki"}], "as": "ayna-2"}}
            ]},
            "#sonra"
        ]}),
    )]);
    let a =
        r.start(json!({"bir": "http://127.0.0.1:9/fiyat", "iki": "http://127.0.0.1:8765/fiyat"}));
    assert_eq!(names(&a), vec!["ayna", "ayna"]);
    assert_eq!(a.jobs[0].data["url"], "http://127.0.0.1:9/fiyat");
    assert_eq!(a.jobs[1].data["url"], "http://127.0.0.1:8765/fiyat");
    // the first copy breaks, the second gives a plain value: it wins by its as
    assert!(r.failed(&a.jobs[0], "exit 7: bağlantı yok").jobs.is_empty());
    let a = r.done(&a.jobs[1], json!("42.50"));
    assert_eq!(names(&a), vec!["sonra"]);
    // the copy that broke is named by its as too
    assert_eq!(
        r.rt.state["output"],
        json!({"#ayna-2": "42.50", "winner": "#ayna-2",
               "failed": ["#ayna-1"], "why": {"#ayna-1": "exit 7: bağlantı yok"}})
    );
}

#[test]
fn a_step_of_a_group_whose_on_does_not_hold_is_out_of_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"race": [{"#önbellek": {"on": [{"$taze": true}]}}, "#canlı"]},
            "#sonra"
        ]}),
    )]);
    let a = r.start(json!({"taze": false}));
    assert_eq!(names(&a), vec!["canlı"]);
    let a = r.done(&a.jobs[0], json!({"fiyat": 5}));
    assert_eq!(names(&a), vec!["sonra"]);
    assert_eq!(
        r.rt.state["output"],
        json!({"fiyat": 5, "winner": "#canlı"})
    );
}

#[test]
fn a_step_of_a_group_whose_args_cannot_be_read_breaks_in_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"all": [{"#a": {"args": "$yok"}}, "#b"], "fail": "never"},
            "#sonra"
        ]}),
    )]);
    let a = r.start(json!({}));
    assert_eq!(names(&a), vec!["b"]);
    let a = r.done(&a.jobs[0], json!({"b": 1}));
    assert_eq!(names(&a), vec!["sonra"]);
    assert_eq!(r.rt.state["output"]["failed"], json!(["#a"]));
}

#[test]
fn an_action_s_list_of_args_is_one_set_of_values_not_a_repeat() {
    let mut r = Run::new(&[("posta", json!({"steps": ["#imap-check", "#notify"]}))]);
    r.args.insert(
        "imap-check".into(),
        json!([{"host": "imap.örnek.test"}, {"user": "ayşe@örnek.test"}]),
    );
    let a = r.start(json!({}));
    assert!(a.split.is_empty());
    assert_eq!(names(&a), vec!["imap-check"]);
    assert_eq!(a.jobs[0].item, None);
}
