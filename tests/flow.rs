//! A workflow file in the simple form: `on`, `steps`, and the lists run at
//! the end.
use itero::flow::{self, Fail, Group, Member, Name, Need, Step, Wait};
use serde_json::{Map, Value, json};

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn action(name: &str) -> Name {
    Name::Action {
        name: name.into(),
        version: None,
    }
}

/// A value filled from `state`, when nothing in it is refused.
fn fill(v: Value, state: &Map<String, Value>) -> Value {
    flow::fill(&v, state).unwrap()
}

fn err(text: &str) -> String {
    flow::parse(text).unwrap_err()
}

#[test]
fn on_entries_say_what_they_are() {
    let f = flow::parse(
        r##"{ "on": [
              { "event": "sipariş.alındı" },
              { "schedule": "0 9 * * *" },
              { "path": "/var/mail/$$USER" },
              { "$event.paid": true, "$event.stock": 0 },
              { "has": "$event.sipariş" },
              { "has": ["$event.tutar", "$$API"] }
            ],
            "steps": ["#onayla"] }"##,
    )
    .unwrap();
    assert_eq!(f.on.events, vec![flow::Listen::of("sipariş.alındı")]);
    assert_eq!(f.on.schedules, vec!["0 9 * * *"]);
    assert_eq!(f.on.paths, vec!["/var/mail/$$USER"]);
    assert_eq!(
        f.on.equal,
        obj(json!({"event.paid": true, "event.stock": 0}))
    );
    assert_eq!(f.on.has, vec!["$event.sipariş", "$event.tutar", "$$API"]);
    assert!(flow::starts(&f.on));
    let g = flow::parse(r##"{ "on": [{ "has": "$event.order" }], "steps": ["#ship"] }"##).unwrap();
    assert!(!flow::starts(&g.on), "conditions alone start nothing");
}

#[test]
fn on_windows_say_which_events_start_a_run() {
    use flow::{Gate, Window};
    use std::time::Duration;
    let f = flow::parse(
        r##"{ "on": [
              { "event": "sayfa.değişti" },
              { "unique": "$event.adres", "for": "1d" },
              { "changed": "$event.durum", "per": "$event.site" },
              { "at_most": "1h", "per": "$event.site" },
              { "unique": "$event.kimlik" }
            ],
            "steps": ["#bildir"] }"##,
    )
    .unwrap();
    assert_eq!(
        f.on.windows,
        vec![
            Window {
                gate: Gate::Unique {
                    value: "event.adres".into(),
                    within: Some(Duration::from_secs(86_400))
                },
                per: None
            },
            Window {
                gate: Gate::Changed {
                    value: "event.durum".into()
                },
                per: Some("event.site".into())
            },
            Window {
                gate: Gate::AtMost {
                    every: Duration::from_secs(3_600)
                },
                per: Some("event.site".into())
            },
            Window {
                gate: Gate::Unique {
                    value: "event.kimlik".into(),
                    within: None
                },
                per: None
            },
        ]
    );
}

#[test]
fn every_wrong_window_is_told_at_once() {
    let e = err(r##"{ "on": [
          { "event": "sayfa.değişti" },
          { "unique": "event.adres" },
          { "at_most": "bir saat" },
          { "for": "1d" },
          { "per": "$event.site" },
          { "changed": "$event.durum", "per": 3 },
          { "unique": "$event.x", "changed": "$event.y" },
          { "at_most": "1h", "for": "1d" },
          { "unique": "$$PAROLA" },
          { "at_most": "0s" }
        ],
        "steps": ["#bildir"] }"##);
    for want in [
        "on: unique \"event.adres\" is not a value: write \"$event.adres\" (on/2)",
        "on: at_most: a duration is like 30m, 2h or 1d, not \"bir saat\" (on/3)",
        "on: for goes with unique: {\"unique\": \"$event.id\", \"for\": \"1d\"} (on/4)",
        "on: per goes with unique, changed, at_most, batch or at_least (on/5)",
        "on: per 3 is not a value, like \"$event.site\" (on/6)",
        "says more than one thing: unique, changed, at_most, batch and at_least are entries of their own (on/7)",
        "on: for goes with unique, not at_most (on/8)",
        "on: unique \"$$PAROLA\" is a secret's name, not a value of the event (on/9)",
        "on: at_most is longer than 0s (on/10)",
    ] {
        assert!(e.contains(want), "missing: {want}\nin:\n{e}");
    }
    let e = err(r##"{ "on": [{ "event": "sayfa.değişti" }],
        "steps": [{ "#bildir": { "on": [{ "at_most": "1h" }] } }] }"##);
    assert!(
        e.contains("a step's on holds conditions, not a window (unique, changed, at_most, batch, at_least): a window goes in the workflow's on"),
        "{e}"
    );
}

#[test]
fn an_event_entry_may_give_its_connector_args() {
    let f = flow::parse(
        r##"{ "on": [
              { "event": "posta.geldi", "args": { "KUTU": "satış", "PAROLA": "$$POSTA_PAROLA" } },
              { "event": "posta.geldi", "args": { "PAROLA": "$$POSTA_PAROLA", "KUTU": "destek" } },
              { "event": "sipariş.alındı" }
            ],
            "steps": ["#oku"] }"##,
    )
    .unwrap();
    assert_eq!(f.on.events.len(), 3);
    assert_eq!(f.on.events[0].kind, "posta.geldi");
    assert_eq!(f.on.events[0].args["KUTU"], "satış");
    assert_eq!(f.on.events[2].args, Map::new());
    // the args as written, keys sorted: what tells two copies apart
    assert_eq!(
        f.on.events[1].key(),
        r#"{"KUTU":"destek","PAROLA":"$$POSTA_PAROLA"}"#
    );
    assert_eq!(f.on.events[2].key(), "");
}

#[test]
fn a_connector_s_args_are_checked_each_problem_told() {
    let v = json!({ "on": [
        { "schedule": "0 9 * * *", "args": { "A": 1 } },
        { "event": "posta.geldi", "args": { "kutu adı": "satış", "N": "$output.n", "M": "$$TAMAM" } },
        { "event": "posta.geldi", "args": "satış" },
        { "event": "posta.geldi", "argz": {} }
    ], "steps": ["#oku"] });
    let ps = flow::read(&v).unwrap_err();
    let told: Vec<(&str, &str)> = ps
        .iter()
        .map(|p| (p.at.as_str(), p.message.as_str()))
        .collect();
    assert_eq!(told.len(), 5, "{told:#?}");
    assert_eq!(told[0].0, "on/1");
    assert!(told[0].1.contains("args go with an event"), "{told:#?}");
    assert_eq!((told[1].0, told[2].0), ("on/2", "on/2"));
    assert!(
        told[1].1.contains("\"kutu adı\"") && told[1].1.contains("environment variable"),
        "{told:#?}"
    );
    assert!(
        told[2].1.contains("\"N\"") && told[2].1.contains("no run"),
        "{told:#?}"
    );
    assert_eq!(told[3].0, "on/3");
    assert!(told[3].1.contains("args is an object"), "{told:#?}");
    assert_eq!(told[4].0, "on/4");
}

#[test]
fn repeat_reads_on_a_step_and_on_a_workflow() {
    use flow::Repeat;
    let at = |v: Value| -> Option<Repeat> {
        let f = flow::read(&json!({ "steps": [{ "#yaz": { "repeat": v } }] })).unwrap();
        let Step::One { repeat, .. } = &f.steps[0] else {
            panic!()
        };
        repeat.clone()
    };
    assert_eq!(at(json!(true)), Some(Repeat::Always));
    assert_eq!(at(json!(3)), Some(Repeat::Times(3)));
    assert_eq!(
        at(json!("$output.count")),
        Some(Repeat::TimesOf("output.count".into()))
    );
    assert_eq!(
        at(json!("<= 2026-12-31")),
        Some(Repeat::Until("2027-01-01T00:00:00Z".parse().unwrap()))
    );
    assert_eq!(
        at(json!("> 2026-12-31")),
        Some(Repeat::After("2027-01-01T00:00:00Z".parse().unwrap()))
    );
    assert_eq!(
        at(json!("0 9 * * *")),
        Some(Repeat::Cron("0 9 * * *".into()))
    );
    let f = flow::read(&json!({ "repeat": 2, "steps": ["#yaz"] })).unwrap();
    assert_eq!(f.repeat, Some(Repeat::Times(2)));
    // $count is a value now
    let f =
        flow::read(&json!({ "steps": [{ "#yaz": { "args": { "n": "$count" }, "repeat": 2 } }] }));
    assert!(f.is_ok(), "{f:?}");
}

#[test]
fn a_repeat_that_cannot_be_read_says_why_each_at_its_step() {
    let v = json!({ "repeat": "0 9 * * *", "steps": [
        { "#a": { "repeat": 0 } },
        { "#b": { "repeat": false } },
        { "#c": { "repeat": "her gün" } },
        { "#d": { "repeat": "$yok" } },
        [{ "#e": { "repeat": 2 } }, "#f"]
    ] });
    let ps = flow::read(&v).unwrap_err();
    let told: Vec<(&str, &str)> = ps
        .iter()
        .map(|p| (p.at.as_str(), p.message.as_str()))
        .collect();
    assert_eq!(told.len(), 6, "{told:#?}");
    assert_eq!(told[0].0, "repeat");
    assert!(told[0].1.contains("schedule is in on"), "{told:#?}");
    assert_eq!(told[1].0, "steps/#a");
    assert_eq!(told[2].0, "steps/#b");
    assert_eq!(told[3].0, "steps/#c");
    assert!(told[3].1.contains("repeat"), "{told:#?}");
    assert_eq!(told[4].0, "steps/#d");
    assert_eq!(told[5].0, "steps/all/#e");
    assert!(told[5].1.contains("side by side"), "{told:#?}");
}

#[test]
fn conditions_hold_on_exact_values_present_names_and_defined_secrets() {
    let f = flow::parse(
        r##"{ "on": [{ "has": ["$event.order", "$$API"] }],
              "steps": [{ "#ship": { "on": [{ "$event.paid": true, "$event.stock": 0 }] } }] }"##,
    )
    .unwrap();
    let defined = |n: &str| n == "API";
    let none = |_: &str| false;
    assert!(flow::holds(
        &f.on,
        &obj(json!({"event": {"order": 43}})),
        &defined
    ));
    assert!(!flow::holds(
        &f.on,
        &obj(json!({"event": {"order": 43}})),
        &none
    ));
    assert!(!flow::holds(
        &f.on,
        &obj(json!({"event": {"müşteri": "ayşe"}})),
        &defined
    ));
    let Step::One { on: Some(on), .. } = &f.steps[0] else {
        panic!("{:?}", f.steps[0]);
    };
    let ev = |v: Value| obj(json!({ "event": v }));
    assert!(flow::holds(
        on,
        &ev(json!({"paid": true, "stock": 0})),
        &none
    ));
    // exact: 0 is not false, true is not "true"
    assert!(!flow::holds(
        on,
        &ev(json!({"paid": true, "stock": false})),
        &none
    ));
    assert!(!flow::holds(
        on,
        &ev(json!({"paid": "true", "stock": 0})),
        &none
    ));
    assert!(!flow::holds(on, &ev(json!({"paid": true})), &none));
}

#[test]
fn steps_are_actions_workflows_side_by_side_lists_and_steps_with_data() {
    let f = flow::parse(
        r##"{
          "on": [{ "event": "order.placed" }],
          "steps": [
            ["#check-payment", "stock-check"],
            "order-process",
            "#notify@0.2",
            { "#ship": { "args": { "carrier": "PTT Kargo" }, "on": [{ "$event.paid": true }] } },
            { "#küme-kur": { "undo": "#küme-sil" } },
            { "altyapı": { "undo": "yıkım" } }
          ],
          "success": ["#log-result"],
          "failure": ["#log-result"],
          "error": ["#log-result", "#alert-team"],
          "timeout": "2h"
        }"##,
    )
    .unwrap();
    assert_eq!(
        f.steps[0],
        Step::Side(Group {
            members: vec![
                Member::bare(action("check-payment")),
                Member::bare(Name::Workflow("stock-check".into()))
            ],
            wait: Wait::All,
            fail: None,
        })
    );
    assert_eq!(
        f.steps[1],
        Step::One {
            name: Name::Workflow("order-process".into()),
            args: json!({}),
            on: None,
            undo: None,
            undo_args: json!({}),
            label: "order-process".into(),
            repeat: None,
            window: None,
        }
    );
    let Step::One { name, .. } = &f.steps[2] else {
        panic!()
    };
    assert_eq!(
        *name,
        Name::Action {
            name: "notify".into(),
            version: Some("0.2".into())
        }
    );
    let Step::One {
        name,
        args,
        on,
        undo,
        undo_args,
        label,
        ..
    } = &f.steps[3]
    else {
        panic!()
    };
    assert_eq!(*name, action("ship"));
    assert_eq!(*args, json!({"carrier": "PTT Kargo"}));
    assert_eq!(label, "#ship");
    assert_eq!(on.as_ref().unwrap().equal, obj(json!({"event.paid": true})));
    assert_eq!(*undo, None);
    assert_eq!(*undo_args, json!({}));
    let Step::One { args, undo, .. } = &f.steps[4] else {
        panic!()
    };
    assert_eq!(*args, json!({}));
    assert_eq!(*undo, Some(action("küme-sil")));
    let Step::One { undo, .. } = &f.steps[5] else {
        panic!()
    };
    assert_eq!(*undo, Some(Name::Workflow("yıkım".into())));
    assert_eq!(f.success.len(), 1);
    assert_eq!(f.failure.len(), 1);
    assert_eq!(f.error.len(), 2);
    assert_eq!(f.timeout, Some(std::time::Duration::from_secs(7200)));
}

#[test]
fn a_workflow_without_on_runs_only_as_a_step() {
    let f = flow::parse(r##"{ "steps": ["#ship"] }"##).unwrap();
    assert!(!flow::starts(&f.on));
    assert_eq!(f.timeout, None);
}

#[test]
fn mistakes_are_named() {
    for (text, says) in [
        (
            r##"{ "steps": ["#a"], "tetik": [] }"##,
            "unknown key \"tetik\"",
        ),
        (r##"{ "on": [{ "event": "a.b" }] }"##, "steps"),
        (
            r##"{ "on": [{ "event": "a.b" }], "steps": [] }"##,
            "steps is empty",
        ),
        // the old forms, now refused
        (r##"{ "on": ["a.b"], "steps": ["#a"] }"##, "an object"),
        (
            r##"{ "on": { "$event.a": 1 }, "steps": ["#a"] }"##,
            "a list",
        ),
        (
            r##"{ "on": [{ "event": "sipariş" }], "steps": ["#a"] }"##,
            "\"sipariş\"",
        ),
        (
            r##"{ "on": [{ "schedule": "61 * * * *" }], "steps": ["#a"] }"##,
            "61 * * * *",
        ),
        (
            r##"{ "on": [{ "event": "a.b", "$event.vip": true }], "steps": ["#a"] }"##,
            "one thing",
        ),
        (
            r##"{ "on": [{ "olay": "a.b" }], "steps": ["#a"] }"##,
            "\"olay\"",
        ),
        (r##"{ "on": [{ "$$API": 1 }], "steps": ["#a"] }"##, "$$API"),
        (r##"{ "on": [{ "has": 3 }], "steps": ["#a"] }"##, "has"),
        (
            r##"{ "on": [{ "has": ["ad"] }], "steps": ["#a"] }"##,
            "\"ad\"",
        ),
        (r##"{ "on": [{ "path": 7 }], "steps": ["#a"] }"##, "path"),
        (r##"{ "on": [{ "path": "" }], "steps": ["#a"] }"##, "path"),
        (
            r##"{ "on": [{ "event": "a.b" }], "steps": ["#a"], "timeout": "bir gün" }"##,
            "bir gün",
        ),
        (r##"{ "steps": [{ "#a": {}, "#b": {} }] }"##, "one name"),
        (r##"{ "steps": [{ "#a": "çay" }] }"##, "an object"),
        (
            r##"{ "steps": [["#a", 7]] }"##,
            "side by side is a name or an object",
        ),
        (r##"{ "steps": ["#"] }"##, "\"#\""),
        (r##"{ "steps": ["#a@x.1"] }"##, "version"),
        (r##"{ "steps": [{ "#a": { "undo": 3 } }] }"##, "undo"),
        (r##"{ "steps": [{ "#a": { "undo": "#" } }] }"##, "\"#\""),
        // a step's values go under args
        (
            r##"{ "steps": [{ "#a": { "carrier": "PTT Kargo" } }] }"##,
            "\"carrier\" is not one of args, on, undo, repeat, unique, changed, at_most:",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": 3 } }] }"##,
            "args is an object",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": "çay" } }] }"##,
            "args is an object",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": { "event": 1 } } }] }"##,
            "event is itero's",
        ),
        (
            r##"{ "steps": [{ "#a": { "strict": true } }] }"##,
            "strict is gone",
        ),
        // a value says where it comes from
        (
            r##"{ "on": [{ "has": "$order" }], "steps": ["#a"] }"##,
            "write $event.order",
        ),
        (
            r##"{ "on": [{ "$paid": true }], "steps": ["#a"] }"##,
            "write $event.paid",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": { "n": "sipariş $order" } } }] }"##,
            "$order is no value: write $event.order",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": { "n": "$output.#boyut" } } }] }"##,
            "write $steps.boyut",
        ),
        (
            r##"{ "steps": [{ "#a": { "args": { "n": "$why" } } }] }"##,
            "write $output.why",
        ),
        // every step's result by its name: one name once, and a name a step has
        (
            r##"{ "steps": ["#a", "#a", { "#b": { "args": { "n": "$steps.a/3" } } }] }"##,
            "$steps.a/3 names no step of this workflow",
        ),
        (
            r##"{ "steps": ["#a", { "#b": { "args": { "n": "$steps.c" } } }] }"##,
            "$steps.c names no step of this workflow (its steps: a, b)",
        ),
        (r##"[]"##, "an object"),
    ] {
        let e = err(text);
        assert!(e.contains(says), "{text}: {e}");
    }
}

#[test]
fn message_types_are_two_words_or_more() {
    for t in ["order.placed", "sipariş.ödendi.bugün", "mail.received"] {
        assert!(flow::is_message_type(t), "{t}");
    }
    for t in [
        "order",
        ".placed",
        "order.",
        "order..placed",
        "order placed",
        "$event.order",
    ] {
        assert!(!flow::is_message_type(t), "{t}");
    }
}

#[test]
fn an_entry_keeps_the_known_keys_and_the_persons_own() {
    let meta = flow::entry(&json!({
        "description": "Siparişler dükkândan",
        "from": "2026-11-01", "until": "2026-12-31",
        "internal": true,
        "sahibi": "ayşe", "etiketler": ["ödeme", "kargo"]
    }))
    .unwrap();
    assert_eq!(meta.description.as_deref(), Some("Siparişler dükkândan"));
    assert!(meta.internal && !meta.off);
    assert_eq!(meta.other["sahibi"], "ayşe");
    assert_eq!(meta.other["etiketler"], json!(["ödeme", "kargo"]));
    let at = |s: &str| s.parse().unwrap();
    // a date is a whole day: from its start, until its end
    assert!(!flow::active(&meta, at("2026-10-31T23:59:59Z")));
    assert!(flow::active(&meta, at("2026-11-01T00:00:00Z")));
    assert!(flow::active(&meta, at("2026-12-31T23:59:59Z")));
    assert!(!flow::active(&meta, at("2027-01-01T00:00:00Z")));
    let timed = flow::entry(&json!({ "until": "2026-12-31T18:00:00+03:00" })).unwrap();
    assert!(flow::active(&timed, at("2026-12-31T14:59:59Z")));
    assert!(!flow::active(&timed, at("2026-12-31T15:00:00Z")));
    let off = flow::entry(&json!({ "off": true })).unwrap();
    assert!(!flow::active(&off, at("2026-11-01T00:00:00Z")));
    let plain = flow::entry(&json!({})).unwrap();
    assert!(flow::active(&plain, at("2026-11-01T00:00:00Z")));
}

#[test]
fn entry_mistakes_are_named() {
    for (entry, says) in [
        (json!("açıklama"), "is an object"),
        (json!({ "id": "01a0" }), "id is given by itero"),
        (json!({ "name": "sipariş" }), "name is given by itero"),
        (
            json!({ "created": "2026-10-21" }),
            "created is given by itero",
        ),
        (json!({ "path": "/tmp" }), "path is given by itero"),
        (json!({ "version": "3" }), "version is given by itero"),
        (
            json!({ "from": "yarın" }),
            "from: a date (2026-11-01) or a time",
        ),
        (
            json!({ "until": 2026 }),
            "until: a date (2026-11-01) or a time",
        ),
        (json!({ "off": "evet" }), "off is true or false"),
        (json!({ "internal": 1 }), "internal is true or false"),
        (json!({ "description": ["ü"] }), "description is text"),
        (
            json!({ "settings": { "klasör": "~/Belgeler" } }),
            "settings are gone",
        ),
    ] {
        let e = flow::entry(&entry).unwrap_err();
        assert!(e.contains(says), "{entry}: {e}");
    }
}

#[test]
fn args_may_come_whole_from_a_value() {
    let f = flow::parse(
        r##"{ "steps": [{ "#a": { "args": "$output" } }, { "#b": { "args": "$output.sipariş" } }] }"##,
    )
    .unwrap();
    let Step::One { args, .. } = &f.steps[0] else {
        panic!()
    };
    assert_eq!(*args, json!("$output"));
    let Step::One { args, .. } = &f.steps[1] else {
        panic!()
    };
    assert_eq!(*args, json!("$output.sipariş"));
}

#[test]
fn a_value_reads_where_it_comes_from_and_its_fields() {
    let state = obj(json!({
        "event": {"adet": 5, "sipariş": {"id": "ş-43", "satır": {"ürün": "çay"}}},
        "output": {"sayı": 3, "klasör": "İndirilenler"},
        "steps": {"fiyat-sor": {"fiyat": 12}, "boyut": 1064},
        "config": {"description": "Dükkân"}
    }));
    assert_eq!(fill(json!("$event.adet"), &state), json!(5));
    assert_eq!(
        fill(json!("$output"), &state),
        json!({"sayı": 3, "klasör": "İndirilenler"})
    );
    assert_eq!(fill(json!("$output.sayı"), &state), json!(3));
    // a field an object does not have, or a name with no value, is null
    assert_eq!(fill(json!("$output.yok"), &state), Value::Null);
    assert_eq!(fill(json!("$event.yok"), &state), Value::Null);
    // a value that is no object has no fields: the dot is text
    assert_eq!(fill(json!("$event.adet.yok"), &state), json!("5.yok"));
    // fields at any depth; any step by its name; the manifest
    assert_eq!(
        fill(json!("$event.sipariş.satır.ürün"), &state),
        json!("çay")
    );
    assert_eq!(fill(json!("$steps.fiyat-sor.fiyat"), &state), json!(12));
    assert_eq!(fill(json!("$steps.boyut"), &state), json!(1064));
    assert_eq!(fill(json!("$steps.yok"), &state), Value::Null);
    assert_eq!(fill(json!("$config.description"), &state), json!("Dükkân"));
    assert_eq!(
        fill(
            json!("$event.sipariş.id: $steps.fiyat-sor.fiyat TL"),
            &state
        ),
        json!("ş-43: 12 TL")
    );
    // a secret stays as written, and anything else is itself
    assert_eq!(fill(json!("$$API"), &state), json!("$$API"));
    assert_eq!(fill(json!("çay"), &state), json!("çay"));
    assert_eq!(fill(json!(7), &state), json!(7));
}

#[test]
fn conditions_read_one_field_with_a_dot() {
    let f = flow::parse(
        r##"{ "steps": [{ "#ship": { "on": [{ "$output.ödendi": true }, { "has": "$output.cevap" }] } }] }"##,
    )
    .unwrap();
    let Step::One { on: Some(on), .. } = &f.steps[0] else {
        panic!()
    };
    let none = |_: &str| false;
    assert!(flow::holds(
        on,
        &obj(json!({"output": {"ödendi": true, "cevap": "Evet"}})),
        &none
    ));
    assert!(!flow::holds(
        on,
        &obj(json!({"output": {"ödendi": true}})),
        &none
    ));
    assert!(!flow::holds(on, &obj(json!({"output": true})), &none));
}

#[test]
fn a_word_after_dollar_that_is_no_value_is_named_and_left_as_text() {
    let state = obj(json!({"event": {"order": 7}}));
    // left in the text as written; itero check names what to write instead
    assert_eq!(
        fill(json!("sipariş $order"), &state),
        json!("sipariş $order")
    );
    for (old, instead) in [
        ("$order", "$event.order"),
        ("$order.id", "$event.order.id"),
        ("$output.#boyut", "$steps.boyut"),
        ("$why", "$output.why"),
        ("$failed_step", "$output.failed_step"),
    ] {
        let words = flow::unknown_in(&json!(format!("a {old} b")));
        assert_eq!(words.len(), 1, "{old}");
        assert_eq!(flow::instead(&words[0]), instead, "{old}");
    }
    assert!(flow::unknown_in(&json!("$event.order, $$API, \\$x, $5")).is_empty());
}

#[test]
fn names_are_filled_inside_text() {
    let state = obj(json!({
        "event": {
            "count": 3, "folder": "İndirilenler", "dosya": "rapor", "boş": "",
            "liste": [1, 2], "nesne": {"a": 1}
        },
        "output": {"sayı": 4, "hiç": null}
    }));
    for (text, filled) in [
        (
            "$event.count files in ~/$event.folder",
            "3 files in ~/İndirilenler",
        ),
        ("$event.count.", "3."),
        ("$event.count$event.count", "33"),
        // not there, null or "": empty
        ("x $event.yok y", "x  y"),
        ("[$event.boş]", "[]"),
        ("[$output.hiç]", "[]"),
        // a field when the object has it, else the dot is text
        ("$output.sayı files", "4 files"),
        ("$event.dosya.txt", "rapor.txt"),
        // a list or an object as its JSON
        ("$event.liste and $event.nesne", "[1,2] and {\"a\":1}"),
        // \$ is a dollar sign; a $ that starts no name is itself
        ("\\$event.count costs \\$5", "$event.count costs $5"),
        ("\\$$event.count", "$3"),
        ("a $ b $5 $", "a $ b $5 $"),
    ] {
        assert_eq!(fill(json!(text), &state), json!(filled), "{text}");
    }
    // a whole value keeps its type
    assert_eq!(fill(json!("$event.count"), &state), json!(3));
    assert_eq!(fill(json!("$event.yok"), &state), Value::Null);
    // texts nested in lists and objects are filled too
    assert_eq!(
        fill(
            json!({"buttons": ["Gönder $event.count", "$event.count"], "iç": {"t": "~/$event.folder"}}),
            &state
        ),
        json!({"buttons": ["Gönder 3", 3], "iç": {"t": "~/İndirilenler"}})
    );
}

#[test]
fn a_secret_is_never_filled_inside_text() {
    let state = obj(json!({"event": {"parola": "$$POSTA_PAROLA"}}));
    // a whole value stays as written, for the action alone
    assert_eq!(
        fill(json!("$$POSTA_PAROLA"), &state),
        json!("$$POSTA_PAROLA")
    );
    assert_eq!(
        fill(json!("$event.parola"), &state),
        json!("$$POSTA_PAROLA")
    );
    let e = flow::fill(&json!("giriş $$POSTA_PAROLA"), &state).unwrap_err();
    assert!(e.contains("a secret is a whole value only"), "{e}");
    // a value that holds one is data, only text
    assert_eq!(
        fill(json!("giriş $event.parola"), &state),
        json!("giriş $$POSTA_PAROLA")
    );
    // the args written as a secret, whichever form args take
    let written = flow::written_secrets;
    assert_eq!(
        written(&json!({"a": "$$X", "b": "$event.x", "c": "$$ ", "d": ["$$X"]})),
        ["a"]
    );
    assert_eq!(
        written(&json!([{"a": "$$X"}, {"title": "b", "text": "$event.x"}, {"c": "$$Y"}])),
        ["a", "c"]
    );
    assert!(written(&json!("$output")).is_empty());
    // a workflow that writes one inside text is refused when it is read
    let e = err(r##"{ "steps": [{ "#a": { "args": { "t": ["x $$API"] } } }] }"##);
    assert!(e.contains("a secret is a whole value only"), "{e}");
    // a workflow's $event is data: it is written in the step that needs it
    for text in [
        r##"{ "steps": [{ "küme": { "args": { "anahtar": "$$API" } } }] }"##,
        r##"{ "steps": [["#a", { "küme": { "args": { "anahtar": "$$API" } } }]] }"##,
        r##"{ "steps": [{ "#a": { "undo": { "küme": { "args": { "anahtar": "$$API" } } } } }] }"##,
    ] {
        let e = err(text);
        assert!(e.contains("a secret goes to an action"), "{text}: {e}");
    }
    assert!(
        flow::parse(
            r##"{ "steps": [{ "#a": { "undo": { "#b": { "args": { "k": "$$API" } } } } }] }"##
        )
        .is_ok()
    );
}

fn group(step: &str) -> Group {
    let f = flow::parse(&format!(r##"{{ "steps": [{step}] }}"##)).unwrap();
    let Step::Side(g) = &f.steps[0] else {
        panic!("{:?}", f.steps[0]);
    };
    g.clone()
}

/// The names of a group's steps, in order.
fn step_names(g: &Group) -> Vec<Name> {
    g.members.iter().map(|m| m.name.clone()).collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_bare_list_is_all() {
    let bare = group(r##"["#kontrol", "#tarama", "yedekle"]"##);
    assert_eq!(
        bare,
        group(r##"{ "all": ["#kontrol", "#tarama", "yedekle"] }"##)
    );
    assert_eq!(bare.wait, Wait::All);
    assert_eq!(bare.fail, None);
    assert_eq!(
        step_names(&bare),
        vec![
            action("kontrol"),
            action("tarama"),
            Name::Workflow("yedekle".into())
        ]
    );
}

#[test]
fn a_group_says_how_to_wait() {
    for (key, wait) in [
        ("all", Wait::All),
        ("race", Wait::Race),
        ("any", Wait::Any),
        ("settled", Wait::Settled),
    ] {
        let g = group(&format!(r##"{{ "{key}": ["#şu", "#bu"] }}"##));
        assert_eq!(g.wait, wait, "{key}");
        assert_eq!(g.fail, None, "{key}");
        assert_eq!(step_names(&g), vec![action("şu"), action("bu")], "{key}");
    }
    let g = group(r##"{ "some": ["#a", "#b", "#c"], "count": 2 }"##);
    assert_eq!(g.wait, Wait::Some(Need::Count(2)));
    assert_eq!(g.members.len(), 3);
    let g = group(r##"{ "some": ["#a", "#b", "#c"], "names": ["#a", "#c"] }"##);
    assert_eq!(g.wait, Wait::Some(Need::Names(names(&["#a", "#c"]))));
}

#[test]
fn fail_says_when_the_group_breaks() {
    for (value, fail) in [
        (r##""any""##, Fail::Any),
        (r##""all""##, Fail::All),
        (r##""never""##, Fail::Never),
        (r##""#şu""##, Fail::Steps(names(&["#şu"]))),
        (r##"["#şu", "bu"]"##, Fail::Steps(names(&["#şu", "bu"]))),
    ] {
        let g = group(&format!(
            r##"{{ "all": ["#şu", "bu", "#ğ"], "fail": {value} }}"##
        ));
        assert_eq!(g.wait, Wait::All, "{value}");
        assert_eq!(g.fail, Some(fail), "{value}");
    }
    let g = group(r##"{ "some": ["#a", "#b"], "count": 1, "fail": "never" }"##);
    assert_eq!(g.wait, Wait::Some(Need::Count(1)));
    assert_eq!(g.fail, Some(Fail::Never));
}

#[test]
fn a_workflow_may_be_named_like_a_key() {
    let f = flow::parse(r##"{ "steps": [{ "all": { "args": { "n": 1 } } }, { "fail": {} }] }"##)
        .unwrap();
    assert!(matches!(&f.steps[0], Step::One { name: Name::Workflow(w), .. } if w == "all"));
    assert!(matches!(&f.steps[1], Step::One { name: Name::Workflow(w), .. } if w == "fail"));
}

#[test]
fn a_group_in_the_error_list_and_the_use_of_its_names() {
    let f =
        flow::parse(r##"{ "steps": ["#a"], "error": [{ "settled": ["#günlük", "bildir"] }] }"##)
            .unwrap();
    assert!(f.uses().contains(&action("günlük")));
    assert!(f.uses().contains(&Name::Workflow("bildir".into())));
}

#[test]
fn a_group_mistake_is_named_with_the_step() {
    for (step, says) in [
        // no key, two keys, an unknown key
        (
            r##"{ "fail": "any" }"##,
            "one of all, race, any, some or settled",
        ),
        (
            r##"{ "all": ["#a", "#b"], "race": ["#a", "#b"] }"##,
            "not both all and race",
        ),
        (
            r##"{ "all": ["#a", "#b"], "sınır": 2 }"##,
            "\"sınır\" is not one of",
        ),
        // the list
        (
            r##"{ "race": "#a" }"##,
            "race is a list of two or more steps",
        ),
        (
            r##"{ "race": ["#a"] }"##,
            "race is a list of two or more steps",
        ),
        (r##"{ "any": [] }"##, "any is a list of two or more steps"),
        (
            r##"{ "settled": ["#a", 7] }"##,
            "a step of settled is a name or an object",
        ),
        (r##"{ "all": ["#a", "#"] }"##, "\"#\""),
        // count and names
        (r##"{ "some": ["#a", "#b"] }"##, "some needs count or names"),
        (
            r##"{ "some": ["#a", "#b"], "count": 1, "names": ["#a"] }"##,
            "count or names, not both",
        ),
        (
            r##"{ "some": ["#a", "#b"], "count": 0 }"##,
            "count is a whole number from 1 to 2",
        ),
        (
            r##"{ "some": ["#a", "#b"], "count": 3 }"##,
            "count is a whole number from 1 to 2",
        ),
        (
            r##"{ "some": ["#a", "#b"], "count": "iki" }"##,
            "count is a whole number from 1 to 2",
        ),
        (
            r##"{ "some": ["#a", "#b"], "names": [] }"##,
            "names is a list of the steps",
        ),
        (
            r##"{ "some": ["#a", "#b"], "names": "#a" }"##,
            "names is a list of the steps",
        ),
        (
            r##"{ "some": ["#a", "#b"], "names": ["#a", "#ç"] }"##,
            "names: \"#ç\" is not one of the steps",
        ),
        (
            r##"{ "race": ["#a", "#b"], "count": 1 }"##,
            "count goes with some",
        ),
        (
            r##"{ "race": ["#a", "#b"], "names": ["#a"] }"##,
            "names goes with some",
        ),
        // fail
        (
            r##"{ "all": ["#a", "#b"], "fail": "#ç" }"##,
            "fail: \"#ç\" is not one of the steps",
        ),
        (
            r##"{ "all": ["#a", "#b"], "fail": ["#a", "ç"] }"##,
            "fail: \"ç\" is not one of the steps",
        ),
        (r##"{ "all": ["#a", "#b"], "fail": [] }"##, "fail is"),
        (r##"{ "all": ["#a", "#b"], "fail": 3 }"##, "fail is"),
    ] {
        let e = err(&format!(r##"{{ "steps": [{step}] }}"##));
        assert!(e.contains(says), "{step}: {e}");
        // where: the step as written, or, for one of its steps, its address
        assert!(
            e.contains(&step.replace(' ', "")) || e.contains(step) || e.contains("(steps/"),
            "{step}: {e}"
        );
    }
}

#[test]
fn the_same_step_twice_is_read_by_its_number() {
    // twice, and nothing reads it by name: fine
    flow::parse(r##"{ "steps": [["#kontrol", "#kontrol"], "#notify", "#notify"] }"##).unwrap();
    // whatever their versions, one name
    let g = group(r##"["#kontrol", "#kontrol@0.2"]"##);
    let labels: Vec<&str> = g.members.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, ["#kontrol/1", "#kontrol/2"]);
    let f = flow::parse(
        r##"{ "steps": ["#bölge", "#bölge",
                      { "#topla": { "args": { "a": "$steps.bölge/1", "b": "$steps.bölge/2.toplam" } } }] }"##,
    )
    .unwrap();
    assert_eq!(f.step_keys(), vec!["bölge/1", "bölge/2", "topla"]);
}

#[test]
fn args_may_be_a_list_of_direct_items_and_formatted_pairs() {
    let f = flow::parse(
        r##"{ "steps": [{ "#sor": { "args": [{ "url": "$event.adres", "deneme": 3 }, { "title": "etiket", "text": "ayna $event.n" }] } }] }"##,
    )
    .unwrap();
    let Step::One { args, .. } = &f.steps[0] else {
        panic!()
    };
    let state = obj(
        json!({"event": {"adres": "http://127.0.0.1:9/fiyat", "n": 2, "bar": 42, "ad": "Ayşe"}}),
    );
    assert_eq!(
        flow::fill_args(args, &state).unwrap(),
        obj(json!({"url": "http://127.0.0.1:9/fiyat", "deneme": 3, "etiket": "ayna 2"}))
    );
    // a direct item assigns the value as it is; a pair formats it as text
    let direct = json!([{ "foo": "$event.bar" }, { "baz": "$event.ad is bar" }]);
    assert_eq!(
        flow::fill_args(&direct, &state).unwrap(),
        obj(json!({"foo": 42, "baz": "Ayşe is bar"}))
    );
    let pairs = json!([{ "title": "foo", "text": "$event.bar" }, { "title": "baz", "text": "$event.ad is bar" }]);
    assert_eq!(
        flow::fill_args(&pairs, &state).unwrap(),
        obj(json!({"foo": "42", "baz": "Ayşe is bar"}))
    );
    // an empty list is no args; an object and a whole name are read as before
    assert_eq!(flow::fill_args(&json!([]), &state).unwrap(), Map::new());
    assert_eq!(
        flow::fill_args(&json!({"title": "Fiyat", "text": "$event.bar"}), &state).unwrap(),
        obj(json!({"title": "Fiyat", "text": 42}))
    );
    assert_eq!(
        flow::fill_args(&json!("$output"), &obj(json!({"output": {"a": 1}}))).unwrap(),
        obj(json!({"a": 1}))
    );
}

#[test]
fn a_list_of_args_says_what_is_wrong() {
    for (args, says) in [
        (r#"[{ "url": "a" }, { "url": "b" }]"#, "url is given twice"),
        (
            r#"[{ "url": "a" }, { "title": "url", "text": "b" }]"#,
            "url is given twice",
        ),
        (r#"["$output"]"#, "each item of the list is an object"),
        (r#"[{ "event": 1 }]"#, "event is itero's"),
        (r#"[{ "title": "event", "text": "x" }]"#, "event is itero's"),
        (
            r#"[{ "title": "", "text": "x" }]"#,
            "the title of a pair names an arg",
        ),
        (
            r#"[{ "title": "n", "text": 5 }]"#,
            "the text of a pair is text",
        ),
        (
            r#"[{ "title": "n", "text": "$$API" }]"#,
            "a secret goes as a direct arg",
        ),
        (r#"[{ "n": "key: $$API" }]"#, "secret"),
        (r#"7"#, "args is an object, a list of objects"),
    ] {
        let e = err(&format!(
            r##"{{ "steps": [{{ "#sor": {{ "args": {args} }} }}] }}"##
        ));
        assert!(e.contains(says), "{args}: {e}");
        assert!(e.contains("#sor"), "{args}: {e}");
    }
}

#[test]
fn the_secrets_of_a_list_of_args_are_named() {
    let f = flow::parse(
        r##"{ "steps": [{ "#sor": { "args": [{ "key": "$$API_KEY" }, { "title": "n", "text": "x" }] } }] }"##,
    )
    .unwrap();
    assert_eq!(f.secret_names(), vec!["API_KEY".to_string()]);
}

#[test]
fn a_step_of_a_group_may_be_an_object_with_args_on_and_as() {
    let g = group(
        r##"{ "any": [
              { "#ayna": { "args": [{ "url": "http://127.0.0.1:9/fiyat" }] } },
              { "#ayna": { "args": { "url": "$event.ikinci" } } },
              { "yedek": { "on": [{ "$event.yedekli": true }] } },
              "#son"
            ] }"##,
    );
    let labels: Vec<&str> = g.members.iter().map(|m| m.label.as_str()).collect();
    // an action's step is read with #, a workflow's without, as names are
    assert_eq!(labels, vec!["#ayna/1", "#ayna/2", "yedek", "#son"]);
    assert_eq!(g.members[0].name, action("ayna"));
    assert_eq!(
        g.members[0].args,
        json!([{ "url": "http://127.0.0.1:9/fiyat" }])
    );
    assert!(g.members[2].on.is_some());
    assert_eq!(g.members[3].args, json!({}));
    // fail and names go by the labels, numbers and all
    let g = group(r##"{ "some": ["#ayna", "#ayna"], "names": ["#ayna/2"], "fail": "#ayna/1" }"##);
    assert_eq!(g.wait, Wait::Some(Need::Names(names(&["#ayna/2"]))));
    assert_eq!(g.fail, Some(Fail::Steps(names(&["#ayna/1"]))));
}

#[test]
fn a_step_of_a_group_says_what_is_wrong() {
    for (step, says) in [
        (
            r##"{ "any": ["#ayna", "#ayna"], "fail": "#ayna/3" }"##,
            "fail: \"#ayna/3\" is not one of the steps",
        ),
        (
            r##"{ "all": [{ "#kur": { "undo": "#sil" } }, "#b"] }"##,
            "undo is for a step of a list, not one side by side",
        ),
        (
            r##"{ "all": [{ "#a": { "as": "bir" } }, "#b"] }"##,
            "\"as\" is not one of args, on, undo",
        ),
        (
            r##"{ "all": ["#a", 7] }"##,
            "a step of all is a name or an object",
        ),
        (
            r##"{ "race": ["#a", { "any": ["#b", "#c"] }] }"##,
            "a group inside a group goes in a workflow of its own",
        ),
    ] {
        let e = err(&format!(r##"{{ "steps": [{step}] }}"##));
        assert!(e.contains(says), "{step}: {e}");
    }
}

#[test]
fn the_uses_secrets_and_conditions_of_a_group_s_steps_are_read() {
    let f = flow::parse(
        r##"{ "steps": [{ "race": [
              { "#ayna": { "args": { "key": "$$AYNA_KEY" } } },
              { "yedek": { "on": [{ "has": "$$YEDEK" }] } }
            ] }] }"##,
    )
    .unwrap();
    assert!(f.uses().contains(&action("ayna")));
    assert!(f.uses().contains(&Name::Workflow("yedek".into())));
    assert_eq!(f.secret_names(), vec!["AYNA_KEY".to_string()]);
    assert_eq!(f.defined_names(), vec!["YEDEK".to_string()]);
}

#[test]
fn a_step_s_on_holds_conditions_only() {
    for (on, what) in [
        (r#"{ "event": "deploy.finished" }"#, "event"),
        (r#"{ "schedule": "0 9 * * *" }"#, "schedule"),
        (r#"{ "path": "~/Gelen" }"#, "path"),
    ] {
        for steps in [
            format!(r##"[{{ "#süre": {{ "on": [{on}] }} }}]"##),
            format!(r##"[{{ "any": [{{ "#süre": {{ "on": [{on}] }} }}, "#b"] }}]"##),
        ] {
            let e = err(&format!(r##"{{ "steps": {steps} }}"##));
            assert!(
                e.contains("a step's on holds conditions")
                    && e.contains(what)
                    && e.contains("#süre"),
                "{steps}: {e}"
            );
        }
    }
    // conditions stay
    flow::parse(
        r##"{ "steps": [{ "#a": { "on": [{ "$event.ödendi": true }, { "has": "$event.cevap" }] } }] }"##,
    )
    .unwrap();
}

#[test]
fn the_names_of_the_args_given_to_an_action_are_read() {
    let f = flow::parse(
        r##"{ "steps": [
              { "#notify": { "args": { "title": "Fiyat", "metin": "$output" } } },
              { "any": [{ "#notify": { "args": [{ "text": "$event.x" }, { "title": "ses", "text": "çan" }] } }, "#b"] },
              { "#notify": { "args": "$output" } },
              { "#başka": { "args": { "adres": "x" } } }
            ] }"##,
    )
    .unwrap();
    assert_eq!(
        f.arg_names("notify"),
        names(&["metin", "ses", "text", "title"])
    );
}

#[test]
fn every_problem_of_a_workflow_is_told_with_its_address() {
    let v = json!({
        "on": [{ "event": "posta.geldi" }, { "event": "tek" }],
        "renk": "mavi",
        "steps": ["#ai", { "#ai": { "argz": {} } },
                  ["#a", { "#b": { "on": [{ "event": "x.y" }] } }],
                  { "#ship": { "undo": 3 } }],
        "error": ["#notify", 7],
        "timeout": "uzun"
    });
    let ps = flow::read(&v).unwrap_err();
    let at: Vec<&str> = ps.iter().map(|p| p.at.as_str()).collect();
    assert_eq!(
        at,
        [
            "on/2",
            "",
            "steps/#ai/2",
            "steps/all/#b",
            "steps/#ship/undo",
            "error",
            "timeout"
        ],
        "{ps:?}"
    );
    assert!(ps[1].message.contains("unknown key \"renk\""), "{ps:?}");
    assert!(ps[5].message.contains("the 2nd step"), "{}", ps[5].message);
}

#[test]
fn a_value_of_the_wrong_shape_is_one_problem() {
    let ps = flow::read(&json!({ "on": "her gün", "steps": "#ai" })).unwrap_err();
    assert_eq!(ps.len(), 2, "{ps:?}");
    assert_eq!((ps[0].at.as_str(), ps[1].at.as_str()), ("on", "steps"));
}

#[test]
fn a_step_is_addressed_by_its_action_and_place() {
    let items = json!(["#ai", "#notify", "#ai", "günlük", ["#a", "#b"],
                       { "race": ["#a", "#b"] }, { "race": ["#c", "#d"] }]);
    assert_eq!(
        flow::step_labels(items.as_array().unwrap()),
        [
            "#ai/1", "#notify", "#ai/2", "günlük", "all", "race/1", "race/2"
        ]
    );
}

#[test]
fn a_name_used_more_than_once_is_numbered_over_the_whole_file() {
    let f = flow::parse(
        r##"{ "steps": [
        "#ayna",
        { "race": ["#ayna", "#ayna"], "fail": "#ayna/3" },
        { "#kaydet": { "args": { "ikinci": "$steps.ayna/2.fiyat", "ilk": "$steps.ayna" } } } ],
      "error": ["#ayna"] }"##,
    )
    .unwrap();
    let Step::One { label, .. } = &f.steps[0] else {
        panic!()
    };
    assert_eq!(label, "#ayna/1");
    let Step::Side(g) = &f.steps[1] else { panic!() };
    let labels: Vec<&str> = g.members.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, ["#ayna/2", "#ayna/3"]);
    assert_eq!(g.fail, Some(Fail::Steps(vec!["#ayna/3".into()])));
    let Step::One { label, .. } = &f.steps[2] else {
        panic!()
    };
    assert_eq!(label, "#kaydet", "a name once has no number");
    let Step::One { label, .. } = &f.error[0] else {
        panic!()
    };
    assert_eq!(label, "#ayna/4");
}

#[test]
fn a_number_names_a_step_that_is_there() {
    for (text, says) in [
        (
            r##"{ "steps": [["#a", "#a"], { "#b": { "args": { "x": "$steps.a/3" } } }] }"##,
            "$steps.a/3 names no step",
        ),
        (
            r##"{ "steps": [{ "all": ["#a", "#a"], "fail": "#a/3" }] }"##,
            "\"#a/3\" is not one of the steps",
        ),
        (
            r##"{ "steps": [{ "#a": { "as": "bir" } }] }"##,
            "\"as\" is not one of args, on, undo",
        ),
    ] {
        let e = err(text);
        assert!(e.contains(says), "{text}: {e}");
    }
    // the name alone is the first
    let f = flow::parse(
        r##"{ "steps": [["#a", "#a"], { "#b": { "args": { "x": "$steps.a" } } }],
             "error": [{ "all": ["#c", "#c"], "fail": "#c" }] }"##,
    )
    .unwrap();
    let Step::Side(g) = &f.error[0] else { panic!() };
    assert_eq!(g.fail, Some(Fail::Steps(vec!["#c/1".into()])));
}

/// A name in a path cannot hold a `-`, where it would end the name inside
/// text; names from outside do, as a webhook's headers.
#[test]
fn an_underscore_in_a_path_also_finds_a_dash() {
    let state = json!({
        "event": {
            "headers": { "x-github-event": "push", "çift-tire_alt": 1 },
            "a-b": "tireli",
            "c_d": "alt çizgili",
            "c-d": "tireli",
            "düz": "metin"
        }
    });
    let state = state.as_object().unwrap().clone();
    let whole = |path: &str| fill(json!(format!("${path}")), &state);
    assert_eq!(whole("event.headers.x_github_event"), "push");
    assert_eq!(whole("event.headers.çift_tire_alt"), 1);
    assert_eq!(whole("event.a_b"), "tireli");
    // the name as written comes first
    assert_eq!(whole("event.c_d"), "alt çizgili");
    // and it goes one way: no field by that name, nothing else
    assert_eq!(whole("event.yok_alan"), Value::Null);
    // inside text too; a `-` still ends a name there
    assert_eq!(
        fill(
            json!("$event.headers.x_github_event: $event.a_b-son"),
            &state
        ),
        "push: tireli-son"
    );
    assert_eq!(fill(json!("$event.a-b"), &state), "-b");
    // and in a condition
    assert_eq!(
        flow::lookup(&state, "event.headers.x_github_event"),
        Some(&json!("push"))
    );
    assert_eq!(flow::lookup(&state, "event.düz_yok"), None);
}
