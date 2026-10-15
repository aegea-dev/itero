use itero::engine::manager::decide;
use itero::engine::{Answer, Event, Input, Runtime};
use itero::workflow::File;
use serde_json::{Map, Value, json};

/// A workflow that fetches an item from an API and remembers its name.
const EXAMPLE: &str = r#"{
  "name": "urun-bilgisi",
  "description": "Fetch an item from an API and remember it; ürün bilgisi.",
  "states": ["yeni", "hazir", "isleniyor", "bitti", "zaman-asimi"],
  "start": "yeni",
  "end": ["bitti", "zaman-asimi"],
  "transitions": [
    { "from": "yeni", "to": "hazir" },
    { "from": "hazir", "to": "isleniyor",
      "timeout": "5m", "on_timeout": "zaman-asimi",
      "do": [
        { "action": "http.request@1",
          "with": { "url": "https://api.example.com/items/${args.urun}",
                    "headers": { "Authorization": "Bearer ${env.API_KEY}" } },
          "expect": "output.status == 200 && has(output.json.data)" }
      ] },
    { "from": "isleniyor", "to": "bitti",
      "do": [
        { "action": "json.pick@1", "with": { "path": "json.data" } },
        { "action": "memory.set@1",
          "with": { "ad": "${prev.name}", "kaynak": "API'den alındı" } },
        { "action": "checks.is-valid@1", "with": { "ad": "${memory.ad}" },
          "expect": "output.ok" }
      ] }
  ]
}"#;

fn example() -> File {
    itero::workflow::parse(EXAMPLE).unwrap()
}

fn input(rt: Runtime, kind: &str, id: &str, data: Value) -> Input {
    Input {
        now: "2026-10-14T20:00:00Z".into(),
        workflow: example(),
        run: json!({"id": "r1", "workflow": "urun-bilgisi"})
            .as_object()
            .unwrap()
            .clone(),
        args: json!({"urun": "12"}).as_object().unwrap().clone(),
        memory: Map::new(),
        runtime: rt,
        event: Event {
            id: id.into(),
            kind: kind.into(),
            data,
        },
    }
}

fn at(state: &str) -> Runtime {
    Runtime {
        state: state.into(),
        chain: None,
        failed: None,
    }
}

/// A new run: through `hazir` by itself, to the API request.
fn requested() -> Answer {
    decide(&input(at("yeni"), "run.started", "e1", json!({}))).unwrap()
}

/// The API answered; the chain to `bitti` waits on the check.
fn checking() -> Answer {
    let a = requested();
    let data = json!({"chain": "e1.2", "index": 0, "output": {"status": 200, "json": {"data": {"name": "Ürün"}}}});
    decide(&input(a.runtime, "action.finished", "e2", data)).unwrap()
}

#[test]
fn a_started_run_moves_on_and_requests_the_api() {
    let a = requested();
    assert_eq!(a.runtime.state, "hazir");
    let chain = a.runtime.chain.as_ref().unwrap();
    assert_eq!(
        (chain.id.as_str(), chain.transition, chain.index),
        ("e1.2", 1, 0)
    );
    assert_eq!(a.events.len(), 1);
    let e = &a.events[0];
    assert_eq!(
        (e.kind.as_str(), e.target.as_str()),
        ("action.requested", "worker")
    );
    assert_eq!(e.data["action"], "http.request@1");
    assert_eq!(e.data["with"]["url"], "https://api.example.com/items/12");
    assert_eq!(
        e.data["with"]["headers"]["Authorization"], "Bearer ${env.API_KEY}",
        "secrets stay for the daemon"
    );
    assert_eq!(e.data["key"], "r1:e1.2:0");
    assert_eq!(e.data["run"]["id"], "r1");
    let timers = a.timers.as_ref().unwrap();
    assert_eq!(timers.len(), 1);
    assert_eq!(timers[0].at, "2026-10-14T20:05:00Z");
    assert_eq!(timers[0].data, json!({"chain": "e1.2", "transition": 1}));
    assert!(a.memory.is_empty());
}

#[test]
fn a_run_starts_once() {
    let a = requested();
    let b = decide(&input(a.runtime.clone(), "run.started", "e9", json!({}))).unwrap();
    assert_eq!(b.runtime, a.runtime);
    assert!(b.events.is_empty());
    assert!(b.log.iter().any(|l| l.contains("stale")), "{:?}", b.log);
}

#[test]
fn a_good_answer_runs_the_pure_actions_and_asks_for_the_check() {
    let a = checking();
    assert_eq!(a.runtime.state, "isleniyor");
    let chain = a.runtime.chain.as_ref().unwrap();
    assert_eq!(
        (chain.transition, chain.index),
        (2, 2),
        "json.pick and memory.set ran inline"
    );
    assert_eq!(chain.memory["ad"], "Ürün", "held back");
    assert!(a.memory.is_empty(), "nothing set before the chain ends");
    let e = &a.events[0];
    assert_eq!(e.data["action"], "checks.is-valid@1");
    assert_eq!(
        e.data["with"]["ad"], "Ürün",
        "later actions see held-back memory"
    );
    assert_eq!(
        a.timers.as_deref(),
        Some(&[][..]),
        "the old timer is cleared; this transition has none"
    );
}

#[test]
fn the_last_result_sets_the_memory_and_finishes() {
    let a = checking();
    let chain = a.runtime.chain.as_ref().unwrap().id.clone();
    let data = json!({"chain": chain, "index": 2, "output": {"ok": true}});
    let b = decide(&input(a.runtime, "action.finished", "e3", data)).unwrap();
    assert_eq!(b.runtime.state, "bitti");
    assert!(b.runtime.chain.is_none());
    assert_eq!(b.memory["ad"], "Ürün");
    assert_eq!(b.memory["kaynak"], "API'den alındı");
    assert!(b.events.is_empty());
}

#[test]
fn a_failed_check_fails_the_run_and_sets_nothing() {
    let a = requested();
    let data = json!({"chain": "e1.2", "index": 0, "output": {"status": 404, "json": {}}});
    let b = decide(&input(a.runtime, "action.finished", "e2", data)).unwrap();
    assert_eq!(b.runtime.state, "failed");
    let f = b.runtime.failed.as_ref().unwrap();
    assert_eq!(
        (f.from.as_str(), f.transition, f.chain.as_str()),
        ("hazir", 1, "e1.2")
    );
    let log = b.log.join("\n");
    assert!(
        log.contains("hazir -> isleniyor") && log.contains("http.request@1"),
        "{log}"
    );
    assert!(b.memory.is_empty());
    assert_eq!(b.timers.as_deref(), Some(&[][..]));
}

#[test]
fn a_failed_action_fails_the_run() {
    let a = requested();
    let data = json!({"chain": "e1.2", "index": 0, "error": "bağlantı kurulamadı"});
    let b = decide(&input(a.runtime, "action.failed", "e2", data)).unwrap();
    assert_eq!(b.runtime.state, "failed");
    assert!(b.log.join("\n").contains("bağlantı kurulamadı"));
}

#[test]
fn on_fail_goes_to_its_state_instead() {
    let a = requested();
    let mut i = input(
        a.runtime,
        "action.failed",
        "e2",
        json!({"chain": "e1.2", "index": 0, "error": "x"}),
    );
    i.workflow.transitions[1].on_fail = Some("zaman-asimi".into());
    let b = decide(&i).unwrap();
    assert_eq!(b.runtime.state, "zaman-asimi");
    assert!(b.runtime.failed.is_none());
    assert!(b.memory.is_empty());
}

#[test]
fn a_timeout_goes_to_on_timeout() {
    let a = requested();
    let b = decide(&input(
        a.runtime,
        "timer.fired",
        "e2",
        json!({"chain": "e1.2", "transition": 1}),
    ))
    .unwrap();
    assert_eq!(b.runtime.state, "zaman-asimi");
    assert!(b.runtime.chain.is_none());
}

#[test]
fn stale_results_and_timers_change_nothing() {
    let a = requested();
    for (kind, data) in [
        (
            "action.finished",
            json!({"chain": "eski", "index": 0, "output": {}}),
        ),
        (
            "action.finished",
            json!({"chain": "e1.2", "index": 5, "output": {}}),
        ),
        ("timer.fired", json!({"chain": "eski", "transition": 1})),
    ] {
        let b = decide(&input(a.runtime.clone(), kind, "e9", data)).unwrap();
        assert_eq!(b.runtime, a.runtime);
        assert!(b.timers.is_none() && b.events.is_empty() && b.memory.is_empty());
        assert!(b.log.iter().any(|l| l.contains("stale")), "{:?}", b.log);
    }
}

#[test]
fn resume_runs_the_failed_transition_again_with_the_same_keys() {
    let a = requested();
    let data = json!({"chain": "e1.2", "index": 0, "output": {"status": 500}});
    let failed = decide(&input(a.runtime, "action.finished", "e2", data)).unwrap();
    let b = decide(&input(failed.runtime, "run.resume", "e3", json!({}))).unwrap();
    assert_eq!(b.runtime.state, "hazir");
    assert_eq!(b.events[0].data["key"], "r1:e1.2:0");
    assert!(b.runtime.failed.is_none());
}

#[test]
fn events_that_match_nothing_are_logged() {
    let a = requested();
    let mut rt = a.runtime;
    rt.chain = None;
    let b = decide(&input(rt.clone(), "girdi.geldi", "e2", json!({}))).unwrap();
    assert_eq!(b.runtime, rt);
    assert!(
        b.log.iter().any(|l| l.contains("no transition")),
        "{:?}",
        b.log
    );
}

#[test]
fn when_chooses_among_transitions() {
    let mut i = input(at("yeni"), "run.started", "e1", json!({"öncelik": 1}));
    i.workflow.transitions.insert(
        0,
        serde_json::from_value(json!(
            { "from": "yeni", "to": "bitti", "when": "input['öncelik'] > 5" }
        ))
        .unwrap(),
    );
    assert_eq!(
        decide(&i).unwrap().runtime.state,
        "hazir",
        "1 is not > 5: the next one"
    );
    i.event.data = json!({"öncelik": 9});
    assert_eq!(decide(&i).unwrap().runtime.state, "bitti");
}

#[test]
fn an_endless_loop_fails_the_run() {
    let mut i = input(at("yeni"), "run.started", "e1", json!({}));
    i.workflow.transitions = serde_json::from_value(json!([
        { "from": "yeni", "to": "hazir", "do": [{ "action": "memory.set@1", "with": { "tur": "döngü" } }] },
        { "from": "hazir", "to": "yeni", "do": [{ "action": "memory.set@1", "with": { "tur": "döngü" } }] }
    ])).unwrap();
    let a = decide(&i).unwrap();
    assert_eq!(a.runtime.state, "failed");
    assert!(a.log.join("\n").contains("loop"));
    assert_eq!(
        a.memory["tur"], "döngü",
        "what finished before the loop stays"
    );
}

#[test]
fn emitted_events_wait_for_the_chain_to_succeed() {
    let mut i = input(at("yeni"), "run.started", "e1", json!({}));
    i.workflow.transitions = serde_json::from_value(json!([
        { "from": "yeni", "to": "bitti",
          "do": [ { "action": "event.emit@1", "with": { "type": "iş.bitti", "target": "r2", "data": { "id": "${run.id}" } } } ] }
    ])).unwrap();
    let a = decide(&i).unwrap();
    assert_eq!(a.runtime.state, "bitti");
    assert_eq!(a.events.len(), 1);
    assert_eq!(
        (a.events[0].kind.as_str(), a.events[0].target.as_str()),
        ("iş.bitti", "r2")
    );
    assert_eq!(a.events[0].data, json!({"id": "r1"}));
}

#[test]
fn a_waiting_state_gets_a_timer_for_each_timed_wait() {
    let mut i = input(at("yeni"), "run.started", "e1", json!({}));
    i.workflow.transitions = serde_json::from_value(json!([
        { "from": "yeni", "to": "hazir" },
        { "from": "hazir", "on": "girdi.geldi", "to": "bitti", "timeout": "2d", "on_timeout": "zaman-asimi" }
    ])).unwrap();
    let a = decide(&i).unwrap();
    assert_eq!(a.runtime.state, "hazir");
    let t = &a.timers.as_ref().unwrap()[0];
    assert_eq!(t.at, "2026-10-16T20:00:00Z");
    assert_eq!(t.data, json!({"state": "hazir", "transition": 1}));
    let b = decide(&input(a.runtime, "timer.fired", "e2", t.data.clone())).unwrap();
    assert_eq!(b.runtime.state, "zaman-asimi");
}

#[test]
fn the_same_input_gives_the_same_answer() {
    let i = input(at("yeni"), "run.started", "e1", json!({}));
    assert_eq!(decide(&i).unwrap(), decide(&i).unwrap());
}

#[test]
fn bad_input_is_an_error() {
    let mut i = input(at("yeni"), "run.started", "e1", json!({}));
    i.now = "dün".into();
    assert!(decide(&i).unwrap_err().contains("now"));
}

/// A run of `flow` (workflow JSON) at its start, given one event.
fn flow_input(flow: &str, kind: &str, data: Value) -> Input {
    let workflow = itero::workflow::parse(flow).unwrap();
    let mut i = input(at(&workflow.start), kind, "e1", data);
    i.workflow = workflow;
    i
}

/// The result of the job the answer `a` asked for.
fn finished(i: &Input, a: &Answer, output: Value) -> Input {
    let req = &a.events[0].data;
    let data = json!({"chain": req["chain"], "index": req["index"], "output": output});
    let mut next = i.clone();
    next.runtime = a.runtime.clone();
    next.event = Event {
        id: "e2".into(),
        kind: "action.finished".into(),
        data,
    };
    next
}

#[test]
fn a_named_steps_output_is_seen_later_and_when_skips_an_action() {
    // basla -> bitti: a worker job (id kontrol), then two pure actions: the
    // first runs only when kontrol found something, the second always
    let flow = r#"{ "name": "akis", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti", "do": [
        { "id": "kontrol", "action": "command@1", "with": { "run": ["true"] } },
        { "action": "memory.set@1", "when": "steps.kontrol.json.yeni > 0",
          "with": { "uyari": "${steps.kontrol.json['özet']}" } },
        { "action": "memory.set@1", "with": { "son": "${steps.kontrol.json.son}" } }
      ] } ] }"#;
    let i = flow_input(flow, "run.started", json!({}));
    let a = decide(&i).unwrap();
    assert_eq!(a.events[0].kind, "action.requested");
    // yeni = 0: the first memory.set is skipped
    let out = json!({"exit": 0, "json": {"yeni": 0, "özet": "", "son": "m-7"}});
    let b = decide(&finished(&i, &a, out)).unwrap();
    assert_eq!(b.runtime.state, "bitti");
    assert_eq!(b.memory, *json!({"son": "m-7"}).as_object().unwrap());
    // yeni = 2: both run
    let out = json!({"exit": 0, "json": {"yeni": 2, "özet": "2 yeni ileti", "son": "m-9"}});
    let b = decide(&finished(&i, &a, out)).unwrap();
    assert_eq!(b.memory["uyari"], "2 yeni ileti");
    assert_eq!(b.memory["son"], "m-9");
}

#[test]
fn a_skipped_action_passes_prev_through() {
    let flow = r#"{ "name": "akis", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti", "do": [
        { "action": "json.pick@1", "with": { "path": "a" } },
        { "action": "json.pick@1", "when": "false", "with": { "path": "yok" } },
        { "action": "memory.set@1", "with": { "deger": "${prev}" } }
      ] } ] }"#;
    let a = decide(&flow_input(flow, "run.started", json!({"a": "çay"}))).unwrap();
    assert_eq!(a.runtime.state, "bitti");
    assert_eq!(a.memory["deger"], "çay");
}

#[test]
fn steps_last_across_jobs_in_the_runtime() {
    let flow = r#"{ "name": "akis", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti", "do": [
        { "id": "ilk", "action": "json.pick@1", "with": { "path": "a" } },
        { "action": "command@1", "with": { "run": ["true"] } },
        { "action": "memory.set@1", "with": { "ilk": "${steps.ilk}" } }
      ] } ] }"#;
    let i = flow_input(flow, "run.started", json!({"a": "gül"}));
    let a = decide(&i).unwrap();
    assert_eq!(a.runtime.chain.as_ref().unwrap().steps["ilk"], "gül");
    let b = decide(&finished(&i, &a, json!({"exit": 0}))).unwrap();
    assert_eq!(b.memory["ilk"], "gül");
}

#[test]
fn an_id_used_twice_in_a_workflow_is_refused() {
    let flow = r#"{ "name": "akis", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
      "transitions": [ { "from": "basla", "to": "bitti", "do": [
        { "id": "a", "action": "json.pick@1", "with": { "path": "x" } },
        { "id": "a", "action": "json.pick@1", "with": { "path": "y" } } ] } ] }"#;
    let e = itero::workflow::parse(flow).unwrap_err().to_string();
    assert!(e.contains("id \"a\" is used twice"), "{e}");
}

#[test]
fn a_bad_id_or_when_is_refused() {
    let with = |extra: &str| {
        format!(
            r#"{{ "name": "akis", "states": ["basla", "bitti"], "start": "basla", "end": ["bitti"],
          "transitions": [ {{ "from": "basla", "to": "bitti", "do": [
            {{ {extra} "action": "json.pick@1", "with": {{ "path": "x" }} }} ] }} ] }}"#
        )
    };
    let e = itero::workflow::parse(&with(r#""id": "Büyük","#))
        .unwrap_err()
        .to_string();
    assert!(e.contains("id \"Büyük\""), "{e}");
    let e = itero::workflow::parse(&with(r#""when": "((","#))
        .unwrap_err()
        .to_string();
    assert!(e.contains("bad expression"), "{e}");
}
