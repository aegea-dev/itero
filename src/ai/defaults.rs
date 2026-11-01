//! The values a `#ai` step leaves out. The first that has one wins: the
//! step, the machine (`itero ai defaults set`), the model (itero's table,
//! else its file), llama.cpp. itero fills them in before asking, so a
//! step's request says what was used.
use anyhow::Result;
use itero_ai_index::{Entry, Index};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the machine may set: `#ai`'s args that are not the step's own work.
pub const KEYS: [&str; 11] = [
    "model",
    "thinking",
    "temperature",
    "top_p",
    "top_k",
    "min_p",
    "presence_penalty",
    "max_tokens",
    "seed",
    "context",
    "kv_cache",
];

/// A family's sampling by mode, and where it comes from.
struct Row {
    /// What `general.basename` starts with.
    basename: &'static str,
    plain: [(&'static str, f64); 5],
    thinking: [(&'static str, f64); 5],
    source: &'static str,
}

/// The models itero advises. Before a model's file: a row knows the mode,
/// and a file does not (Qwen3.5-35B's carry the thinking mode's alone).
const TABLE: [Row; 1] = [Row {
    basename: "Qwen3.5-",
    plain: [
        ("temperature", 0.7),
        ("top_p", 0.8),
        ("top_k", 20.0),
        ("min_p", 0.0),
        ("presence_penalty", 1.5),
    ],
    thinking: [
        ("temperature", 1.0),
        ("top_p", 0.95),
        ("top_k", 20.0),
        ("min_p", 0.0),
        ("presence_penalty", 1.5),
    ],
    source: "Unsloth's Qwen3.5 guide",
}];

/// A number as JSON: `top_k` whole, the rest as written.
fn number(k: &str, v: f64) -> Value {
    if k == "top_k" {
        json!(v as i64)
    } else {
        json!(v)
    }
}

/// What a model gives itself: each key, its value, and where from.
pub fn model_values(e: &Entry, thinking: bool) -> Vec<(&'static str, Value, String)> {
    let name = e.basename.as_deref().unwrap_or("");
    if let Some(r) = TABLE.iter().find(|r| name.starts_with(r.basename)) {
        let from = format!("itero's table ({})", r.source);
        let row = if thinking { &r.thinking } else { &r.plain };
        return row
            .iter()
            .map(|(k, v)| (*k, number(k, *v), from.clone()))
            .collect();
    }
    let Some(s) = &e.sampling else {
        return vec![];
    };
    [
        ("temperature", s.temperature),
        ("top_p", s.top_p),
        ("top_k", s.top_k),
        ("min_p", s.min_p),
    ]
    .into_iter()
    .filter_map(|(k, v)| Some((k, number(k, v?), "the model's file".to_string())))
    .collect()
}

/// A value as a step's environment has it.
fn env_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Fills what a step left out. A model this machine does not have gives
/// nothing: itero-ai says so when asked.
pub fn fill(env: &mut BTreeMap<String, String>, machine: &Map<String, Value>, ix: &Index) {
    for k in KEYS {
        if let (false, Some(v)) = (env.contains_key(k), machine.get(k)) {
            env.insert(k.to_string(), env_text(v));
        }
    }
    let Some(e) = env
        .get("model")
        .and_then(|m| itero_ai_index::resolve(ix, m).ok())
    else {
        return;
    };
    let thinking = env.get("thinking").is_some_and(|t| t == "true");
    for (k, v, _) in model_values(e, thinking) {
        env.entry(k.to_string()).or_insert_with(|| env_text(&v));
    }
}

/// The machine's file, in itero's state folder: a person's choices, so
/// not in the cache.
pub fn path() -> Result<PathBuf> {
    Ok(crate::machine::state_dir()?.join("ai-defaults.json"))
}

/// The machine's defaults; no file yet is none.
pub fn read(p: &Path) -> Result<Map<String, Value>> {
    match std::fs::read_to_string(p) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| anyhow::anyhow!("{}: {e}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(anyhow::anyhow!("{}: {e}", p.display())),
    }
}

/// Writes it whole: a new file, then a rename.
pub fn write(p: &Path, m: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    }
    let new = p.with_extension("json.new");
    std::fs::write(&new, serde_json::to_string_pretty(m)? + "\n")
        .map_err(|e| anyhow::anyhow!("{}: {e}", new.display()))?;
    std::fs::rename(&new, p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))
}

/// `old` with `changes` in it: a null takes a key out. Values follow
/// `#ai`'s own rules.
pub fn changed(
    old: &Map<String, Value>,
    changes: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut new = old.clone();
    for (k, v) in changes {
        if !KEYS.contains(&k.as_str()) {
            let (last, rest) = KEYS.split_last().expect("keys");
            return Err(format!("it takes {} and {last}, not {k}", rest.join(", ")));
        }
        if v.is_null() {
            new.remove(k);
        } else {
            new.insert(k.clone(), v.clone());
        }
    }
    let mut env: BTreeMap<String, String> =
        new.iter().map(|(k, v)| (k.clone(), env_text(v))).collect();
    env.insert("text".into(), "x".into());
    super::step::body(env.get("model").map_or("", String::as_str), &env)?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use itero_ai_index::FileSampling;

    fn model(repo: &str, basename: Option<&str>, sampling: Option<FileSampling>) -> Entry {
        Entry {
            name: format!("{repo}:Q4_K_M"),
            repo: repo.into(),
            quant: "Q4_K_M".into(),
            paths: vec!["/m.gguf".into()],
            maker: "u".into(),
            params: None,
            architecture: "qwen35".into(),
            context: Some(262144),
            bytes: 0,
            licence: "apache-2.0".into(),
            chat: true,
            runs: true,
            thinking: true,
            basename: basename.map(String::from),
            sampling,
            thinking_off: Some(true),
            modified: 0,
            downloaded: None,
            removed: None,
        }
    }

    fn env(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn machine(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn ix(models: Vec<Entry>) -> Index {
        Index { models }
    }

    #[test]
    fn the_first_layer_that_has_a_value_wins() {
        let qwen = ix(vec![model("u/q", Some("Qwen3.5-4B"), None)]);
        let mut e = env(&[("text", "özetle"), ("temperature", "0.1")]);
        fill(
            &mut e,
            &machine(json!({ "model": "u/q", "top_p": 0.5 })),
            &qwen,
        );
        assert_eq!(e["model"], "u/q");
        assert_eq!(e["temperature"], "0.1", "the step");
        assert_eq!(e["top_p"], "0.5", "the machine");
        assert_eq!(e["top_k"], "20", "the model");
        assert!(!e.contains_key("seed"), "llama.cpp's own");
    }

    #[test]
    fn the_table_row_follows_thinking_and_comes_before_the_file() {
        let file = FileSampling {
            temperature: Some(1.0),
            top_p: Some(0.95),
            top_k: Some(20.0),
            min_p: None,
        };
        let q = ix(vec![model("u/q", Some("Qwen3.5-35B-A3B"), Some(file))]);
        let mut e = env(&[("model", "u/q"), ("text", "x")]);
        fill(&mut e, &Map::new(), &q);
        assert_eq!(
            (e["temperature"].as_str(), e["presence_penalty"].as_str()),
            ("0.7", "1.5")
        );
        let mut e = env(&[("model", "u/q"), ("text", "x"), ("thinking", "true")]);
        fill(&mut e, &Map::new(), &q);
        assert_eq!(e["temperature"], "1.0");
    }

    #[test]
    fn a_model_without_a_row_uses_its_file_and_others_nothing() {
        let file = FileSampling {
            temperature: Some(1.0),
            top_k: Some(64.0),
            ..Default::default()
        };
        let m = ix(vec![
            model("u/gemma", Some("Gemma-4-E2B-It"), Some(file)),
            model("u/yalın", Some("Agents-A1"), None),
            model("u/ince", Some("Qwen3.8"), None),
        ]);
        let mut e = env(&[("model", "u/gemma"), ("text", "x")]);
        fill(&mut e, &Map::new(), &m);
        assert_eq!(e["top_k"], "64");
        for repo in ["u/yalın", "u/ince", "u/yok"] {
            let mut e = env(&[("model", repo), ("text", "x")]);
            fill(&mut e, &Map::new(), &m);
            assert_eq!(e.len(), 2, "{repo}");
        }
    }

    #[test]
    fn a_change_merges_null_takes_out_and_what_is_wrong_is_said() {
        let old = machine(json!({ "model": "u/q", "top_k": 20 }));
        let new = changed(&old, &machine(json!({ "top_k": null, "temperature": 0.3 }))).unwrap();
        assert_eq!(
            Value::Object(new),
            json!({ "model": "u/q", "temperature": 0.3 })
        );
        for (bad, says) in [
            (json!({ "text": "x" }), "text"),
            (json!({ "temperature": "sıcak" }), "temperature"),
            (json!({ "kv_cache": "renkli" }), "kv_cache"),
        ] {
            let e = changed(&old, &machine(bad)).unwrap_err();
            assert!(e.contains(says), "{e}");
        }
    }

    #[test]
    fn the_file_is_written_whole_and_none_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("durum/ai-defaults.json");
        assert!(read(&p).unwrap().is_empty());
        write(&p, &machine(json!({ "top_k": 20 }))).unwrap();
        assert_eq!(read(&p).unwrap()["top_k"], 20);
    }
}
