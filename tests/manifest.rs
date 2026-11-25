//! itero.config.json's schema: made from the config's types, published,
//! and every example a config it accepts.
use serde_json::{Value, json};

fn schema() -> Value {
    itero::manifest::schema()
}

#[test]
fn the_published_schema_is_the_generated_one() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/public/schema/itero.config.json");
    let now = serde_json::to_string_pretty(&schema()).unwrap() + "\n";
    if std::env::var_os("ITERO_WRITE_SCHEMA").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &now).unwrap();
    }
    let published = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        published == now,
        "docs/public/schema/itero.config.json is not the schema the types make: make schema"
    );
}

#[test]
fn the_schema_knows_every_key_and_no_other() {
    let s = schema();
    let top: Vec<&str> = s["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for k in [
        "$schema",
        "name",
        "version",
        "description",
        "tags",
        "port",
        "folders",
        "limits",
        "workflow",
        "action",
        "connector",
        "metadata",
    ] {
        assert!(top.contains(&k), "{k}: {top:?}");
    }
    assert_eq!(top.len(), 12, "{top:?}");
    assert_eq!(s["additionalProperties"], json!(false), "{s}");
}

/// The problems the schema finds in `config`, as jsonschema tells them.
fn wrong(config: &Value) -> Vec<String> {
    let v = jsonschema::validator_for(&schema()).unwrap();
    v.iter_errors(config).map(|e| e.to_string()).collect()
}

#[test]
fn each_example_is_a_config_the_schema_accepts() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut seen = 0;
    for e in std::fs::read_dir(examples).unwrap() {
        let f = e.unwrap().path().join("itero.config.json");
        if !f.is_file() {
            continue;
        }
        let c: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert!(wrong(&c).is_empty(), "{}: {:?}", f.display(), wrong(&c));
        seen += 1;
    }
    assert!(seen >= 4, "{seen}");
}

#[test]
fn the_schema_refuses_what_is_not_itero_and_takes_what_is() {
    // every key and form the config has, at once
    let good = json!({
        "$schema": "https://docs.itero.sh/schema/itero.config.json",
        "name": "ev-isleri",
        "version": "0.1.0",
        "description": "Posta ve siparişler",
        "tags": ["ev"],
        "port": 8787,
        "folders": { "workflows": ["iş akışları"], "actions": ["eylemler"], "connectors": ["kaynaklar"] },
        "limits": { "workflows": 5, "runs": 7, "steps": 2, "logs": 1,
                    "space": "500MB", "log": "64KB", "keep": "2h", "quiet": "30m", "most": "1h 30m" },
        "workflow": { "keep": false, "load": true,
                      "gözcü": { "description": "Siteler", "tags": ["gece"], "off": false,
                                 "from": "2026-11-01", "until": "2026-12-01T09:00:00Z",
                                 "internal": false, "metadata": { "renk": 1 } } },
        "action": { "bak": { "description": "Bakar", "metadata": { "ne": "olursa" } } },
        "connector": { "posta.geldi": true },
        "metadata": { "sahibi": { "her": ["şey"] } }
    });
    assert!(wrong(&good).is_empty(), "{:?}", wrong(&good));
    for bad in [
        json!({ "prot": 8787 }),
        json!({ "port": null }),
        json!({ "folders": null }),
        json!({ "port": "8787" }),
        json!({ "port": 70000 }),
        json!({ "name": "ev-işleri" }),
        json!({ "name": "Ev" }),
        json!({ "limits": { "steps": 0 } }),
        json!({ "limits": { "space": "lots" } }),
        json!({ "limits": { "keep": "1x" } }),
        json!({ "limits": { "kep": "1d" } }),
        json!({ "folders": { "workflow": ["x"] } }),
        json!({ "workflow": { "gözcü": { "of": true } } }),
        json!({ "workflow": { "gözcü": { "from": "yarın" } } }),
        json!({ "workflow": { "gözcü": "açık" } }),
        json!({ "action": { "bak": { "off": true } } }),
        json!({ "action": { "bak": { "args": {} } } }),
        json!({ "tags": "ev" }),
    ] {
        assert!(!wrong(&bad).is_empty(), "accepted: {bad}");
    }
}

/// What `problems` says of `text`: each problem's address and message.
fn told(text: &str) -> Vec<(String, String)> {
    let (_, ps) = itero::manifest::problems(text);
    ps.into_iter().map(|p| (p.at, p.message)).collect()
}

#[test]
fn every_problem_of_a_config_is_told_at_once_with_its_place() {
    let ps = told(
        r#"{
        "prot": 8787,
        "port": "8787",
        "limits": { "keep": "1x", "steps": 0 },
        "workflow": { "gözcü": { "of": true }, "eski": { "settings": {} } },
        "action": { "kargola": { "args": { "kaç": 1 } } }
    }"#,
    );
    let has = |at: &str, says: &str| ps.iter().any(|(a, m)| a == at && m.contains(says));
    assert!(
        has(
            "",
            r#""prot" is not a key of itero.config.json; your own keys go in "metadata""#
        ),
        "{ps:?}"
    );
    assert!(has("", "port is a number from 1 to 65535"), "{ps:?}");
    assert!(
        has(
            "limits",
            "keep: a duration is like 30m, 2h or 1d, not \"1x\""
        ),
        "{ps:?}"
    );
    assert!(has("limits", "steps is at least 1"), "{ps:?}");
    assert!(
        has(
            "workflows/gözcü",
            r#""of" is not a key of a workflow entry: description, tags, off, from, until, internal, metadata"#
        ),
        "{ps:?}"
    );
    assert!(has("workflows/eski", "settings are gone"), "{ps:?}");
    assert!(
        has(
            "actions/kargola",
            "args are gone: an action's args are written in the step that runs it"
        ),
        "{ps:?}"
    );
    assert_eq!(ps.len(), 7, "{ps:?}");
    assert!(
        ps.iter().all(|(_, m)| m.starts_with("itero.config.json: ")),
        "{ps:?}"
    );
}

#[test]
fn json_that_does_not_parse_is_one_problem_with_its_line() {
    let ps = told("{\n  \"port\": 8787,\n  ,\n}");
    assert_eq!(ps.len(), 1, "{ps:?}");
    assert!(ps[0].1.contains("line 3"), "{ps:?}");
}

#[test]
fn true_and_false_entries_metadata_and_schema_are_fine() {
    let ps = told(
        r#"{
        "$schema": "https://docs.itero.sh/schema/itero.config.json",
        "name": "ev-isleri",
        "tags": ["ev"],
        "metadata": { "sahibi": { "ne": "olursa" } },
        "workflow": { "load": true, "keep": false, "gözcü": { "metadata": { "renk": 1 } } }
    }"#,
    );
    assert!(ps.is_empty(), "{ps:?}");
}

#[test]
fn values_the_patterns_let_through_are_still_read() {
    // each pattern takes these, and the parsers refuse them
    let ps = told(
        r#"{ "limits": { "keep": "0s", "space": "99999999999999999999GB" },
                       "workflow": { "gözcü": { "from": "2026-13-45" } } }"#,
    );
    let has = |at: &str, says: &str| ps.iter().any(|(a, m)| a == at && m.contains(says));
    assert!(has("limits", "keep is at least 1s"), "{ps:?}");
    assert!(has("limits", "space: a size is like 10MB or 1GB"), "{ps:?}");
    assert!(
        has("workflows/gözcü", "from: a date (2026-11-01) or a time"),
        "{ps:?}"
    );
    assert_eq!(ps.len(), 3, "{ps:?}");
}

#[test]
fn each_kind_of_mistake_is_told_in_itero_words() {
    let cases = [
        (
            r#"{ "name": "ev-işleri" }"#,
            "",
            "name: lowercase letters, digits and hyphens, 1 to 63, a letter or digit at each end",
        ),
        (
            r#"{ "port": 70000 }"#,
            "",
            "port is a number from 1 to 65535",
        ),
        (r#"{ "tags": "ev" }"#, "", "tags is a list of text"),
        (
            r#"{ "id": "x" }"#,
            "",
            "id is given by itero and cannot be set",
        ),
        (
            r#"{ "limits": { "kep": "1d" } }"#,
            "limits",
            r#""kep" is not a limit: workflows, runs, steps, space, keep, log, logs, quiet, most"#,
        ),
        (
            r#"{ "limits": { "space": "lots" } }"#,
            "limits",
            "space: a size is like 10MB or 1GB, not \"lots\"",
        ),
        (
            r#"{ "folders": { "workflow": ["x"] } }"#,
            "folders",
            r#""workflow" is not a key of folders: workflows, actions, connectors"#,
        ),
        (
            r#"{ "workflow": { "gözcü": "açık" } }"#,
            "workflows/gözcü",
            r#"a workflow entry is true, false or an object: { "description": "..." }"#,
        ),
        (
            r#"{ "workflow": { "gözcü": { "from": "yarın" } } }"#,
            "workflows/gözcü",
            "from: a date (2026-11-01) or a time (2026-11-01T09:00:00Z)",
        ),
        (
            r#"{ "action": { "bak": { "off": true } } }"#,
            "actions/bak",
            r#""off" is not a key of an action entry: description, tags, metadata"#,
        ),
        (
            r#"{ "action": { "bak": { "repeat": [] } } }"#,
            "actions/bak",
            r##"repeat is gone: repeat goes on the step: { "#bak": { "repeat": ... } }"##,
        ),
        (
            r#"{ "connector": { "posta.geldi": { "args": {} } } }"#,
            "connectors/posta.geldi",
            r#"args are gone: write them in the on entry of each workflow that listens: { "event": "posta.geldi", "args": {...} }"#,
        ),
    ];
    for (text, at, says) in cases {
        let ps = told(text);
        assert!(
            ps.len() == 1 && ps[0].0 == at && ps[0].1 == format!("itero.config.json: {says}"),
            "{text}: {ps:?}"
        );
    }
}
