//! A workflow file in the simple form: `on`, `steps`, and the lists run at
//! the end.
use itero::flow::{self, Fail, Group, Name, Need, Step, Wait};
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
            { "#ship": { "args": { "carrier": "PTT Kargo" }, "on": [{ "$paid": true }] } },
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
            names: vec![
                action("check-payment"),
                Name::Workflow("stock-check".into())
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
            strict: false
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
        strict,
    } = &f.steps[3]
    else {
        panic!()
    };
    assert_eq!(*name, action("ship"));
    assert_eq!(*args, json!({"carrier": "PTT Kargo"}));
    assert!(!strict);
    assert_eq!(on.as_ref().unwrap().equal, obj(json!({"paid": true})));
    assert_eq!(*undo, None);
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
        (r##"{ "steps": [{ "#a": { "undo": 3 } }] }"##, "undo"),
        (r##"{ "steps": [{ "#a": { "undo": "#" } }] }"##, "\"#\""),
        // a step's values go under args
        (
            r##"{ "steps": [{ "#a": { "carrier": "PTT Kargo" } }] }"##,
            "\"carrier\" is not one of args, on, undo, strict",
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
            r##"{ "steps": [{ "#a": { "args": { "output": 1 } } }] }"##,
            "output is given by itero",
        ),
        (
            r##"{ "steps": [{ "#a": { "strict": "evet" } }] }"##,
            "strict is true or false",
        ),
        (
            r##"{ "steps": [{ "işlem": { "strict": true } }] }"##,
            "strict is for an action's step",
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
        "$order",
    ] {
        assert!(!flow::is_message_type(t), "{t}");
    }
}

#[test]
fn an_entry_keeps_the_known_keys_and_the_persons_own() {
    let (meta, settings) = flow::entry(&json!({
        "description": "Siparişler dükkândan",
        "from": "2026-11-01", "until": "2026-12-31",
        "internal": true,
        "sahibi": "ayşe", "etiketler": ["ödeme", "kargo"],
        "settings": { "kargo": "PTT Kargo" }
    }))
    .unwrap();
    assert_eq!(meta.description.as_deref(), Some("Siparişler dükkândan"));
    assert!(meta.internal && !meta.off);
    assert_eq!(meta.other["sahibi"], "ayşe");
    assert_eq!(meta.other["etiketler"], json!(["ödeme", "kargo"]));
    assert_eq!(settings["kargo"], "PTT Kargo");
    let at = |s: &str| s.parse().unwrap();
    // a date is a whole day: from its start, until its end
    assert!(!flow::active(&meta, at("2026-10-31T23:59:59Z")));
    assert!(flow::active(&meta, at("2026-11-01T00:00:00Z")));
    assert!(flow::active(&meta, at("2026-12-31T23:59:59Z")));
    assert!(!flow::active(&meta, at("2027-01-01T00:00:00Z")));
    let (timed, _) = flow::entry(&json!({ "until": "2026-12-31T18:00:00+03:00" })).unwrap();
    assert!(flow::active(&timed, at("2026-12-31T14:59:59Z")));
    assert!(!flow::active(&timed, at("2026-12-31T15:00:00Z")));
    let (off, _) = flow::entry(&json!({ "off": true })).unwrap();
    assert!(!flow::active(&off, at("2026-11-01T00:00:00Z")));
    let (plain, none) = flow::entry(&json!({})).unwrap();
    assert!(flow::active(&plain, at("2026-11-01T00:00:00Z")) && none.is_empty());
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
        (json!({ "settings": ["ü"] }), "settings are an object"),
    ] {
        let e = flow::entry(&entry).unwrap_err();
        assert!(e.contains(says), "{entry}: {e}");
    }
}

#[test]
fn args_may_come_whole_from_a_name_and_stay_with_their_step() {
    let f = flow::parse(
        r##"{ "steps": [{ "#a": { "args": "$output", "strict": true } }, { "#b": { "args": "$output.sipariş" } }] }"##,
    )
    .unwrap();
    let Step::One { args, strict, .. } = &f.steps[0] else {
        panic!()
    };
    assert_eq!(*args, json!("$output"));
    assert!(strict);
    let Step::One { args, strict, .. } = &f.steps[1] else {
        panic!()
    };
    assert_eq!(*args, json!("$output.sipariş"));
    assert!(!strict);
}

#[test]
fn a_name_reads_the_state_and_a_dot_one_field_of_it() {
    let state = obj(json!({"output": {"sayı": 3, "klasör": "İndirilenler"}, "adet": 5}));
    assert_eq!(fill(json!("$adet"), &state), json!(5));
    assert_eq!(
        fill(json!("$output"), &state),
        json!({"sayı": 3, "klasör": "İndirilenler"})
    );
    assert_eq!(fill(json!("$output.sayı"), &state), json!(3));
    // a field an object does not have, or a name with no value, is null
    assert_eq!(fill(json!("$output.yok"), &state), Value::Null);
    assert_eq!(fill(json!("$yok"), &state), Value::Null);
    // a value that is no object has no fields: the dot is text
    assert_eq!(fill(json!("$adet.yok"), &state), json!("5.yok"));
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
fn a_plain_result_of_steps_side_by_side_is_read_by_its_steps_name() {
    // side by side, a step that prints no object is kept under its name
    let state = obj(json!({"output": {"#boyut": 1064, "#tür": "image/png", "#kontrol-a": "açık"}}));
    assert_eq!(fill(json!("$output.#boyut"), &state), json!(1064));
    assert_eq!(fill(json!("$output.#kontrol-a"), &state), json!("açık"));
    assert_eq!(
        fill(json!("$output.#boyut bayt, $output.#tür"), &state),
        json!("1064 bayt, image/png")
    );
    assert_eq!(
        fill(json!("a: $output.#kontrol-a."), &state),
        json!("a: açık.")
    );
    // a # with no name after it is text
    assert_eq!(
        fill(json!("$output.# yok"), &state),
        json!(format!("{}.# yok", state["output"]))
    );
    let f = flow::parse(r##"{ "steps": [{ "#ship": { "on": [{ "$output.#boyut": 1064 }] } }] }"##)
        .unwrap();
    let Step::One { on: Some(on), .. } = &f.steps[0] else {
        panic!()
    };
    assert!(flow::holds(on, &state, &|_: &str| false));
}

#[test]
fn names_are_filled_inside_text() {
    let state = obj(json!({
        "count": 3, "folder": "İndirilenler", "dosya": "rapor", "boş": "",
        "liste": [1, 2], "nesne": {"a": 1},
        "output": {"sayı": 4, "hiç": null}
    }));
    for (text, filled) in [
        ("$count files in ~/$folder", "3 files in ~/İndirilenler"),
        ("$count.", "3."),
        ("$count$count", "33"),
        // not there, null or "": empty
        ("x $yok y", "x  y"),
        ("[$boş]", "[]"),
        ("[$output.hiç]", "[]"),
        // a field when the object has it, else the dot is text
        ("$output.sayı files", "4 files"),
        ("$dosya.txt", "rapor.txt"),
        // a list or an object as its JSON
        ("$liste and $nesne", "[1,2] and {\"a\":1}"),
        // \$ is a dollar sign; a $ that starts no name is itself
        ("\\$count costs \\$5", "$count costs $5"),
        ("\\$$count", "$3"),
        ("a $ b $5 $", "a $ b $5 $"),
    ] {
        assert_eq!(fill(json!(text), &state), json!(filled), "{text}");
    }
    // a whole value keeps its type
    assert_eq!(fill(json!("$count"), &state), json!(3));
    assert_eq!(fill(json!("$yok"), &state), Value::Null);
    // texts nested in lists and objects are filled too
    assert_eq!(
        fill(
            json!({"buttons": ["Gönder $count", "$count"], "iç": {"t": "~/$folder"}}),
            &state
        ),
        json!({"buttons": ["Gönder 3", 3], "iç": {"t": "~/İndirilenler"}})
    );
}

#[test]
fn a_secret_is_never_filled_inside_text() {
    let state = obj(json!({"parola": "$$POSTA_PAROLA"}));
    // a whole value stays as written, for the action alone
    assert_eq!(
        fill(json!("$$POSTA_PAROLA"), &state),
        json!("$$POSTA_PAROLA")
    );
    assert_eq!(fill(json!("$parola"), &state), json!("$$POSTA_PAROLA"));
    for text in ["giriş $$POSTA_PAROLA", "giriş $parola"] {
        let e = flow::fill(&json!(text), &state).unwrap_err();
        assert!(e.contains("a secret is a whole value only"), "{text}: {e}");
    }
    // a workflow that writes one inside text is refused when it is read
    let e = err(r##"{ "steps": [{ "#a": { "args": { "t": ["x $$API"] } } }] }"##);
    assert!(e.contains("a secret is a whole value only"), "{e}");
}

fn group(step: &str) -> Group {
    let f = flow::parse(&format!(r##"{{ "steps": [{step}] }}"##)).unwrap();
    let Step::Side(g) = &f.steps[0] else {
        panic!("{:?}", f.steps[0]);
    };
    g.clone()
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
        bare.names,
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
        assert_eq!(g.names, vec![action("şu"), action("bu")], "{key}");
    }
    let g = group(r##"{ "some": ["#a", "#b", "#c"], "count": 2 }"##);
    assert_eq!(g.wait, Wait::Some(Need::Count(2)));
    assert_eq!(g.names.len(), 3);
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
            r##"{ "settled": ["#a", { "#b": {} }] }"##,
            "settled holds names of actions and workflows",
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
        // the step is shown as written
        assert!(
            e.contains(&step.replace(' ', "")) || e.contains(step),
            "{step}: {e}"
        );
    }
}
