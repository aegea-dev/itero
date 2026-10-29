//! A real model: runs only when ITERO_AI_TEST_MODEL names a GGUF file
//! (Qwen3.5-4B advised), never in CI. llama.cpp's backend starts once a
//! process: this file's only test. A schema holds the answer to its
//! form, and the prompt says that form too, as `#ai` does: a model made to
//! open a JSON string it did not mean to write answers poorly.
use itero_ai::engine::{Engine, Message, Piece, Prompt, Sampling};
use itero_ai_index::{Entry, Index};
use serde_json::json;

fn ask(
    e: &mut itero_ai::llama::LlamaEngine,
    text: &str,
    schema: Option<serde_json::Value>,
    thinking: bool,
) -> (String, String) {
    let p = Prompt {
        model: "test/model".into(),
        messages: vec![Message {
            role: "user".into(),
            content: text.into(),
        }],
        // Qwen3.5's own settings
        sampling: Sampling {
            temperature: Some(0.7),
            top_p: Some(0.8),
            top_k: Some(20),
            min_p: Some(0.0),
            presence_penalty: Some(1.5),
            frequency_penalty: None,
            seed: Some(7),
        },
        max_tokens: Some(if thinking { 4096 } else { 128 }),
        stop: vec![],
        json: schema.is_some(),
        schema,
        thinking,
    };
    let (mut c, mut r) = (String::new(), String::new());
    e.complete(&p, &mut |piece| {
        match piece {
            Piece::Content(t) => c.push_str(&t),
            Piece::Reasoning(t) => r.push_str(&t),
        }
        true
    })
    .unwrap();
    (c, r)
}

#[test]
fn a_real_model_answers_chooses_and_fills_fields() {
    let Ok(path) = std::env::var("ITERO_AI_TEST_MODEL") else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let ix = d.path().join("models.json");
    Index {
        models: vec![Entry {
            name: "test/model:Q4_K_M".into(),
            repo: "test/model".into(),
            quant: "Q4_K_M".into(),
            paths: vec![path.into()],
            maker: "test".into(),
            params: None,
            architecture: "qwen35".into(),
            context: None,
            bytes: 0,
            licence: "unknown".into(),
            chat: true,
            runs: true,
            thinking: true,
            modified: 0,
            downloaded: None,
            removed: None,
        }],
    }
    .write(&ix)
    .unwrap();
    let mut e = itero_ai::llama::LlamaEngine::new(ix).unwrap();
    assert_eq!(e.loaded(), None);

    let (c, r) = ask(&mut e, "Say only this word: merhaba", None, false);
    assert!(
        c.to_lowercase().contains("merhaba") && r.is_empty(),
        "{c:?} {r:?}"
    );
    assert_eq!(e.loaded().as_deref(), Some("test/model:Q4_K_M"));

    let (c, _) = ask(
        &mut e,
        "Sort this mail from our own monitoring as urgent, later or spam: \
         'Production database down since 03:12. Customers cannot pay. On-call, please act now.' \
         Answer as a JSON string: \"urgent\", \"later\" or \"spam\".",
        Some(json!({ "type": "string", "enum": ["urgent", "later", "spam"] })),
        false,
    );
    assert_eq!(c, "\"urgent\"");

    let (c, _) = ask(
        &mut e,
        "Fatura: 1250 TL, son ödeme 2026-11-15. Answer as a JSON object: \\
         {\"amount\": <number>, \"due\": \"<YYYY-MM-DD>\"}.",
        Some(json!({ "type": "object",
            "properties": { "amount": { "type": "number" },
                            "due": { "type": "string", "format": "date" } },
            "required": ["amount", "due"], "additionalProperties": false })),
        false,
    );
    let v: serde_json::Value = serde_json::from_str(&c).unwrap();
    assert_eq!(v["amount"].as_f64(), Some(1250.0), "{c}");
    assert_eq!(v["due"], "2026-11-15", "{c}");

    let (c, r) = ask(
        &mut e,
        "What is 17 * 3? Answer with the number only.",
        None,
        true,
    );
    assert!(!r.is_empty(), "thinking comes apart: {c:?}");
    assert!(c.trim().ends_with("51"), "{c:?}");
}
