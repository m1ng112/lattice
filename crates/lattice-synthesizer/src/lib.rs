//! AI-driven code synthesis for the Lattice language.
//!
//! This crate extracts synthesis intents from the Lattice AST,
//! builds structured prompts for LLM-based code generation,
//! and runs a generate-and-verify loop with proof checking.

pub mod cache;
pub mod client;
pub mod engine;
pub mod error;
pub mod extractor;
pub mod optimizer;
pub mod prompt;
pub mod types;

pub use cache::SynthesisCache;
pub use client::{LlmClient, LlmProvider};
pub use engine::Synthesizer;
pub use error::SynthesisError;
pub use types::{SynthesisRequest, SynthesisResult};
