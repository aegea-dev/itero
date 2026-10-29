//! itero-ai: a local model for itero, behind the OpenAI Chat Completions
//! API on a Unix socket. itero starts it and tells it what it needs in its
//! environment; it has no commands of its own.
pub mod api;
pub mod engine;
pub mod inspect;
pub mod llama;
pub mod server;
pub mod think;
