//! itero-ai with its advised model, Qwen3.5-4B, at temperature 0 so each
//! answer is the same every run. Local only: they run when
//! ITERO_AI_TEST_MODEL names Qwen3.5-4B's GGUF file (`make test-ai` names
//! LM Studio's copy), never in CI, and a file of another model is refused.
//!
//! llama.cpp's backend starts once a process, so the tests share one
//! engine, one at a time. A schema holds an answer to its form, and the
//! prompt says that form too, as `#ai` does: a model made to open a JSON
//! string it did not mean to write answers poorly.
use itero_ai::engine::{Engine, Finish, Message, Piece, Prompt, Reason, Sampling};
use itero_ai::llama::LlamaEngine;
use itero_ai_index::{Entry, Index};
use serde_json::{Value, json};

const MODEL: &str = "unsloth/Qwen3.5-4B-GGUF";

/// The engine on the Qwen3.5-4B file ITERO_AI_TEST_MODEL names, with its
/// index's folder; `None` when it names none.
fn engine() -> Option<(tempfile::TempDir, LlamaEngine)> {
    let path = std::env::var("ITERO_AI_TEST_MODEL").ok()?;
    let d = tempfile::tempdir().unwrap();
    let ix = d.path().join("models.json");
    Index {
        models: vec![Entry {
            name: format!("{MODEL}:Q4_K_M"),
            repo: MODEL.into(),
            quant: "Q4_K_M".into(),
            paths: vec![path.clone().into()],
            maker: "unsloth".into(),
            params: Some("4B".into()),
            architecture: "qwen35".into(),
            context: None,
            bytes: 0,
            licence: "apache-2.0".into(),
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
    let mut e = LlamaEngine::new(ix).unwrap();
    // these tests are Qwen3.5-4B's: its answers are what they expect
    let v = e.inspect(path.as_ref()).unwrap();
    assert_eq!(
        (v["basename"].as_str(), v["params"].as_str()),
        (Some("Qwen3.5-4B"), Some("4B")),
        "ITERO_AI_TEST_MODEL is not Qwen3.5-4B: {path}"
    );
    Some((d, e))
}

/// Greedy: temperature 0, every other sampler left wide open.
fn greedy() -> Sampling {
    Sampling {
        temperature: Some(0.0),
        top_p: Some(1.0),
        top_k: Some(0),
        min_p: Some(0.0),
        presence_penalty: None,
        frequency_penalty: None,
        seed: Some(1),
    }
}

fn prompt(text: &str) -> Prompt {
    Prompt {
        model: MODEL.into(),
        messages: vec![Message {
            role: "user".into(),
            content: text.into(),
        }],
        sampling: greedy(),
        max_tokens: Some(256),
        stop: vec![],
        schema: None,
        json: false,
        thinking: false,
    }
}

/// The answer, its thinking, and how it ended.
fn ask(e: &mut LlamaEngine, p: &Prompt) -> (String, String, Finish) {
    let (mut c, mut r) = (String::new(), String::new());
    let f = e
        .complete(p, &mut |piece| {
            match piece {
                Piece::Content(t) => c.push_str(&t),
                Piece::Reasoning(t) => r.push_str(&t),
            }
            true
        })
        .unwrap();
    (c, r, f)
}

fn choose(e: &mut LlamaEngine, mail: &str) -> String {
    let mut p = prompt(&format!(
        "Sort this mail to our team as urgent, later or spam: '{mail}' \
         Answer as a JSON string: \"urgent\", \"later\" or \"spam\"."
    ));
    p.schema = Some(json!({ "type": "string", "enum": ["urgent", "later", "spam"] }));
    p.json = true;
    let (c, _, _) = ask(e, &p);
    serde_json::from_str(&c).unwrap()
}

/// Fails a case with what it saw.
macro_rules! ensure {
    ($cond:expr, $($why:tt)+) => {
        if !$cond {
            return Err(format!($($why)+));
        }
    };
}

fn a_plain_answer_without_thinking(e: &mut LlamaEngine) -> Result<(), String> {
    let (c, r, f) = ask(e, &prompt("Say only this word: merhaba"));
    ensure!(c.to_lowercase().contains("merhaba"), "{c:?}");
    ensure!(r.is_empty(), "no thinking asked: {r:?}");
    ensure!(f.reason == Reason::Stop, "{:?}", f.reason);
    Ok(())
}

fn the_same_question_gets_the_same_answer(e: &mut LlamaEngine) -> Result<(), String> {
    let p = prompt("Name three Turkish cities, comma-separated, nothing else.");
    let (a, _, _) = ask(e, &p);
    let (b, _, _) = ask(e, &p);
    ensure!(a == b, "{a:?} then {b:?}");
    ensure!(a.contains(','), "{a:?}");
    Ok(())
}

fn choose_sorts_three_mails(e: &mut LlamaEngine) -> Result<(), String> {
    let outage =
        "Production database down since 03:12. Customers cannot pay. On-call, please act now.";
    let got = choose(e, outage);
    ensure!(got == "urgent", "the outage: {got}");
    let spam = "CONGRATULATIONS!!! You won a free cruise. Click here to claim your prize today.";
    let got = choose(e, spam);
    ensure!(got == "spam", "the spam: {got}");
    // A mail it finds borderline goes to the label listed first: at
    // temperature 0 the moved meeting is "urgent" listed urgent, later,
    // spam, and "later" with later first, grammar or not (measured
    // 2026-10-29). Only "not spam" is its own.
    let meeting = "Hi team, the monthly planning meeting moves to next Thursday 14:00. \
                   Agenda to follow. No action needed.";
    let got = choose(e, meeting);
    ensure!(got != "spam", "the meeting: {got}");
    Ok(())
}

fn fields_of_a_turkish_invoice(e: &mut LlamaEngine) -> Result<(), String> {
    let mut p = prompt(
        "Fatura: 1250 TL, son ödeme 2026-11-15. Ödenmezse hizmet kesilecek. \
         Answer as a JSON object: {\"amount\": <number>, \"due\": \"<YYYY-MM-DD>\", \"urgent\": <true or false>}.",
    );
    p.schema = Some(json!({ "type": "object",
        "properties": { "amount": { "type": "number" },
                        "due": { "type": "string", "format": "date" },
                        "urgent": { "type": "boolean" } },
        "required": ["amount", "due", "urgent"], "additionalProperties": false }));
    p.json = true;
    let (c, _, _) = ask(e, &p);
    let v: Value = serde_json::from_str(&c).map_err(|err| format!("{err}: {c}"))?;
    ensure!(v["amount"].as_f64() == Some(1250.0), "{c}");
    ensure!(v["due"] == "2026-11-15", "{c}");
    ensure!(v["urgent"].is_boolean(), "{c}");
    Ok(())
}

fn thinking_comes_apart_from_the_answer(e: &mut LlamaEngine) -> Result<(), String> {
    let mut p = prompt("What is 17 * 3? Answer with the number only.");
    p.thinking = true;
    p.max_tokens = Some(4096);
    let (c, r, f) = ask(e, &p);
    ensure!(!r.is_empty(), "no thinking: {c:?}");
    ensure!(f.reason == Reason::Stop, "thinking did not end: {r}");
    ensure!(c.trim() == "51", "{c:?}");
    Ok(())
}

fn a_stop_text_and_max_tokens_end_it(e: &mut LlamaEngine) -> Result<(), String> {
    let mut p = prompt("Count from 1 to 10, one number a line, nothing else.");
    p.stop = vec!["5".into()];
    let (c, _, f) = ask(e, &p);
    ensure!(f.reason == Reason::Stop, "{:?}", f.reason);
    ensure!(c.contains('4') && !c.contains('5'), "{c:?}");
    let mut p = prompt("Write a long story about the sea.");
    p.max_tokens = Some(8);
    let (_, _, f) = ask(e, &p);
    ensure!(
        (f.reason, f.completion_tokens) == (Reason::Length, 8),
        "{:?} after {}",
        f.reason,
        f.completion_tokens
    );
    Ok(())
}

type Case = fn(&mut LlamaEngine) -> Result<(), String>;

/// Every case on one engine, which is freed at the end (llama.cpp aborts
/// an exit with a model loaded); each case is told by name.
#[test]
fn qwen3_5_4b_at_temperature_0() {
    let Some((_d, mut e)) = engine() else {
        return;
    };
    let cases: [(&str, Case); 6] = [
        (
            "a plain answer without thinking",
            a_plain_answer_without_thinking,
        ),
        (
            "the same question gets the same answer",
            the_same_question_gets_the_same_answer,
        ),
        ("choose sorts three mails", choose_sorts_three_mails),
        ("fields of a Turkish invoice", fields_of_a_turkish_invoice),
        (
            "thinking comes apart from the answer",
            thinking_comes_apart_from_the_answer,
        ),
        (
            "a stop text and max_tokens end it",
            a_stop_text_and_max_tokens_end_it,
        ),
    ];
    let mut failed = vec![];
    for (name, case) in cases {
        match case(&mut e) {
            Ok(()) => eprintln!("ok: {name}"),
            Err(why) => {
                eprintln!("FAILED: {name}: {why}");
                failed.push(format!("{name}: {why}"));
            }
        }
    }
    drop(e);
    assert!(failed.is_empty(), "{failed:#?}");
}
