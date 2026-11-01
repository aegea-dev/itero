//! One GGUF file's header: what it is, and whether this build can run it.
//! Only the header and the vocabulary are read, never the weights.
use crate::engine::EngineError;
use llama_cpp_2::gguf::GgufContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use serde_json::{Value, json};
use std::path::Path;

/// `general.file_type` (llama.cpp's `llama_ftype`) as the quantization
/// names files carry.
const FILE_TYPES: [(u64, &str); 16] = [
    (1, "F16"),
    (2, "Q4_0"),
    (3, "Q4_1"),
    (7, "Q8_0"),
    (8, "Q5_0"),
    (9, "Q5_1"),
    (10, "Q2_K"),
    (11, "Q3_K_S"),
    (12, "Q3_K_M"),
    (13, "Q3_K_L"),
    (14, "Q4_K_S"),
    (15, "Q4_K_M"),
    (16, "Q5_K_S"),
    (17, "Q5_K_M"),
    (18, "Q6_K"),
    (32, "BF16"),
];

/// GGUF's value types for whole numbers, as its header numbers them.
const UINT32: u32 = 4;
const INT32: u32 = 5;
const UINT64: u32 = 10;
const INT64: u32 = 11;

fn text(g: &GgufContext, key: &str) -> Option<String> {
    let i = g.find_key(key);
    (i >= 0).then(|| g.val_str(i).map(String::from)).flatten()
}

fn whole(g: &GgufContext, key: &str) -> Option<u64> {
    let i = g.find_key(key);
    if i < 0 {
        return None;
    }
    match g.kv_type(i) {
        UINT32 => Some(u64::from(g.val_u32(i))),
        INT32 => u64::try_from(g.val_i32(i)).ok(),
        UINT64 | INT64 => Some(g.val_u64(i)),
        _ => None,
    }
}

/// A number in the header, through llama.cpp's own text of it, rounded:
/// a file keeps 0.95 as 0.949999988.
fn sampling_value(m: &LlamaModel, key: &str) -> Option<f64> {
    let v: f64 = m.meta_val_str(key).ok()?.trim().parse().ok()?;
    Some((v * 1000.0).round() / 1000.0)
}

/// The sampling a file names, read from the model: `GgufContext` reads no
/// floats. A model this build cannot load has none.
fn sampling(m: &LlamaModel) -> Option<Value> {
    let got: serde_json::Map<String, Value> = [
        ("temperature", "general.sampling.temp"),
        ("top_p", "general.sampling.top_p"),
        ("top_k", "general.sampling.top_k"),
        ("min_p", "general.sampling.min_p"),
    ]
    .into_iter()
    .filter_map(|(k, key)| Some((k.to_string(), json!(sampling_value(m, key)?))))
    .collect();
    (!got.is_empty()).then_some(Value::Object(got))
}

pub fn inspect(backend: &LlamaBackend, path: &Path) -> Result<Value, EngineError> {
    let bytes = std::fs::metadata(path)
        .map_err(|e| EngineError::NotFound(format!("{}: {e}", path.display())))?
        .len();
    let g = GgufContext::from_file(path)
        .ok_or_else(|| EngineError::BadRequest(format!("{}: not a GGUF file", path.display())))?;
    let arch = text(&g, "general.architecture").unwrap_or_else(|| "unknown".into());
    let quant = whole(&g, "general.file_type")
        .and_then(|t| FILE_TYPES.iter().find(|(n, _)| *n == t))
        .map_or("unknown", |(_, q)| q);
    let vocab = LlamaModel::load_from_file(
        backend,
        path,
        &LlamaModelParams::default().with_vocab_only(true),
    );
    let template = text(&g, "tokenizer.chat_template");
    Ok(json!({
        "maker": text(&g, "general.organization"),
        "basename": text(&g, "general.basename").or_else(|| text(&g, "general.name")),
        "params": text(&g, "general.size_label"),
        "architecture": arch,
        "quant": quant,
        "context": whole(&g, &format!("{arch}.context_length")),
        "bytes": bytes,
        "licence": text(&g, "general.license").unwrap_or_else(|| "unknown".into()),
        "chat": g.find_key("tokenizer.chat_template") >= 0,
        "runs": vocab.is_ok(),
        "sampling": vocab.as_ref().ok().and_then(sampling),
        "thinking_off": template.is_some_and(|t| t.contains("enable_thinking")),
        "thinking": crate::think::splits(&arch),
    }))
}
