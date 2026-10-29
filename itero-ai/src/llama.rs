//! The engine: llama.cpp through llama-cpp-2. One model loaded at a time;
//! a request for another frees it first.
use crate::engine::{Engine, EngineError, Finish, Piece, Prompt, Reason};
use crate::think::Splitter;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// The context every answer has: its prompt and what it writes.
const CONTEXT: u32 = 16_384;
/// Prompt tokens decoded at once.
const BATCH: usize = 2048;
/// Tokens back the repetition penalties look.
const PENALTY_LAST: i32 = 64;

/// A model loaded, with what answering needs of it.
struct Loaded {
    name: String,
    architecture: String,
    template: String,
    model: LlamaModel,
}

pub struct LlamaEngine {
    backend: LlamaBackend,
    index: PathBuf,
    loaded: Option<Loaded>,
}

impl LlamaEngine {
    /// The engine, reading models from the index at `index`. llama.cpp's
    /// own log is quiet unless `ITERO_AI_LLAMA_LOG` is set.
    pub fn new(index: PathBuf) -> anyhow::Result<LlamaEngine> {
        let mut backend = LlamaBackend::init()?;
        if std::env::var_os("ITERO_AI_LLAMA_LOG").is_none() {
            backend.void_logs();
        }
        Ok(LlamaEngine {
            backend,
            index,
            loaded: None,
        })
    }

    fn load(&mut self, name: &str) -> Result<&Loaded, EngineError> {
        let ix = itero_ai_index::Index::read(&self.index)
            .map_err(|e| EngineError::Failed(format!("{e:#}")))?;
        let entry = itero_ai_index::resolve(&ix, name).map_err(|e| match e {
            itero_ai_index::ResolveError::NotFound(_) => EngineError::NotFound(e.to_string()),
            _ => EngineError::BadRequest(e.to_string()),
        })?;
        if self.loaded.as_ref().is_none_or(|l| l.name != entry.name) {
            // the old one is freed before the new one loads
            self.loaded = None;
            let path = &entry.paths[0];
            let params = LlamaModelParams::default().with_n_gpu_layers(999);
            let model = LlamaModel::load_from_file(&self.backend, path, &params)
                .map_err(|e| EngineError::Failed(format!("{}: {e}", path.display())))?;
            let template = model
                .chat_template(None)
                .map_err(|e| EngineError::Failed(format!("{name} has no chat template: {e}")))?
                .to_string()
                .map_err(|e| EngineError::Failed(format!("{name}'s chat template: {e}")))?;
            self.loaded = Some(Loaded {
                name: entry.name.clone(),
                architecture: entry.architecture.clone(),
                template,
                model,
            });
        }
        Ok(self.loaded.as_ref().expect("just loaded"))
    }
}

/// The prompt in the model's own template, rendered as its makers do
/// (Jinja, with Python's string methods), thinking on or off.
fn render(template: &str, p: &Prompt) -> Result<String, EngineError> {
    let bad = |e: minijinja::Error| EngineError::Failed(format!("the model's template: {e}"));
    let mut env = minijinja::Environment::new();
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_template("chat", template).map_err(bad)?;
    let messages: Vec<_> = p
        .messages
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();
    env.get_template("chat")
        .and_then(|t| {
            t.render(serde_json::json!({
                "messages": messages,
                "add_generation_prompt": true,
                "enable_thinking": p.thinking,
            }))
        })
        .map_err(bad)
}

fn failed(e: impl std::fmt::Display) -> EngineError {
    EngineError::Failed(e.to_string())
}

fn sampler(model: &LlamaModel, p: &Prompt) -> Result<LlamaSampler, EngineError> {
    let s = &p.sampling;
    let mut chain = vec![];
    let schema = p
        .schema
        .clone()
        .or_else(|| p.json.then(|| serde_json::json!({ "type": "object" })));
    if let Some(schema) = schema {
        let grammar = llama_cpp_2::json_schema_to_grammar(&schema.to_string())
            .map_err(|e| EngineError::BadRequest(format!("response_format's schema: {e}")))?;
        chain
            .push(LlamaSampler::grammar(model, &grammar, "root").map_err(|e| {
                EngineError::BadRequest(format!("response_format's schema: {e:?}"))
            })?);
    }
    // not given: llama.cpp's own defaults
    chain.push(LlamaSampler::penalties(
        model.n_vocab(),
        PENALTY_LAST,
        1.0,
        s.frequency_penalty.unwrap_or(0.0),
        s.presence_penalty.unwrap_or(0.0),
    ));
    chain.push(LlamaSampler::top_k(s.top_k.unwrap_or(40)));
    chain.push(LlamaSampler::top_p(s.top_p.unwrap_or(0.95), 1));
    chain.push(LlamaSampler::min_p(s.min_p.unwrap_or(0.05), 1));
    chain.push(LlamaSampler::temp(s.temperature.unwrap_or(0.8)));
    chain.push(LlamaSampler::dist(s.seed.unwrap_or_else(seed)));
    Ok(LlamaSampler::chain_simple(chain))
}

/// A seed when none is asked for: the clock's nanoseconds.
fn seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1)
}

impl Engine for LlamaEngine {
    fn complete(
        &mut self,
        p: &Prompt,
        sink: &mut dyn FnMut(Piece) -> bool,
    ) -> Result<Finish, EngineError> {
        self.load(&p.model)?;
        let l = self.loaded.as_ref().expect("loaded");
        let prompt = render(&l.template, p)?;
        let tokens = l.model.vocab().tokenize(prompt.as_bytes(), false, true);
        let n_prompt = tokens.len() as u32;
        if n_prompt >= CONTEXT {
            return Err(EngineError::BadRequest(format!(
                "the prompt is {n_prompt} tokens: the context holds {CONTEXT}"
            )));
        }
        let room = CONTEXT - n_prompt;
        let max = p.max_tokens.map_or(room, |m| m.min(room));
        let cparams = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT))
            .with_n_batch(BATCH as u32);
        let mut ctx = l
            .model
            .new_context(&self.backend, cparams)
            .map_err(failed)?;
        let mut batch = LlamaBatch::new(BATCH, 1);
        let mut pos = 0i32;
        for chunk in tokens.chunks(BATCH) {
            batch.clear();
            for (i, t) in chunk.iter().enumerate() {
                let at = pos + i as i32;
                batch
                    .add(*t, at, &[0], at as u32 == n_prompt - 1)
                    .map_err(failed)?;
            }
            ctx.decode(&mut batch).map_err(failed)?;
            pos += chunk.len() as i32;
        }
        let mut sampler = sampler(&l.model, p)?;
        let vocab = l.model.vocab();
        let mut split = Splitter::for_architecture(&l.architecture, p.thinking);
        // a template that opens the thinking itself, at the prompt's end
        if prompt.trim_end().ends_with("<think>") {
            split = split.inside();
        }
        let (mut n, mut reason) = (0u32, Reason::Length);
        let mut text = String::new();
        let mut bytes = vec![];
        let mut idx = batch.n_tokens() - 1;
        'answer: while n < max {
            // sampling accepts the token too: the grammar and the penalties
            // see it once
            let t = sampler.sample(&ctx, idx);
            if vocab.is_eog(t) {
                reason = Reason::Stop;
                break;
            }
            n += 1;
            vocab.token_to_piece_into(t, &mut bytes, false, None);
            // a character may come in two tokens: only whole ones go out
            let whole = match std::str::from_utf8(&bytes) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            let piece = String::from_utf8(bytes.drain(..whole).collect()).expect("checked");
            text.push_str(&piece);
            // a stop text ends the answer and is not given; one cut across
            // two pieces leaves its start in the piece before
            let stop = p.stop.iter().find(|st| text.ends_with(st.as_str()));
            let give = match stop {
                Some(st) => &piece[..piece.len().saturating_sub(st.len())],
                None => &piece,
            };
            for out in split.feed(give) {
                if !sink(out) {
                    break 'answer;
                }
            }
            if stop.is_some() {
                reason = Reason::Stop;
                break;
            }
            batch.clear();
            batch.add(t, pos, &[0], true).map_err(failed)?;
            pos += 1;
            ctx.decode(&mut batch).map_err(failed)?;
            idx = 0;
        }
        for out in split.end() {
            sink(out);
        }
        Ok(Finish {
            reason,
            prompt_tokens: n_prompt,
            completion_tokens: n,
        })
    }

    fn inspect(&mut self, path: &Path) -> Result<serde_json::Value, EngineError> {
        crate::inspect::inspect(&self.backend, path)
    }

    fn loaded(&self) -> Option<String> {
        self.loaded.as_ref().map(|l| l.name.clone())
    }
}
