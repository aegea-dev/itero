//! What the server asks of an engine, and what it gets back: the shapes
//! between the OpenAI API and llama.cpp, so the server is tested with a
//! fake engine.
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// `system`, `user` or `assistant`.
    pub role: String,
    pub content: String,
}

/// Sampling a request asks for; what it leaves out, the engine's defaults.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sampling {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub min_p: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub seed: Option<u32>,
}

/// One request for an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    /// The model's name, as the index has it: `<org>/<repo>[:<QUANT>]`.
    pub model: String,
    pub messages: Vec<Message>,
    pub sampling: Sampling,
    pub max_tokens: Option<u32>,
    /// Texts that end the answer; the text itself is not given.
    pub stop: Vec<String>,
    /// A JSON schema the answer matches.
    pub schema: Option<serde_json::Value>,
    /// The answer is a JSON value (with `schema`, that schema's).
    pub json: bool,
    /// The model may think first; its thinking comes apart from its answer.
    pub thinking: bool,
}

/// A piece of an answer, as it comes.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Content(String),
    Reasoning(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reason {
    /// The model ended it, or a stop text did.
    Stop,
    /// `max_tokens`, or the context, ran out.
    Length,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finish {
    pub reason: Reason,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    /// No such model on this machine (or no such file).
    NotFound(String),
    /// The request cannot be answered as asked.
    BadRequest(String),
    Failed(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotFound(m) | EngineError::BadRequest(m) | EngineError::Failed(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for EngineError {}

pub trait Engine: Send + 'static {
    /// Answers `p`, giving each piece to `sink` as it comes; `sink` says
    /// `false` when nobody is listening any more, and the answer stops.
    fn complete(
        &mut self,
        p: &Prompt,
        sink: &mut dyn FnMut(Piece) -> bool,
    ) -> Result<Finish, EngineError>;
    /// One model file's header, as JSON.
    fn inspect(&mut self, path: &Path) -> Result<serde_json::Value, EngineError>;
    /// The model loaded now, by its name.
    fn loaded(&self) -> Option<String>;
}

/// An engine for tests: knows the models in `models`, and answers
/// `pieces` to any prompt for one of them.
#[derive(Debug, Default)]
pub struct FakeEngine {
    pub models: Vec<String>,
    pub pieces: Vec<Piece>,
    pub loaded: Option<String>,
}

impl Engine for FakeEngine {
    fn complete(
        &mut self,
        p: &Prompt,
        sink: &mut dyn FnMut(Piece) -> bool,
    ) -> Result<Finish, EngineError> {
        if !self.models.contains(&p.model) {
            return Err(EngineError::NotFound(format!(
                "no model {} on this machine",
                p.model
            )));
        }
        self.loaded = Some(p.model.clone());
        let mut n = 0;
        for piece in self.pieces.clone() {
            n += 1;
            if !sink(piece) {
                break;
            }
        }
        Ok(Finish {
            reason: Reason::Stop,
            prompt_tokens: 3,
            completion_tokens: n,
        })
    }

    fn inspect(&mut self, path: &Path) -> Result<serde_json::Value, EngineError> {
        Ok(serde_json::json!({ "path": path }))
    }

    fn loaded(&self) -> Option<String> {
        self.loaded.clone()
    }
}
