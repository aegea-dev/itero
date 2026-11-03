//! The manager over steps: one decision per event, JSON in, JSON out.
use itero::engine::Event;
use itero::engine::steps::{
    self, Ended, Phase, Runtime, StepAnswer, StepChild, StepInput, StepJob,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const NOW: &str = "2026-10-21T10:00:00Z";

/// A run of `root`, fed one event at a time, keeping its runtime.
struct Run {
    root: String,
    flows: BTreeMap<String, Value>,
    repeat: BTreeMap<String, Value>,
    defined: Vec<String>,
    /// It is a workflow used as a step: a run another run's step started.
    child: bool,
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
            repeat: BTreeMap::new(),
            defined: vec![],
            child: false,
            rt: Runtime::default(),
            now: NOW.into(),
            seq: 0,
        }
    }

    /// A run of `flows[0]` that another run's step started.
    fn child(flows: &[(&str, Value)]) -> Run {
        let mut r = Run::new(flows);
        r.child = true;
        r
    }

    fn send(&mut self, kind: &str, data: Value) -> StepAnswer {
        self.try_send(kind, data).unwrap()
    }

    fn try_send(&mut self, kind: &str, data: Value) -> Result<StepAnswer, String> {
        self.seq += 1;
        let input = StepInput {
            now: self.now.clone(),
            workflow: self.root.clone(),
            flows: self.flows.clone(),
            repeat: self.repeat.clone(),
            config: Map::new(),
            defined: self.defined.clone(),
            child: self.child,
            runtime: self.rt.clone(),
            event: Event {
                id: format!("olay-{}", self.seq),
                kind: kind.into(),
                data,
            },
        };
        let a = steps::decide(&input)?;
        self.rt = a.runtime.clone();
        Ok(a)
    }

    /// Starts the run as the daemon does: the message's data is its
    /// `$event`.
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

    /// The run its step at `c.path` started ended, and returned what
    /// `steps::returned` gives.
    fn ended(&mut self, c: &StepChild, returned: Value) -> StepAnswer {
        let mut d = returned;
        d["path"] = json!(c.path);
        d["run"] = json!("çocuk-1");
        self.send("workflow.ended", d)
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

/// The runs an answer starts: each workflow and its `$event`.
fn children(a: &StepAnswer) -> Vec<(String, Value)> {
    a.children
        .iter()
        .map(|c| (c.workflow.clone(), Value::Object(c.event.clone())))
        .collect()
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
    assert_eq!(state(&r), json!({"event": {"sipariş": 43, "tutar": 120}}));
    assert_eq!(r.rt.phase, Phase::Steps);
}

#[test]
fn a_step_s_result_is_output_and_kept_by_its_name() {
    let mut r = Run::new(&[("a", json!({"steps": ["#bir", "#iki", "#üç"]}))]);
    let a = r.start(json!({"geçici": "x", "kalıcı": 1}));
    let a = r.done(&a.jobs[0], json!({"metin": "çay", "geçici": null}));
    assert_eq!(
        state(&r),
        json!({"event": {"geçici": "x", "kalıcı": 1},
               "output": {"metin": "çay", "geçici": null},
               "steps": {"bir": {"metin": "çay", "geçici": null}}})
    );
    assert_eq!(names(&a), vec!["iki"]);
    // the next step's result takes the place of $output, printing nothing too
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(r.rt.state["output"], "");
    assert_eq!(r.rt.state["steps"]["iki"], "");
    assert_eq!(r.rt.state["steps"]["bir"]["metin"], "çay");
    assert_eq!(names(&a), vec!["üç"]);
    let a = r.done(&a.jobs[0], json!(524));
    assert_eq!(r.rt.state["output"], 524);
    assert!(a.jobs.is_empty());
    assert_eq!(r.rt.ended, Some(Ended::Done));
}

#[test]
fn a_step_s_args_reach_its_action_and_nothing_else() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"#say": {"args": {"klasör": "İndirilenler", "parola": "$$POSTA"}}},
            {"#bildir": {"args": {"adet": "$output", "nerede": "$steps.say.yer"}}},
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
    // args join nothing: the state is the event, the results, the manifest
    assert_eq!(state(&r), json!({"event": {}}));
    let a = r.done(&a.jobs[0], json!({"yer": "Masaüstü", "sayı": 524}));
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(
        a.jobs[0].data,
        json!({"adet": {"yer": "Masaüstü", "sayı": 524}, "nerede": "Masaüstü"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(!r.rt.state.contains_key("adet"));
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(names(&a), vec!["son"]);
    assert!(a.jobs[0].data.is_empty(), "#son is given nothing");
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#son"));
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
        r.rt.state["output"],
        json!({"stock": true, "paid": true,
               "durum": {"check-payment": "tamam", "check-stock": "düşük"}})
    );
    // and each one's own result by its name
    assert_eq!(
        r.rt.state["steps"]["check-stock"],
        json!({"stock": true, "durum": "düşük"})
    );
    assert_eq!(r.rt.state["steps"]["check-payment"]["paid"], true);
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
    assert_eq!(r.rt.state["output"], json!({"bir": 3, "renk": "mavi"}));
    assert_eq!(r.rt.state["steps"]["bir"], 3);
}

#[test]
fn a_workflow_as_a_step_starts_a_run_of_its_own_and_waits_for_it() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [
                "#öde",
                {"stok": {"args": {"depo": "İzmir", "adet": "$output.adet"}}},
                {"#ship": {"args": {"n": "$output", "durum": "$steps.stok.status",
                                     "raf": "$steps.stok.steps.ayır.ayrıldı"}}}
            ]}),
        ),
        ("stok", json!({"steps": ["#depo-bak", "#ayır"]})),
    ]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({"adet": 2}));
    // no job of stok's: a run of its own, its $event the args given
    assert!(a.jobs.is_empty());
    assert_eq!(
        a.children,
        vec![StepChild {
            path: vec![0, 1],
            workflow: "stok".into(),
            event: serde_json::from_value(json!({"depo": "İzmir", "adet": 2})).unwrap(),
        }]
    );
    let stok = a.children[0].clone();
    assert_eq!(r.rt.ended, None, "it waits for its child");
    assert!(!steps::is_waiting(&r.rt), "running, not waiting for data");
    // the child's answer for another step, or before its own, is not this
    let wrong = StepChild {
        path: vec![0, 2],
        ..stok.clone()
    };
    assert!(r.ended(&wrong, json!({"output": 1})).jobs.is_empty());
    // what it returned: its $output for the next step, and kept by its name
    let a = r.ended(
        &stok,
        json!({"output": {"ayrıldı": true}, "steps": {"depo-bak": "A-3", "ayır": {"ayrıldı": true}},
               "status": "done"}),
    );
    assert_eq!(names(&a), vec!["ship"]);
    assert_eq!(r.rt.state["output"], json!({"ayrıldı": true}));
    assert_eq!(a.jobs[0].data["n"], json!({"ayrıldı": true}));
    assert_eq!(a.jobs[0].data["durum"], "done");
    assert_eq!(a.jobs[0].data["raf"], true);
    assert_eq!(
        r.rt.state["steps"]["stok"],
        json!({"output": {"ayrıldı": true}, "steps": {"depo-bak": "A-3", "ayır": {"ayrıldı": true}},
               "status": "done"})
    );
    // a second answer for it reaches nothing
    let ship = a.jobs[0].clone();
    assert!(r.ended(&stok, json!({"output": 2})).jobs.is_empty());
    assert_eq!(names(&r.done(&ship, json!({}))), Vec::<String>::new());
    assert_eq!(r.rt.ended, Some(Ended::Done));
}

#[test]
fn a_run_started_by_a_step_is_its_own_workflow_with_its_end_lists() {
    let stok = json!({"steps": [{"#depo-bak": {"args": {"depo": "$event.depo"}}}, "#ayır"],
                      "success": [{"#bildir": {"args": {"raf": "$steps.depo-bak"}}}]});
    let mut r = Run::child(&[("stok", stok)]);
    let a = r.start(json!({"depo": "İzmir"}));
    assert_eq!(a.jobs[0].data["depo"], "İzmir", "its $event: its args");
    let a = r.done(&a.jobs[0], json!("A-3"));
    let a = r.done(&a.jobs[0], json!({"ayrıldı": true}));
    // its own success list runs
    assert_eq!(names(&a), vec!["bildir"]);
    assert_eq!(a.jobs[0].data["raf"], "A-3");
    r.done(&a.jobs[0], json!({"bildirildi": true}));
    assert_eq!(r.rt.ended, Some(Ended::Done));
    // rule 1: the success list's result, and its steps
    assert_eq!(
        steps::returned(&r.rt),
        json!({"output": {"bildirildi": true},
               "steps": {"depo-bak": "A-3", "ayır": {"ayrıldı": true}, "bildir": {"bildirildi": true}},
               "status": "done"})
    );
    // without lists: its last step's
    let mut r = Run::child(&[("stok", json!({"steps": ["#depo-bak"]}))]);
    let a = r.start(json!({}));
    r.done(&a.jobs[0], json!("A-3"));
    assert_eq!(steps::returned(&r.rt)["output"], "A-3");
    assert_eq!(steps::returned(&r.rt)["status"], "done");
}

#[test]
fn a_child_that_broke_undoes_its_steps_then_runs_its_error_list() {
    let altyapı = json!({"steps": [{"#ağ-kur": {"undo": "#ağ-sil"}}, "#disk-kur"],
                         "error": [{"#rapor": {"args": {"neden": "$output.why"}}}]});
    let mut r = Run::child(&[("altyapı", altyapı)]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!({"ağ": "n-1"}));
    let a = r.failed(&a.jobs[0], "exit 1: disk yok");
    // its undo first, then its error list
    assert_eq!(names(&a), vec!["ağ-sil"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["rapor"]);
    assert_eq!(a.jobs[0].data["neden"], "exit 1: disk yok");
    r.done(&a.jobs[0], json!({"bildirildi": "ekip"}));
    assert!(matches!(r.rt.ended, Some(Ended::Error { .. })));
    // rule 2: the error list's result, and its steps
    let back = steps::returned(&r.rt);
    assert_eq!(back["output"], json!({"bildirildi": "ekip"}));
    assert_eq!(back["status"], "error");
    assert_eq!(back["steps"]["ağ-kur"], json!({"ağ": "n-1"}));

    // with no error list: its $output says why, its steps what they gave
    let mut r = Run::child(&[("altyapı", json!({"steps": ["#ağ-kur", "#disk-kur"]}))]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!({"ağ": "n-1"}));
    r.failed(&a.jobs[0], "exit 1: disk yok");
    let back = steps::returned(&r.rt);
    assert_eq!(back["status"], "error");
    assert_eq!(back["output"]["why"], "exit 1: disk yok");
    assert_eq!(back["output"]["failed_step"], "#disk-kur");
    assert_eq!(back["steps"], json!({"ağ-kur": {"ağ": "n-1"}}));
}

#[test]
fn a_child_that_failed_returns_its_failure_list_s_result_or_its_last_step_s() {
    let kontrol = |failure: Value| json!({"steps": ["#kontrol"], "failure": failure});
    let mut r = Run::child(&[("denetim", kontrol(json!(["#uyar"])))]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!({"success": false, "eksik": 3}));
    assert_eq!(names(&a), vec!["uyar"]);
    r.done(&a.jobs[0], json!({"uyarıldı": true}));
    assert_eq!(r.rt.ended, Some(Ended::Failed));
    assert_eq!(steps::returned(&r.rt)["output"], json!({"uyarıldı": true}));
    assert_eq!(steps::returned(&r.rt)["status"], "failed");
    let mut r = Run::child(&[("denetim", kontrol(json!([])))]);
    let a = r.start(json!({}));
    r.done(&a.jobs[0], json!({"success": false, "eksik": 3}));
    assert_eq!(
        steps::returned(&r.rt)["output"],
        json!({"success": false, "eksik": 3})
    );
}

#[test]
fn a_child_that_broke_is_no_error_for_its_parent() {
    let mut r = Run::new(&[
        (
            "kurulum",
            json!({"steps": [{"altyapı": {"undo": "#hepsini-sil"}}, "#dene"],
                   "error": ["#hata"]}),
        ),
        ("altyapı", json!({"steps": ["#ağ-kur"]})),
    ]);
    let a = r.start(json!({}));
    let a = r.ended(
        &a.children[0],
        json!({"output": {"why": "exit 1", "failed_step": "#ağ-kur", "undo_failed": []},
               "steps": {}, "status": "error"}),
    );
    // the parent goes on with what its child returned
    assert_eq!(names(&a), vec!["dene"]);
    assert_eq!(r.rt.state["output"]["why"], "exit 1");
    assert_eq!(r.rt.state["steps"]["altyapı"]["status"], "error");
    // the parent breaking later undoes its own step only: the child's
    // insides are the child's
    let a = r.failed(&a.jobs[0], "exit 1");
    assert_eq!(names(&a), vec!["hepsini-sil"]);
    let a = r.done(&a.jobs[0], json!({}));
    assert_eq!(names(&a), vec!["hata"]);
}

#[test]
fn a_run_started_by_a_step_cannot_split() {
    let mut r = Run::child(&[("çoklu", json!({"steps": ["#yaz"]}))]);
    r.repeat
        .insert("yaz".into(), json!([{"dosya": "bir"}, {"dosya": "iki"}]));
    let e = r.try_send("run.started", json!({})).unwrap_err();
    assert!(
        e.contains("#yaz") && e.contains("workflow used as a step"),
        "{e}"
    );
}

#[test]
fn a_workflow_side_by_side_is_a_run_beside_the_action() {
    let mut r = Run::new(&[
        (
            "sipariş",
            json!({"steps": [["#check-payment", "stok"], "#ship"]}),
        ),
        ("stok", json!({"steps": ["#depo-bak", "#ayır"]})),
    ]);
    let a = r.start(json!({"order": 43}));
    assert_eq!(names(&a), vec!["check-payment"]);
    assert_eq!(children(&a), vec![("stok".into(), json!({}))]);
    assert_eq!(a.children[0].path, vec![0, 0, 1]);
    let (pay, stok) = (a.jobs[0].clone(), a.children[0].clone());
    assert!(r.done(&pay, json!({"paid": true})).jobs.is_empty());
    let a = r.ended(
        &stok,
        json!({"output": {"stock": true}, "steps": {}, "status": "done"}),
    );
    assert_eq!(names(&a), vec!["ship"]);
    assert_eq!(r.rt.state["output"], json!({"paid": true, "stock": true}));
    assert_eq!(r.rt.state["steps"]["stok"]["status"], "done");
}

#[test]
fn a_child_that_returned_from_its_error_list_ended_well_for_its_group() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [["#çay", "demleme"], "#sonra"], "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su"], "error": ["#rapor"]})),
    ]);
    let a = r.start(json!({}));
    let (cay, demleme) = (a.jobs[0].clone(), a.children[0].clone());
    assert!(r.done(&cay, json!({"sıcak": 1})).jobs.is_empty());
    let a = r.ended(
        &demleme,
        json!({"output": {"rapor": "su yok"}, "steps": {}, "status": "error"}),
    );
    assert_eq!(names(&a), vec!["sonra"], "not broken");
    assert_eq!(r.rt.state["output"], json!({"sıcak": 1, "rapor": "su yok"}));
}

#[test]
fn a_child_stopped_by_its_group_is_named_and_its_path_given() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"race": ["#çay", "demleme"]}, "#sonra"]}),
        ),
        ("demleme", json!({"steps": ["#su"]})),
    ]);
    let a = r.start(json!({}));
    let (cay, demleme) = (a.jobs[0].clone(), a.children[0].clone());
    assert_eq!(demleme.path, vec![0, 0, 1]);
    let a = r.done(&cay, json!({"içildi": true}));
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["demleme"]);
    // its path, for the daemon to stop its run
    assert_eq!(a.stopped, vec![demleme.path.clone()]);
    // what it returns now reaches nothing
    assert!(
        r.ended(
            &demleme,
            json!({"output": {}, "steps": {}, "status": "done"})
        )
        .jobs
        .is_empty()
    );
    // a group that ends in the decision that would start it starts no run
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"race": ["demleme", {"#çay": {"args": "$event.yok"}}]}, "#sonra"],
                   "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su"]})),
    ]);
    let a = r.start(json!({}));
    assert!(a.children.is_empty(), "{:?}", a.children);
    assert_eq!(names(&a), vec!["hata"]);
}

#[test]
fn a_group_that_broke_lets_its_running_child_run_to_its_end() {
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [["#çay", "#şeker", "demleme"], "#sonra"], "error": ["#hata"]}),
        ),
        ("demleme", json!({"steps": ["#su"]})),
    ]);
    let a = r.start(json!({}));
    let (cay, seker, demleme) = (a.jobs[0].clone(), a.jobs[1].clone(), a.children[0].clone());
    r.waiting(&cay, json!({"sipariş": 7}));
    let a = r.failed(&seker, "exit 1: şeker yok");
    // the waiting step waits no more; the child runs on
    assert_eq!(stopped(&a), vec!["#çay"]);
    assert_eq!(a.stopped, vec![cay.path.clone()]);
    assert_eq!(r.rt.ended, None);
    // it ends: the group breaks
    let a = r.ended(
        &demleme,
        json!({"output": {"sıcak": true}, "steps": {}, "status": "done"}),
    );
    assert!(breaks(&a), "{:?}", names(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#şeker"));
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
            json!({"on": [{ "has": "$event.paid" }], "steps": [
                {"#ship":           {"on": [{"$event.paid": true,  "$event.stock": true}]}},
                {"#remind-payment": {"on": [{"$event.paid": false, "$event.stock": true}]}}
            ]}),
        ),
    ]);
    let a = r.start(json!({"order": 43}));
    let a = r.done(&a.jobs[0], json!({"paid": false, "stock": true}));
    // işle's on holds over its $event, the args it is given
    assert_eq!(
        children(&a),
        vec![("işle".into(), json!({"paid": false, "stock": true}))]
    );
    let mut işle = Run::child(&[("işle", r.flows["işle"].clone())]);
    let b = işle.start(json!({"paid": false, "stock": true}));
    assert_eq!(names(&b), vec!["remind-payment"]);
    // $$API is not defined: #bildir is skipped
    let a = r.ended(
        &a.children[0],
        json!({"output": {}, "steps": {}, "status": "done"}),
    );
    assert_eq!(names(&a), vec!["bitir"]);

    // a workflow whose on does not hold is skipped as a whole
    let mut r = Run::new(&[
        ("sipariş", json!({"steps": ["#check", "işle", "#bitir"]})),
        (
            "işle",
            json!({"on": [{ "has": "$event.paid" }], "steps": ["#ship"]}),
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
        a.jobs[0].event,
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#check-payment"));
    assert!(r.rt.why.as_deref().unwrap().contains("2h"));
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
    // the error list starts with why the run ended as its $output
    assert_eq!(
        r.rt.state["output"],
        json!({"why": "exit 3: bağlantı yok", "failed_step": "#check-payment", "undo_failed": []})
    );
    let log = a.jobs[0].clone();
    assert_eq!(r.rt.why.as_deref(), Some("exit 3: bağlantı yok"));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#check-payment"));
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
    assert_eq!(r.rt.why.as_deref(), Some("exit 1: yanıt yok"));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#dene"));
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
        json!(r.rt.undo_failed),
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#dene"));
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
fn repeat_runs_the_rest_once_per_item() {
    let flow =
        json!({"steps": [{"#imap-check": {"args": {"host": "imap.örnek.test"}}}, "#notify"]});
    let mut r = Run::new(&[("posta", flow.clone())]);
    r.repeat.insert(
        "imap-check".into(),
        json!([{"user": "ayşe@örnek.test"}, {"user": "can@örnek.test"}]),
    );
    let a = r.start(json!({"saat": 9}));
    assert!(a.jobs.is_empty());
    assert_eq!(a.split.len(), 2);
    assert_eq!(r.rt.ended, Some(Ended::Split));
    for (child, user) in a.split.iter().zip(["ayşe@örnek.test", "can@örnek.test"]) {
        let mut c = Run::new(&[("posta", flow.clone())]);
        c.repeat = r.repeat.clone();
        c.rt = child.clone();
        let a = c.start(Value::Null);
        assert_eq!(names(&a), vec!["imap-check"]);
        assert_eq!(a.jobs[0].item, Some(json!({"user": user})));
        assert_eq!(a.jobs[0].data["host"], "imap.örnek.test");
        assert_eq!(c.rt.state["event"]["saat"], 9);
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#iki"));
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
            event: None,
        },
        "exit 1",
    );
    assert_eq!(names(&a), vec!["küme-sil"]);
}

#[test]
fn a_secret_held_by_an_event_value_breaks_a_step_that_puts_it_in_text() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            "#gir",
            {"#bildir": {"args": {"text": "giriş: $event.parola"}}}
        ], "error": ["#uyar"]}),
    )]);
    let a = r.start(json!({"parola": "$$POSTA"}));
    let a = r.done(&a.jobs[0], json!(""));
    assert_eq!(names(&a), vec!["uyar"]);
    assert_eq!(r.rt.failed_step.as_deref(), Some("#bildir"));
    let why = r.rt.why.clone().unwrap();
    assert!(why.contains("a secret is a whole value only"), "{why}");
    // the error list starts with why
    assert_eq!(r.rt.state["output"]["failed_step"], "#bildir");
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
            assert_eq!(r.rt.failed_step.as_deref(), Some(AD[i]));
            assert_eq!(r.rt.why.as_deref(), Some("exit 3: bağlantı yok"));
            assert_eq!(r.rt.state["output"]["failed_step"], AD[i], "{group} {i}");
            assert!(a.stopped.is_empty() && stopped(&a).is_empty(), "{i}");
        }
    }
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#çay"));
}

#[test]
fn race_ends_with_the_first_step_to_end_and_names_the_winner() {
    for i in 0..3 {
        let (mut r, j) = grup(json!({"race": AD}));
        let a = r.done(&j[i], basar(i));
        assert!(goes_on(&a), "{i}");
        assert_eq!(
            r.rt.state["output"],
            json!({ ALAN[i]: i, "winner": AD[i].trim_start_matches('#') })
        );
        let others: Vec<&StepJob> = (0..3).filter(|k| *k != i).map(|k| &j[k]).collect();
        assert_eq!(sorted(&a), paths(&others));
        assert_eq!(stopped(&a).len(), 2);
        // the others' answers come too late
        assert!(r.done(others[0], json!({"geç": 9})).jobs.is_empty());
        assert!(r.failed(others[1], "exit 1").jobs.is_empty());
        assert_eq!(
            r.rt.state["output"],
            json!({ ALAN[i]: i, "winner": AD[i].trim_start_matches('#') })
        );
    }
}

#[test]
fn race_breaks_when_the_first_to_end_broke() {
    let (mut r, j) = grup(json!({"race": AD}));
    let a = r.failed(&j[2], "exit 2: yok");
    assert!(breaks(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#ılık"));
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
        json!({"tatlı": 1, "winner": "şeker", "failed": ["çay"], "why": {"çay": "exit 1"}})
    );
    assert_eq!(stopped(&a), vec!["#ılık"]);
    assert_eq!(sorted(&a), vec![j[2].path.clone()]);
    // the same when the winner is first
    let (mut r, j) = grup(json!({"any": AD}));
    let a = r.done(&j[2], json!(7));
    assert!(goes_on(&a));
    assert_eq!(r.rt.state["output"], json!({"ılık": 7, "winner": "ılık"}));
    assert_eq!(stopped(&a), vec!["#çay", "#şeker"]);
}

#[test]
fn any_breaks_when_every_step_broke_with_the_first_by_time() {
    let (mut r, j) = grup(json!({"any": AD}));
    assert!(r.failed(&j[2], "exit 2: ılık").jobs.is_empty());
    assert!(r.failed(&j[0], "exit 1: çay").jobs.is_empty());
    let a = r.failed(&j[1], "exit 3: şeker");
    assert!(breaks(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#ılık"));
    assert_eq!(r.rt.why.as_deref(), Some("exit 2: ılık"));
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
        json!({"sıcak": 0, "ılık": 2, "failed": ["şeker"], "why": {"şeker": "exit 1"}})
    );
    assert!(a.stopped.is_empty());
}

#[test]
fn some_with_a_count_breaks_when_the_count_cannot_be_reached() {
    let (mut r, j) = grup(json!({"some": AD, "count": 2}));
    assert!(r.failed(&j[1], "exit 1: şeker").jobs.is_empty());
    let a = r.failed(&j[0], "exit 2: çay");
    assert!(breaks(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#şeker"));
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#ılık"));
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
        json!({"sıcak": 0, "failed": ["şeker", "ılık"],
               "why": {"şeker": "exit 1: şeker yok", "ılık": "exit 2: ılık yok"}})
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
    assert_eq!(r.rt.state["output"], json!({"winner": "çay", "n": 1}));
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
        json!({"tatlı": 1, "ılık": 2, "failed": ["çay"], "why": {"çay": "exit 1: çay"}})
    );
    // a race whose first step broke ends, and stops the others
    let (mut r, j) = grup(json!({"race": AD, "fail": "never"}));
    let a = r.failed(&j[1], "exit 1: şeker");
    assert!(goes_on(&a));
    assert_eq!(stopped(&a), vec!["#çay", "#ılık"]);
    assert_eq!(
        r.rt.state["output"],
        json!({"failed": ["şeker"], "why": {"şeker": "exit 1: şeker"}})
    );
}

#[test]
fn fail_any_breaks_the_group_at_the_first_break() {
    let (mut r, j) = grup(json!({"any": AD, "fail": "any"}));
    let a = r.failed(&j[0], "exit 1: çay");
    assert!(breaks(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#çay"));
    assert_eq!(stopped(&a), vec!["#ılık", "#şeker"]);
    // settled waits for its steps all the same: #şeker runs to its end
    let (mut r, j) = grup(json!({"settled": AD, "fail": "any"}));
    r.done(&j[0], basar(0));
    let a = r.failed(&j[2], "exit 1: ılık");
    assert!(a.jobs.is_empty() && a.stopped.is_empty());
    let a = r.done(&j[1], basar(1));
    assert!(breaks(&a));
    assert_eq!(r.rt.failed_step.as_deref(), Some("#ılık"));
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#çay"));
    // one ended well: the count cannot be reached, nothing breaks, and the
    // group ends with the last
    let (mut r, j) = grup(json!({"some": AD, "count": 2, "fail": "all"}));
    r.failed(&j[0], "exit 1");
    r.failed(&j[1], "exit 2");
    let a = r.done(&j[2], basar(2));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"ılık": 2, "failed": ["çay", "şeker"],
               "why": {"çay": "exit 1", "şeker": "exit 2"}})
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
    assert_eq!(r.rt.failed_step.as_deref(), Some("#ılık"));
    assert_eq!(r.rt.why.as_deref(), Some("exit 2: ılık"));
    // and when it did not break, the group ends with the others
    let (mut r, j) = grup(json!({"all": AD, "fail": "#ılık"}));
    r.failed(&j[0], "exit 1");
    r.done(&j[2], basar(2));
    let a = r.done(&j[1], basar(1));
    assert!(goes_on(&a));
    assert_eq!(
        r.rt.state["output"],
        json!({"tatlı": 1, "ılık": 2, "failed": ["çay"], "why": {"çay": "exit 1"}})
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
        json!({"tatlı": true, "winner": "şeker"})
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
    assert_eq!(out["failed"], json!(["çay"]));
    assert!(out["why"]["çay"].as_str().unwrap().contains("2h"));
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
            json!({"on": [{"has": "$event.yok"}], "steps": ["#oku"]}),
        ),
        (
            "arşiv",
            json!({"on": [{"has": "$event.yok"}], "steps": ["#oku"]}),
        ),
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
            json!({"fiyat": 5, "winner": "canlı"}),
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
        json!({"failed": ["şeker"], "why": {"şeker": "exit 7: bağlantı yok"}})
    );
    // any, its winner after a break
    let (mut r, j) = grup(json!({"any": AD}));
    r.failed(&j[0], "exit 1: çay");
    assert!(goes_on(&r.done(&j[2], basar(2))));
    assert_eq!(
        r.rt.state["output"],
        json!({"ılık": 2, "winner": "ılık", "failed": ["çay"], "why": {"çay": "exit 1: çay"}})
    );
    // nothing broke: no failed, but for settled and fail never
    let (mut r, j) = grup(json!({"any": AD}));
    assert!(goes_on(&r.done(&j[0], basar(0))));
    assert_eq!(r.rt.state["output"], json!({"sıcak": 0, "winner": "çay"}));
}

#[test]
fn a_group_in_a_workflow_used_as_a_step_is_its_run_s_own() {
    let iç = json!({"steps": [{"any": ["#şeker", "#ılık"]}, "#karıştır"]});
    // the child's run decides its own group
    let mut c = Run::child(&[("iç", iç.clone())]);
    let j = c.start(json!({})).jobs;
    assert_eq!(
        names(&c.failed(&j[0], "exit 1: şeker")),
        Vec::<String>::new()
    );
    let a = c.done(&j[1], basar(2));
    assert_eq!(names(&a), vec!["karıştır"], "the any ended with #ılık");
    c.done(&a.jobs[0], json!({"karışık": true}));
    assert_eq!(c.rt.ended, Some(Ended::Done));
    // every step of it broke: the child broke, which its parent's group
    // reads as what it returned
    let mut c = Run::child(&[("iç", iç.clone())]);
    let j = c.start(json!({})).jobs;
    c.failed(&j[0], "exit 1: şeker");
    c.failed(&j[1], "exit 2: ılık");
    let back = steps::returned(&c.rt);
    assert_eq!(back["status"], "error");
    let mut r = Run::new(&[
        (
            "küme",
            json!({"steps": [{"all": ["#çay", "iç"]}, "#sonra"], "error": ["#hata"]}),
        ),
        ("iç", iç),
    ]);
    let a = r.start(json!({}));
    let (cay, child) = (a.jobs[0].clone(), a.children[0].clone());
    assert!(r.ended(&child, back).jobs.is_empty());
    assert_eq!(names(&r.done(&cay, basar(0))), vec!["sonra"]);
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
        event: None,
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
fn a_list_of_args_is_one_object_for_its_action_only() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"#sor": {"args": [{"url": "$event.adres", "adet": "$event.n"}, {"title": "etiket", "text": "ayna $event.n"}]}},
            {"#bildir": {"args": [{"fiyat": "$output"}]}},
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
    assert!(!r.rt.state.contains_key("etiket") && !r.rt.state.contains_key("adet"));
    let a = r.done(&a.jobs[0], json!(42.5));
    assert_eq!(a.jobs[0].data["fiyat"], 42.5);
    assert!(!r.rt.state.contains_key("fiyat"));
}

#[test]
fn one_action_twice_in_a_group_runs_with_each_step_s_args_and_is_read_by_its_number() {
    let mut r = Run::new(&[(
        "fiyat",
        json!({"steps": [
            {"any": [
                {"#ayna": {"args": [{"url": "$event.bir"}]}},
                {"#ayna": {"args": [{"url": "$event.iki"}]}}
            ]},
            "#sonra"
        ]}),
    )]);
    let a =
        r.start(json!({"bir": "http://127.0.0.1:9/fiyat", "iki": "http://127.0.0.1:8765/fiyat"}));
    assert_eq!(names(&a), vec!["ayna", "ayna"]);
    assert_eq!(a.jobs[0].data["url"], "http://127.0.0.1:9/fiyat");
    assert_eq!(a.jobs[1].data["url"], "http://127.0.0.1:8765/fiyat");
    // the first copy breaks, the second gives a plain value: it wins by its number
    assert!(r.failed(&a.jobs[0], "exit 7: bağlantı yok").jobs.is_empty());
    let a = r.done(&a.jobs[1], json!("42.50"));
    assert_eq!(names(&a), vec!["sonra"]);
    // the copy that broke is named by its number too
    assert_eq!(
        r.rt.state["output"],
        json!({"ayna/2": "42.50", "winner": "ayna/2",
               "failed": ["ayna/1"], "why": {"ayna/1": "exit 7: bağlantı yok"}})
    );
    // and read by it later
    assert_eq!(r.rt.state["steps"]["ayna/2"], json!("42.50"));
}

#[test]
fn a_step_of_a_group_whose_on_does_not_hold_is_out_of_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"race": [{"#önbellek": {"on": [{"$event.taze": true}]}}, "#canlı"]},
            "#sonra"
        ]}),
    )]);
    let a = r.start(json!({"taze": false}));
    assert_eq!(names(&a), vec!["canlı"]);
    let a = r.done(&a.jobs[0], json!({"fiyat": 5}));
    assert_eq!(names(&a), vec!["sonra"]);
    assert_eq!(r.rt.state["output"], json!({"fiyat": 5, "winner": "canlı"}));
}

#[test]
fn a_step_of_a_group_whose_args_cannot_be_read_breaks_in_it() {
    let mut r = Run::new(&[(
        "a",
        json!({"steps": [
            {"all": [{"#a": {"args": "$event.yok"}}, "#b"], "fail": "never"},
            "#sonra"
        ]}),
    )]);
    let a = r.start(json!({}));
    assert_eq!(names(&a), vec!["b"]);
    let a = r.done(&a.jobs[0], json!({"b": 1}));
    assert_eq!(names(&a), vec!["sonra"]);
    assert_eq!(r.rt.state["output"]["failed"], json!(["a"]));
}

#[test]
fn an_action_s_list_of_args_is_one_set_of_values_not_a_repeat() {
    let mut r = Run::new(&[(
        "posta",
        json!({"steps": [
            {"#imap-check": {"args": [{"host": "imap.örnek.test"}, {"user": "ayşe@örnek.test"}]}},
            "#notify"
        ]}),
    )]);
    let a = r.start(json!({}));
    assert!(a.split.is_empty());
    assert_eq!(names(&a), vec!["imap-check"]);
    assert_eq!(a.jobs[0].item, None);
    assert_eq!(
        Value::Object(a.jobs[0].data.clone()),
        json!({"host": "imap.örnek.test", "user": "ayşe@örnek.test"})
    );
}

#[test]
fn the_name_alone_reads_the_first_of_its_number() {
    let mut r = Run::new(&[(
        "bölge",
        json!({"steps": [
            "#ölç", "#ölç",
            {"#topla": {"args": {"ilk": "$steps.ölç", "ikinci": "$steps.ölç/2.değer"}}}
        ]}),
    )]);
    let a = r.start(json!({}));
    let a = r.done(&a.jobs[0], json!({"değer": 3}));
    let a = r.done(&a.jobs[0], json!({"değer": 4}));
    assert_eq!(names(&a), vec!["topla"]);
    assert_eq!(a.jobs[0].data["ilk"], json!({"değer": 3}));
    assert_eq!(a.jobs[0].data["ikinci"], json!(4));
}
