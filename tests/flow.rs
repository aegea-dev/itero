//! A workflow file in the simple form: `on`, `steps`, and the lists run at
//! the end.
use itero::flow::{self, Name, Step};
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
              { "$paid": true, "$stock": 0 },
              { "has": "$sipariş" },
              { "has": ["$tutar", "$$API"] }
            ],
            "steps": ["#onayla"] }"##,
    )
    .unwrap();
    assert_eq!(f.on.events, vec!["sipariş.alındı"]);
    assert_eq!(f.on.schedules, vec!["0 9 * * *"]);
    assert_eq!(f.on.paths, vec!["/var/mail/$$USER"]);
    assert_eq!(f.on.equal, obj(json!({"paid": true, "stock": 0})));
    assert_eq!(f.on.has, vec!["$sipariş", "$tutar", "$$API"]);
    assert!(flow::starts(&f.on));
    let g = flow::parse(r##"{ "on": [{ "has": "$order" }], "steps": ["#ship"] }"##).unwrap();
    assert!(!flow::starts(&g.on), "conditions alone start nothing");
}

#[test]
fn conditions_hold_on_exact_values_present_names_and_defined_secrets() {
    let f = flow::parse(
        r##"{ "on": [{ "has": ["$order", "$$API"] }],
              "steps": [{ "#ship": { "on": [{ "$paid": true, "$stock": 0 }] } }] }"##,
    )
    .unwrap();
    let defined = |n: &str| n == "API";
    let none = |_: &str| false;
    assert!(flow::holds(&f.on, &obj(json!({"order": 43})), &defined));
    assert!(!flow::holds(&f.on, &obj(json!({"order": 43})), &none));
    assert!(!flow::holds(
        &f.on,
        &obj(json!({"müşteri": "ayşe"})),
        &defined
    ));
    let Step::One { on: Some(on), .. } = &f.steps[0] else {
        panic!("{:?}", f.steps[0]);
    };
    assert!(flow::holds(
        on,
        &obj(json!({"paid": true, "stock": 0})),
        &none
    ));
    // exact: 0 is not false, true is not "true"
    assert!(!flow::holds(
        on,
        &obj(json!({"paid": true, "stock": false})),
        &none
    ));
    assert!(!flow::holds(
        on,
        &obj(json!({"paid": "true", "stock": 0})),
        &none
    ));
    assert!(!flow::holds(on, &obj(json!({"paid": true})), &none));
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
            { "#ship": { "carrier": "PTT Kargo", "on": [{ "$paid": true }] } }
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
        Step::Side(vec![
            action("check-payment"),
            Name::Workflow("stock-check".into())
        ])
    );
    assert_eq!(
        f.steps[1],
        Step::One {
            name: Name::Workflow("order-process".into()),
            data: Map::new(),
            on: None
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
    let Step::One { name, data, on } = &f.steps[3] else {
        panic!()
    };
    assert_eq!(*name, action("ship"));
    assert_eq!(*data, obj(json!({"carrier": "PTT Kargo"})));
    assert_eq!(on.as_ref().unwrap().equal, obj(json!({"paid": true})));
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
        (r##"{ "on": { "$a": 1 }, "steps": ["#a"] }"##, "a list"),
        (
            r##"{ "on": [{ "event": "sipariş" }], "steps": ["#a"] }"##,
            "\"sipariş\"",
        ),
        (
            r##"{ "on": [{ "schedule": "61 * * * *" }], "steps": ["#a"] }"##,
            "61 * * * *",
        ),
        (
            r##"{ "on": [{ "event": "a.b", "$vip": true }], "steps": ["#a"] }"##,
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
        (r##"{ "steps": [["#a", { "#b": {} }]] }"##, "side by side"),
        (r##"{ "steps": ["#"] }"##, "\"#\""),
        (r##"{ "steps": ["#a@x.1"] }"##, "version"),
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
        "$order",
    ] {
        assert!(!flow::is_message_type(t), "{t}");
    }
}
