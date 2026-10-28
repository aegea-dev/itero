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
        // two steps with the same name, whatever their versions
        (
            r##"{ "all": ["#a", "#b", "#a"] }"##,
            "#a is in the list twice",
        ),
        (
            r##"{ "race": ["#a", "#a@0.2"] }"##,
            "#a is in the list twice",
        ),
        (
            r##"{ "settled": ["yedekle", "#a", "yedekle"] }"##,
            "yedekle is in the list twice",
        ),
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

#[test]
fn a_bare_list_may_name_a_step_twice() {
    let g = group(r##"["#kontrol", "#kontrol"]"##);
    assert_eq!(g.members.len(), 2);
}

#[test]
fn args_may_be_a_list_of_direct_items_and_formatted_pairs() {
    let f = flow::parse(
        r##"{ "steps": [{ "#sor": { "args": [{ "url": "$adres", "deneme": 3 }, { "title": "etiket", "text": "ayna $n" }] } }] }"##,
    )
    .unwrap();
    let Step::One { args, .. } = &f.steps[0] else {
        panic!()
    };
    let state = obj(json!({"adres": "http://127.0.0.1:9/fiyat", "n": 2, "bar": 42, "ad": "Ayşe"}));
    assert_eq!(
        flow::fill_args(args, &state).unwrap(),
        obj(json!({"url": "http://127.0.0.1:9/fiyat", "deneme": 3, "etiket": "ayna 2"}))
    );
    // a direct item assigns the value as it is; a pair formats it as text
    let direct = json!([{ "foo": "$bar" }, { "baz": "$ad is bar" }]);
    assert_eq!(
        flow::fill_args(&direct, &state).unwrap(),
        obj(json!({"foo": 42, "baz": "Ayşe is bar"}))
    );
    let pairs =
        json!([{ "title": "foo", "text": "$bar" }, { "title": "baz", "text": "$ad is bar" }]);
    assert_eq!(
        flow::fill_args(&pairs, &state).unwrap(),
        obj(json!({"foo": "42", "baz": "Ayşe is bar"}))
    );
    // an empty list is no args; an object and a whole name are read as before
    assert_eq!(flow::fill_args(&json!([]), &state).unwrap(), Map::new());
    assert_eq!(
        flow::fill_args(&json!({"title": "Fiyat", "text": "$bar"}), &state).unwrap(),
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
        (r#"[{ "output": 1 }]"#, "output is given by itero"),
        (
            r#"[{ "title": "output", "text": "x" }]"#,
            "output is given by itero",
        ),
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
fn a_step_of_a_group_may_be_an_object_with_args_on_strict_and_as() {
    let g = group(
        r##"{ "any": [
              { "#ayna": { "args": [{ "url": "http://127.0.0.1:9/fiyat" }], "as": "ayna-1" } },
              { "#ayna": { "args": { "url": "$ikinci" }, "as": "ayna-2", "strict": true } },
              { "yedek": { "on": [{ "$yedekli": true }], "as": "yedek-akış" } },
              "#son"
            ] }"##,
    );
    let labels: Vec<&str> = g.members.iter().map(|m| m.label.as_str()).collect();
    // an action's step is read with #, a workflow's without, as names are
    assert_eq!(labels, vec!["#ayna-1", "#ayna-2", "yedek-akış", "#son"]);
    assert_eq!(g.members[0].name, action("ayna"));
    assert_eq!(
        g.members[0].args,
        json!([{ "url": "http://127.0.0.1:9/fiyat" }])
    );
    assert!(g.members[1].strict);
    assert!(g.members[2].on.is_some());
    assert_eq!(g.members[3].args, json!({}));
    // fail and names go by the labels
    let g = group(
        r##"{ "some": [{ "#ayna": { "as": "bir" } }, { "#ayna": { "as": "iki" } }], "names": ["#iki"], "fail": "#bir" }"##,
    );
    assert_eq!(g.wait, Wait::Some(Need::Names(names(&["#iki"]))));
    assert_eq!(g.fail, Some(Fail::Steps(names(&["#bir"]))));
}

#[test]
fn a_step_of_a_group_says_what_is_wrong() {
    for (step, says) in [
        (
            r##"{ "any": [{ "#ayna": {} }, { "#ayna": {} }] }"##,
            "#ayna is in the list twice: give each \"as\"",
        ),
        (
            r##"{ "any": ["#ayna", { "#ayna": { "as": "ayna" } }] }"##,
            "#ayna is in the list twice",
        ),
        (
            r##"{ "any": [{ "#ayna": { "as": "bir" } }, { "#ayna": { "as": "iki" } }], "fail": "#ayna" }"##,
            "fail: \"#ayna\" is not one of the steps",
        ),
        (
            r##"{ "all": [{ "#kur": { "undo": "#sil" } }, "#b"] }"##,
            "undo is for a step of a list, not one side by side",
        ),
        (
            r##"{ "all": [{ "#a": { "as": "" } }, "#b"] }"##,
            "as names the step",
        ),
        (
            r##"{ "all": [{ "#a": { "as": "#a2" } }, "#b"] }"##,
            "as names the step",
        ),
        (
            r##"{ "all": ["#a", 7] }"##,
            "a step of all is a name or an object",
        ),
    ] {
        let e = err(&format!(r##"{{ "steps": [{step}] }}"##));
        assert!(e.contains(says), "{step}: {e}");
    }
    // as names a step side by side only
    let e = err(r##"{ "steps": [{ "#a": { "as": "bir" } }] }"##);
    assert!(e.contains("as names a step side by side"), "{e}");
}

#[test]
fn the_uses_secrets_and_conditions_of_a_group_s_steps_are_read() {
    let f = flow::parse(
        r##"{ "steps": [{ "race": [
              { "#ayna": { "args": { "key": "$$AYNA_KEY" }, "as": "bir" } },
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
        r##"{ "steps": [{ "#a": { "on": [{ "$ödendi": true }, { "has": "$cevap" }] } }] }"##,
    )
    .unwrap();
}

#[test]
fn the_names_of_the_args_given_to_an_action_are_read() {
    let f = flow::parse(
        r##"{ "steps": [
              { "#notify": { "args": { "title": "Fiyat", "metin": "$output" } } },
              { "any": [{ "#notify": { "args": [{ "text": "$x" }, { "title": "ses", "text": "çan" }] } }, "#b"] },
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
