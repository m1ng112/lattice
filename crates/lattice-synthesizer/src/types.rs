//! Core types for AI-driven code synthesis.

use serde::{Deserialize, Serialize};

/// Strategy hint for how synthesized code should handle concurrency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthesisStrategy {
    PessimisticLocking,
    OptimisticLocking,
    LockFree,
    Custom(String),
}

impl SynthesisStrategy {
    /// Parse a strategy from a Lattice identifier string.
    pub fn from_ident(s: &str) -> Self {
        match s {
            "pessimistic_locking" => Self::PessimisticLocking,
            "optimistic_locking" => Self::OptimisticLocking,
            "lock_free" => Self::LockFree,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// What the synthesized code should be optimized for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimizationTarget {
    Latency,
    Throughput,
    Memory,
    TimeComplexity,
    Custom(String),
}

impl OptimizationTarget {
    /// Parse an optimization target from a Lattice identifier string.
    pub fn from_ident(s: &str) -> Self {
        match s {
            "latency" => Self::Latency,
            "throughput" => Self::Throughput,
            "memory" => Self::Memory,
            "time_complexity" => Self::TimeComplexity,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// A request to synthesize a function implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisRequest {
    /// Name of the function to synthesize.
    pub function_name: String,
    /// Parameters as `(name, type_string)` pairs.
    pub parameters: Vec<(String, String)>,
    /// Return type as a string, or `"()"` if none.
    pub return_type: String,
    /// Human-readable precondition strings.
    pub preconditions: Vec<String>,
    /// Human-readable postcondition strings.
    pub postconditions: Vec<String>,
    /// Human-readable invariant strings.
    pub invariants: Vec<String>,
    /// Concurrency strategy hint from `synthesize(strategy: ...)`.
    pub strategy: Option<SynthesisStrategy>,
    /// Optimization target from `synthesize(optimize: ...)`.
    pub optimize: Option<OptimizationTarget>,
}

/// The result of a synthesis attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SynthesisResult {
    /// Successfully synthesized code.
    Synthesized {
        code: String,
        verified: bool,
        attempts: u32,
    },
    /// Synthesis not possible; manual implementation required.
    ManualRequired { reason: String },
    /// Retrieved from cache.
    Cached { code: String, cache_key: String },
}
