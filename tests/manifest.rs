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
